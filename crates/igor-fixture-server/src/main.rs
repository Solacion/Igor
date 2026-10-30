//! `igor-fixture-server`: serves one fixture file as an MCP server over stdio.
//!
//! ```text
//! igor-fixture-server --fixture <path> [--call-log <path>]
//! ```
//!
//! `IGOR_FIXTURE_FILE` and `IGOR_FIXTURE_CALL_LOG` are used when the flags are absent. Stdout
//! carries only MCP messages; errors go to stderr.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use igor_fixture_server::{CallLog, CallLogError, Fixture, FixtureError, FixtureServer};
use rmcp::ServiceExt;
use thiserror::Error;

const FIXTURE_ENV: &str = "IGOR_FIXTURE_FILE";
const CALL_LOG_ENV: &str = "IGOR_FIXTURE_CALL_LOG";
const USAGE: &str = "usage: igor-fixture-server --fixture <path> [--call-log <path>]";

#[derive(Debug, Error)]
enum StartError {
    #[error("{0}\n{USAGE}")]
    Usage(String),
    #[error(transparent)]
    Fixture(#[from] FixtureError),
    #[error(transparent)]
    CallLog(#[from] CallLogError),
    #[error("MCP session failed: {0}")]
    Session(String),
}

struct Options {
    fixture: PathBuf,
    call_log: Option<PathBuf>,
}

fn parse_args(mut args: impl Iterator<Item = OsString>) -> Result<Options, StartError> {
    let mut fixture = std::env::var_os(FIXTURE_ENV).map(PathBuf::from);
    let mut call_log = std::env::var_os(CALL_LOG_ENV).map(PathBuf::from);
    while let Some(arg) = args.next() {
        let slot = match arg.to_str() {
            Some("--fixture") => &mut fixture,
            Some("--call-log") => &mut call_log,
            _ => {
                return Err(StartError::Usage(format!(
                    "unexpected argument {}",
                    arg.to_string_lossy()
                )));
            }
        };
        let value = args
            .next()
            .ok_or_else(|| StartError::Usage(format!("{} needs a value", arg.to_string_lossy())))?;
        *slot = Some(PathBuf::from(value));
    }
    let fixture = fixture.ok_or_else(|| {
        StartError::Usage(format!(
            "no fixture file: pass --fixture or set {FIXTURE_ENV}"
        ))
    })?;
    Ok(Options { fixture, call_log })
}

async fn run() -> Result<(), StartError> {
    let options = parse_args(std::env::args_os().skip(1))?;
    let fixture = Fixture::load(&options.fixture)?;
    let call_log = match &options.call_log {
        Some(path) => Some(CallLog::create(path).await?),
        None => None,
    };
    let service = FixtureServer::new(fixture, call_log)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| StartError::Session(e.to_string()))?;
    service
        .waiting()
        .await
        .map_err(|e| StartError::Session(e.to_string()))?;
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("igor-fixture-server: {error}");
            ExitCode::FAILURE
        }
    }
}
