//! `igor-cli`: headless IGOR harness over the core session API, and the fallback demo.
//!
//! Loads the configuration, connects the configured MCP servers, runs one request through a
//! [`Session`], prints the runtime events as plain text, and asks the operator in the terminal
//! to approve write actions and answer clarification questions. After each turn it reads a
//! follow-up message, continuing the conversation until end of input or an empty line. With `--export`, the events are
//! also written to a file as redacted JSON Lines.
//!
//! Exit codes: 0 completed, 1 startup error, 2 the turn failed, 3 a limit stopped the turn,
//! 4 cancelled.

mod args;
mod operator;
mod render;
mod script;
mod setup;

use std::io::Write;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use igor_config::{Config, ConfigError, ProviderConfig, ProviderKind, SecretError};
use igor_core::CancellationToken;
use igor_core::events::TurnOutcome;
use igor_core::ids::SequentialIds;
use igor_core::limits::{LimitTooSmall, ResultSizeLimit};
use igor_core::provider::Provider;
use igor_core::session::{Session, SessionConfig, SessionParts};
use igor_core::testing::ScriptedProvider;
use igor_provider_anthropic::{AnthropicProvider, AnthropicSettings};
use igor_provider_openai::{OpenAiCompatibleProvider, OpenAiSettings};
use thiserror::Error;
use tokio::io::BufReader;
use tokio::sync::mpsc;

use crate::args::{Args, ProviderChoice, USAGE};

const EXIT_STARTUP: u8 = 1;
const EXIT_FAILED: u8 = 2;
const EXIT_LIMIT: u8 = 3;
const EXIT_CANCELLED: u8 = 4;

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}\n{USAGE}")]
    Usage(#[from] args::ArgsError),
    #[error("no configuration file: pass --config or set IGOR_CONFIG")]
    NoConfig,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Script(#[from] script::ScriptError),
    #[error("provider {0:?} is not in the configuration")]
    UnknownProvider(String),
    #[error("provider {id:?} needs an api_key")]
    MissingApiKey { id: String },
    #[error("provider {id:?}: {source}")]
    ApiKey { id: String, source: SecretError },
    #[error("provider {id:?} could not be created: {message}")]
    ProviderBuild { id: String, message: String },
    #[error("invalid limits: {0}")]
    Limits(#[from] LimitTooSmall),
    #[error("cannot create export file {}: {source}", path.display())]
    Export {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("no request: pass it as arguments or on stdin")]
    EmptyRequest,
    #[error("terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("igor-cli: {error}");
            ExitCode::from(EXIT_STARTUP)
        }
    }
}

async fn run() -> Result<ExitCode, CliError> {
    let args = args::parse(std::env::args_os().skip(1))?;
    let config_path = igor_config::config_path(args.config.as_deref()).ok_or(CliError::NoConfig)?;
    let config = Config::load(&config_path)?;
    let (provider, scripted) = build_provider(&args, &config)?;

    let mut export = args
        .export
        .as_ref()
        .map(|path| {
            std::fs::File::create(path)
                .map(std::io::BufWriter::new)
                .map_err(|source| CliError::Export {
                    path: path.clone(),
                    source,
                })
        })
        .transpose()?;

    let mut stdout = std::io::stdout();
    let connected = setup::connect(&config, &mut stdout).await?;
    let (sink, mut events) = mpsc::unbounded_channel();
    let (mut session, handle) = Session::new(
        SessionParts {
            provider,
            registry: connected.registry,
            backends: connected.backends,
            ids: Arc::new(SequentialIds::new("call")),
            events: Arc::new(sink),
        },
        session_config(&config)?,
    );

    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut request = match args.request {
        Some(request) => request,
        None => read_message(&mut stdin, &mut stdout, "Request: ")
            .await?
            .ok_or(CliError::EmptyRequest)?,
    };

    // Ctrl-C cancels the turn in progress; the conversation can continue afterwards.
    let current = Arc::new(Mutex::new(CancellationToken::new()));
    let on_interrupt = Arc::clone(&current);
    tokio::spawn(async move {
        while tokio::signal::ctrl_c().await.is_ok() {
            if let Ok(token) = on_interrupt.lock() {
                token.cancel();
            }
        }
    });
    let conversation: Result<TurnOutcome, CliError> = async {
        loop {
            let cancel = CancellationToken::new();
            if let Ok(mut slot) = current.lock() {
                *slot = cancel.clone();
            }
            let (outcome, operated) = tokio::join!(
                session.send_message(request, cancel.clone()),
                operator::operate(
                    &mut events,
                    &handle,
                    &mut stdin,
                    &mut stdout,
                    export.as_mut().map(|file| file as &mut dyn Write),
                    &cancel,
                )
            );
            operated?;
            // Follow-up messages continue the same conversation until end of input or an empty
            // line.
            match read_message(&mut stdin, &mut stdout, "\nYou: ").await? {
                Some(next) => request = next,
                None => break Ok(outcome),
            }
        }
    }
    .await;
    // Stop every server, and wait for it, on every path out of the conversation.
    session.shutdown().await;
    let outcome = conversation?;
    if let Some(Err(error)) = scripted.map(|provider| provider.verify()) {
        eprintln!("igor-cli: the script did not match the run: {error}");
    }
    Ok(exit_code(&outcome))
}

