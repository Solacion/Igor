//! Desktop composition only; permissions, orchestration and approval stay in igor-core.

use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use igor_config::{Config, ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::ids::SequentialIds;
use igor_core::limits::ResultSizeLimit;
use igor_core::provider::Provider;
use igor_core::registry::ToolRegistry;
use igor_core::session::{SessionConfig, SessionParts};
use igor_core::tools::ToolBackend;
use igor_mcp::{McpBackend, StdioServerConfig};
use igor_provider_anthropic::{AnthropicProvider, AnthropicSettings};
use igor_provider_openai::{OpenAiCompatibleProvider, OpenAiSettings};

use crate::sessions::{BridgeError, ServerStatus, StartRequest};

pub struct Prepared {
    pub parts: SessionParts,
    pub config: SessionConfig,
    pub servers: Vec<Arc<McpBackend>>,
    pub statuses: Vec<ServerStatus>,
    pub provider_id: String,
    pub model: String,
    pub secrets: Vec<Secret>,
}

pub async fn prepare(
    request: StartRequest,
    cancel: &CancellationToken,
) -> Result<Prepared, BridgeError> {
    let config = tokio::task::spawn_blocking(move || {
        Config::load(std::path::Path::new(&request.config_path))
    })
    .await
    .map_err(|_| BridgeError::Configuration)?
    .map_err(|_| BridgeError::Configuration)?;
    let configured = config
        .provider(&request.provider_id)
        .ok_or(BridgeError::Provider)?;
    let mut secrets = Vec::new();
    let provider = provider(configured, &mut secrets)?;
    let mut servers = Vec::new();
    let mut statuses = Vec::new();
    let mut descriptors = Vec::new();
    let mut backends: HashMap<_, Arc<dyn ToolBackend>> = HashMap::new();
    for server in config.servers() {
        if cancel.is_cancelled() {
            return Err(BridgeError::Cancelled);
        }
        let mut stdio = StdioServerConfig::new(&server.command);
        stdio.args = server.args.iter().map(OsString::from).collect();
        stdio.cwd = Some(server.cwd.clone());
        stdio.env_allowlist = server.env_allowlist.clone();
        stdio.startup_timeout = server.startup_timeout;
        stdio.call_timeout = server.call_timeout;
        for (name, reference) in &server.env {
            let secret = reference
                .resolve()
                .map_err(|_| BridgeError::Configuration)?;
            stdio
                .env
                .push((name.clone(), OsString::from(secret.expose())));
            secrets.push(secret);
        }
        let connected = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(BridgeError::Cancelled),
            result = McpBackend::connect(server.id.clone(), &stdio) => result,
        };
        let mut count = 0;
        let mut ready = false;
        if let Ok(backend) = connected {
            let backend = Arc::new(backend);
            let tools = tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(BridgeError::Cancelled),
                result = backend.list_tools() => result,
            };
            if let Ok(tools) = tools {
                count = tools.len();
                ready = true;
                descriptors.extend(tools);
                backends.insert(server.id.clone(), backend.clone());
                servers.push(backend);
            }
        }
        statuses.push(ServerStatus {
            server_id: server.id.to_string(),
            connected: ready,
            tool_count: count,
        });
    }
    let classes = descriptors
        .iter()
        .map(|tool| (tool.id.clone(), config.permissions().classify(&tool.id)))
        .collect();
    let limits = config.limits();
    let (events, _unused) = tokio::sync::mpsc::unbounded_channel();
    Ok(Prepared {
        parts: SessionParts {
            provider,
            registry: ToolRegistry::build(descriptors, &classes),
            backends,
            ids: Arc::new(SequentialIds::new("call")),
            events: Arc::new(events),
        },
        config: SessionConfig {
            max_model_requests: limits.max_iterations,
            tool_timeout: limits.tool_call_timeout,
            model_timeout: Duration::from_secs(120),
            result_limit: ResultSizeLimit::new(limits.max_result_bytes)
                .map_err(|_| BridgeError::Configuration)?,
            ..SessionConfig::default()
        },
        provider_id: configured.id.clone(),
        model: configured.model.clone(),
        servers,
        statuses,
        secrets,
    })
}

fn provider(
    config: &ProviderConfig,
    secrets: &mut Vec<Secret>,
) -> Result<Arc<dyn Provider>, BridgeError> {
    let key = config
        .api_key
        .as_ref()
        .map(|reference| reference.resolve())
        .transpose()
        .map_err(|_| BridgeError::Provider)?;
    if let Some(key) = &key {
        secrets.push(Secret::new(key.expose()));
    }
    match config.kind {
        ProviderKind::Anthropic => {
            AnthropicSettings::from_config(config, key.ok_or(BridgeError::Provider)?)
                .and_then(AnthropicProvider::new)
                .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
                .map_err(|_| BridgeError::Provider)
        }
        ProviderKind::OpenAiCompatible => OpenAiSettings::from_config(config, key)
            .and_then(OpenAiCompatibleProvider::new)
            .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
            .map_err(|_| BridgeError::Provider),
    }
}