/// Prompts and reads one message; `None` at end of input or on an empty line.
async fn read_message(
    stdin: &mut BufReader<tokio::io::Stdin>,
    stdout: &mut std::io::Stdout,
    prompt: &str,
) -> std::io::Result<Option<String>> {
    write!(stdout, "{prompt}")?;
    stdout.flush()?;
    Ok(operator::read_line(stdin)
        .await?
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty()))
}

/// The provider for the session, and the same provider again when it is scripted, so the
/// script can be verified after the run.
type Providers = (Arc<dyn Provider>, Option<Arc<ScriptedProvider>>);

/// The scripted provider, or a configured Anthropic or OpenAI-compatible (such as LM Studio)
/// provider.
fn build_provider(args: &Args, config: &Config) -> Result<Providers, CliError> {
    match &args.provider {
        ProviderChoice::Scripted { script } => {
            let provider = Arc::new(script::load(script)?);
            Ok((provider.clone(), Some(provider)))
        }
        ProviderChoice::Configured(id) => {
            let configured = config
                .provider(id)
                .ok_or_else(|| CliError::UnknownProvider(id.clone()))?;
            Ok((configured_provider(configured)?, None))
        }
    }
}

fn configured_provider(config: &ProviderConfig) -> Result<Arc<dyn Provider>, CliError> {
    let id = || config.id.clone();
    let api_key = config
        .api_key
        .as_ref()
        .map(|key| key.resolve())
        .transpose()
        .map_err(|source| CliError::ApiKey { id: id(), source })?;
    let built = match config.kind {
        ProviderKind::Anthropic => {
            let api_key = api_key.ok_or_else(|| CliError::MissingApiKey { id: id() })?;
            AnthropicSettings::from_config(config, api_key)
                .and_then(AnthropicProvider::new)
                .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
                .map_err(|e| e.to_string())
        }
        ProviderKind::OpenAiCompatible => OpenAiSettings::from_config(config, api_key)
            .and_then(OpenAiCompatibleProvider::new)
            .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
            .map_err(|e| e.to_string()),
    };
    built.map_err(|message| CliError::ProviderBuild { id: id(), message })
}

fn session_config(config: &Config) -> Result<SessionConfig, LimitTooSmall> {
    let limits = config.limits();
    Ok(SessionConfig {
        max_model_requests: limits.max_iterations,
        tool_timeout: limits.tool_call_timeout,
        result_limit: ResultSizeLimit::new(limits.max_result_bytes)?,
        ..SessionConfig::default()
    })
}

fn exit_code(outcome: &TurnOutcome) -> ExitCode {
    match outcome {
        TurnOutcome::Completed => ExitCode::SUCCESS,
        TurnOutcome::Failed { .. } => ExitCode::from(EXIT_FAILED),
        TurnOutcome::LimitReached { .. } => ExitCode::from(EXIT_LIMIT),
        TurnOutcome::Cancelled => ExitCode::from(EXIT_CANCELLED),
    }
}
