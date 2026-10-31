//! MCP client for IGOR.
//!
//! [`McpBackend`] connects to one MCP server, started as a stdio child process, and implements
//! [`igor_core::tools::ToolBackend`] over it with `rmcp`. It maps MCP types to core types and MCP
//! failures to [`BackendError`], so the core never sees the MCP library.
//!
//! The child process gets a cleared environment: only variables named in
//! [`StdioServerConfig::env_allowlist`] are copied from IGOR's environment, plus those set
//! explicitly in [`StdioServerConfig::env`]. API keys and other secrets in IGOR's environment are
//! therefore not inherited.
//!
//! The child's stderr is kept as bounded, redacted diagnostics: see [`McpBackend::recent_stderr`]
//! and [`ConnectError::stderr`].

mod stderr;

pub use stderr::{MAX_STDERR_LINE_CHARS, MAX_STDERR_LINES, REDACTED};

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use igor_core::CancellationToken;
use igor_core::tools::{
    BackendError, ServerId, ToolBackend, ToolContent, ToolDescriptor, ToolHints, ToolId, ToolOutput,
};
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Tool};
use rmcp::service::NotificationContext;
use rmcp::service::{Peer, RunningService, ServiceError};
use rmcp::transport::TokioChildProcess;
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::process::Command;

/// Default bound on starting the server and completing `initialize`.
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Default bound on each `tools/list` or `tools/call` request.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// How to start one stdio MCP server.
#[derive(Clone)]
pub struct StdioServerConfig {
    /// Program to run.
    pub command: PathBuf,
    /// Program arguments.
    pub args: Vec<OsString>,
    /// Working directory; IGOR's own when `None`.
    pub cwd: Option<PathBuf>,
    /// Names of IGOR's environment variables the child inherits, when set. Nothing else is
    /// inherited.
    pub env_allowlist: Vec<String>,
    /// Variables set in the child, such as resolved secrets. Values never appear in `Debug`
    /// output.
    pub env: Vec<(String, OsString)>,
    /// Bound on spawning the process and completing `initialize`.
    pub startup_timeout: Duration,
    /// Bound on each request after startup.
    pub call_timeout: Duration,
}

impl StdioServerConfig {
    /// A configuration that runs `command` with no arguments, an empty environment and the
    /// default timeouts.
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env_allowlist: Vec::new(),
            env: Vec::new(),
            startup_timeout: DEFAULT_STARTUP_TIMEOUT,
            call_timeout: DEFAULT_CALL_TIMEOUT,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.command);
        command.args(&self.args).env_clear();
        for name in &self.env_allowlist {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.envs(self.env.iter().map(|(name, value)| (name, value)));
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        command
    }

    /// The secret values given to the child, which its stderr must not reveal.
    fn secret_values(&self) -> Vec<String> {
        self.env
            .iter()
            .map(|(_, value)| value.to_string_lossy().into_owned())
            .collect()
    }
}

impl fmt::Debug for StdioServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let env_names: Vec<&str> = self.env.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("StdioServerConfig")
            .field("command", &self.command)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("env_allowlist", &self.env_allowlist)
            .field("env", &env_names)
            .field("startup_timeout", &self.startup_timeout)
            .field("call_timeout", &self.call_timeout)
            .finish()
    }
}

/// The server could not be started or did not complete `initialize`.
#[derive(Debug, Error)]
pub enum ConnectError {
    /// The process could not be spawned, for example because the command does not exist.
    #[error("cannot start server {server}: {source}")]
    Spawn {
        /// Server being started.
        server: ServerId,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The MCP `initialize` handshake failed.
    #[error("server {server} failed to initialize: {reason}")]
    Initialize {
        /// Server being started.
        server: ServerId,
        /// What went wrong.
        reason: String,
        /// The server's last stderr lines, redacted.
        stderr: Vec<String>,
    },
    /// `initialize` did not complete within the startup timeout.
    #[error("server {server} did not initialize within {timeout:?}")]
    Timeout {
        /// Server being started.
        server: ServerId,
        /// The startup timeout.
        timeout: Duration,
        /// The server's last stderr lines, redacted.
        stderr: Vec<String>,
    },
}

impl ConnectError {
    /// The server's last stderr lines, redacted, oldest first. Empty when the process did not
    /// start. Untrusted server output: display as plain text only.
    pub fn stderr(&self) -> &[String] {
        match self {
            Self::Spawn { .. } => &[],
            Self::Initialize { stderr, .. } | Self::Timeout { stderr, .. } => stderr,
        }
    }
}

/// The MCP client side: records `notifications/tools/list_changed` so the session can tell when
/// a server's tool definitions may have changed.
struct Client {
    tool_list_version: Arc<AtomicU64>,
}

impl ClientHandler for Client {
    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        self.tool_list_version.fetch_add(1, Ordering::SeqCst);
    }
}

/// The stderr lines of a server whose start failed. The child is stopping, so the reader gets a
/// moment to collect its last lines.
async fn last_lines(
    reader: Option<tokio::task::JoinHandle<()>>,
    log: &stderr::StderrLog,
) -> Vec<String> {
    if let Some(reader) = reader {
        let _ = tokio::time::timeout(stderr::DRAIN_TIMEOUT, reader).await;
    }
    log.snapshot()
}

/// Bound on [`McpBackend::shutdown`]: how long the server gets to exit after its input closes
/// before it is killed, plus the wait for the kill.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// One connected stdio MCP server.
///
/// [`McpBackend::shutdown`] stops the server and waits until its process has exited. Dropping
/// the backend also stops it, but in the background: a process that is exiting may not wait for
/// that, so call `shutdown` before exit.
pub struct McpBackend {
    server_id: ServerId,
    peer: Peer<RoleClient>,
    /// `None` once shut down.
    service: Mutex<Option<RunningService<RoleClient, Client>>>,
    /// Count of `notifications/tools/list_changed` received.
    tool_list_version: Arc<AtomicU64>,
    stderr: stderr::StderrLog,
    call_timeout: Duration,
}

impl fmt::Debug for McpBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpBackend")
            .field("server_id", &self.server_id)
            .field("connected", &self.is_connected())
            .field("call_timeout", &self.call_timeout)
            .finish_non_exhaustive()
    }
}

impl McpBackend {
    /// Starts the server and completes the MCP `initialize` handshake.
    ///
    /// The child's stderr is captured from the start: see [`McpBackend::recent_stderr`]. When
    /// the start fails, the error carries the last lines.
    pub async fn connect(
        server_id: ServerId,
        config: &StdioServerConfig,
    ) -> Result<Self, ConnectError> {
        let (transport, child_stderr) = TokioChildProcess::builder(config.command())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ConnectError::Spawn {
                server: server_id.clone(),
                source,
            })?;
        let (stderr, reader) = match child_stderr {
            Some(child_stderr) => {
                let (log, task) = stderr::capture(child_stderr, config.secret_values());
                (log, Some(task))
            }
            None => (stderr::StderrLog::default(), None),
        };
        let tool_list_version = Arc::new(AtomicU64::new(0));
        let client = Client {
            tool_list_version: Arc::clone(&tool_list_version),
        };
        let started = tokio::time::timeout(config.startup_timeout, client.serve(transport)).await;
        let service = match started {
            Ok(Ok(service)) => service,
            Ok(Err(error)) => {
                return Err(ConnectError::Initialize {
                    server: server_id,
                    reason: error.to_string(),
                    stderr: last_lines(reader, &stderr).await,
                });
            }
            Err(_elapsed) => {
                return Err(ConnectError::Timeout {
                    server: server_id,
                    timeout: config.startup_timeout,
                    stderr: last_lines(reader, &stderr).await,
                });
            }
        };
        Ok(Self {
            server_id,
            peer: service.peer().clone(),
            service: Mutex::new(Some(service)),
            tool_list_version,
            stderr,
            call_timeout: config.call_timeout,
        })
    }

    /// The server's last stderr lines, redacted, oldest first. Untrusted server output: display
    /// as plain text only.
    pub fn recent_stderr(&self) -> Vec<String> {
        self.stderr.snapshot()
    }

    fn service(&self) -> MutexGuard<'_, Option<RunningService<RoleClient, Client>>> {
        self.service.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// True while the session is running and the transport is open.
    pub fn is_connected(&self) -> bool {
        self.service()
            .as_ref()
            .is_some_and(|service| !service.is_closed())
            && !self.peer.is_transport_closed()
    }

    /// Stops the session without waiting. The child process is killed in the background and
    /// later requests fail with [`BackendError::NotConnected`].
    pub fn disconnect(&self) {
        if let Some(service) = self.service().as_ref() {
            service.cancellation_token().cancel();
        }
    }

    /// Stops the session and waits, up to [`SHUTDOWN_TIMEOUT`], until the server process has
    /// exited: its input is closed, and it is killed if it does not exit in time. Later requests
    /// fail with [`BackendError::NotConnected`].
    pub async fn shutdown(&self) {
        let service = self.service().take();
        if let Some(mut service) = service {
            let _ = service.close_with_timeout(SHUTDOWN_TIMEOUT).await;
        }
    }

    fn ensure_connected(&self) -> Result<(), BackendError> {
        if self.is_connected() {
            Ok(())
        } else {
            Err(BackendError::NotConnected)
        }
    }

    fn timed_out(&self, what: &str) -> BackendError {
        BackendError::OutcomeUnknown(format!("no {what} response within {:?}", self.call_timeout))
    }
}

#[async_trait]
impl ToolBackend for McpBackend {
    fn server_id(&self) -> &ServerId {
        &self.server_id
    }

    fn recent_stderr(&self) -> Vec<String> {
        McpBackend::recent_stderr(self)
    }

    fn is_connected(&self) -> bool {
        McpBackend::is_connected(self)
    }

    async fn shutdown(&self) {
        McpBackend::shutdown(self).await;
    }

    fn tool_list_version(&self) -> u64 {
        self.tool_list_version.load(Ordering::SeqCst)
    }

    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, BackendError> {
        self.ensure_connected()?;
        let tools = tokio::time::timeout(self.call_timeout, self.peer.list_all_tools())
            .await
            .map_err(|_| self.timed_out("tools/list"))?
            .map_err(map_service_error)?;
        Ok(tools
            .into_iter()
            .map(|tool| descriptor(&self.server_id, tool))
            .collect())
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Map<String, Value>,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, BackendError> {
        if cancel.is_cancelled() {
            return Err(BackendError::CancelledBeforeSend);
        }
        self.ensure_connected()?;
        let params = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
        // From here the request may have been sent, so every failure has an unknown outcome.
        // Dropping the request future on timeout or cancellation sends `notifications/cancelled`.
        let request = tokio::time::timeout(self.call_timeout, self.peer.call_tool_once(params));
        let response = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                return Err(BackendError::OutcomeUnknown(
                    "cancelled after the request was sent".into(),
                ));
            }
            response = request => response.map_err(|_| self.timed_out("tools/call"))?,
        };
        match response.map_err(map_service_error)? {
            CallToolResponse::Complete(result) => Ok(output(result)),
            _ => Err(BackendError::OutcomeUnknown(
                "server returned an input request or task, which IGOR does not support".into(),
            )),
        }
    }
}

/// Maps an `rmcp` request failure. Only a JSON-RPC error response proves the server rejected the
/// request; anything else after sending leaves the outcome unknown.
fn map_service_error(error: ServiceError) -> BackendError {
    match error {
        ServiceError::McpError(error) => {
            BackendError::Protocol(format!("{} (code {})", error.message, error.code.0))
        }
        other => BackendError::OutcomeUnknown(other.to_string()),
    }
}

fn descriptor(server: &ServerId, tool: Tool) -> ToolDescriptor {
    let hints = tool
        .annotations
        .as_ref()
        .map(|a| ToolHints {
            read_only: a.read_only_hint,
            destructive: a.destructive_hint,
        })
        .unwrap_or_default();
    ToolDescriptor {
        id: ToolId {
            server: server.clone(),
            name: tool.name.into_owned(),
        },
        description: tool.description.map(|d| d.into_owned()),
        input_schema: Value::Object(Map::clone(&tool.input_schema)),
        hints,
    }
}

fn output(result: CallToolResult) -> ToolOutput {
    ToolOutput {
        content: result.content.into_iter().map(content).collect(),
        structured: result.structured_content,
        is_error: result.is_error.unwrap_or(false),
    }
}

fn content(block: ContentBlock) -> ToolContent {
    let description = match block {
        ContentBlock::Text(text) => return ToolContent::Text { text: text.text },
        ContentBlock::Image(image) => format!("image ({})", image.mime_type),
        ContentBlock::Audio(audio) => format!("audio ({})", audio.mime_type),
        ContentBlock::Resource(_) => "embedded resource".to_owned(),
        ContentBlock::ResourceLink(_) => "resource link".to_owned(),
        _ => "unsupported content".to_owned(),
    };
    ToolContent::Unsupported { description }
}

#[cfg(test)]
mod tests {
    use rmcp::model::ToolAnnotations;
    use serde_json::json;

    use super::*;

    #[test]
    fn maps_tool_definition_and_hints() -> Result<(), Box<dyn std::error::Error>> {
        let server = ServerId::new("sales")?;
        let tool: Tool = serde_json::from_value(json!({
            "name": "create_draft_order",
            "description": "Saves a draft.",
            "inputSchema": { "type": "object", "properties": { "x": { "type": "string" } } },
            "annotations": { "readOnlyHint": false, "destructiveHint": false }
        }))?;
        let d = descriptor(&server, tool);
        assert_eq!(d.id.to_string(), "sales/create_draft_order");
        assert_eq!(d.description.as_deref(), Some("Saves a draft."));
        assert_eq!(d.input_schema["properties"]["x"]["type"], "string");
        assert_eq!(
            d.hints,
            ToolHints {
                read_only: Some(false),
                destructive: Some(false)
            }
        );
        Ok(())
    }

    #[test]
    fn missing_annotations_give_no_hints() -> Result<(), Box<dyn std::error::Error>> {
        let mut tool: Tool = serde_json::from_value(json!({
            "name": "t", "inputSchema": { "type": "object" }
        }))?;
        assert_eq!(
            descriptor(&ServerId::new("s")?, tool.clone()).hints,
            ToolHints::default()
        );
        tool.annotations = Some(ToolAnnotations::new());
        assert_eq!(
            descriptor(&ServerId::new("s")?, tool).hints,
            ToolHints::default()
        );
        Ok(())
    }

    #[test]
    fn maps_result_content() -> Result<(), serde_json::Error> {
        let result: CallToolResult = serde_json::from_value(json!({
            "content": [
                { "type": "text", "text": "hello" },
                { "type": "image", "data": "AAAA", "mimeType": "image/png" }
            ],
            "structuredContent": { "n": 1 },
            "isError": true
        }))?;
        let out = output(result);
        assert_eq!(
            out.content,
            [
                ToolContent::Text {
                    text: "hello".into()
                },
                ToolContent::Unsupported {
                    description: "image (image/png)".into()
                }
            ]
        );
        assert_eq!(out.structured, Some(json!({ "n": 1 })));
        assert!(out.is_error);
        Ok(())
    }

    #[test]
    fn absent_is_error_means_success() -> Result<(), serde_json::Error> {
        let result: CallToolResult = serde_json::from_value(json!({ "content": [] }))?;
        assert!(!output(result).is_error);
        Ok(())
    }

    #[test]
    fn only_json_rpc_errors_are_protocol_errors() {
        let protocol = map_service_error(ServiceError::McpError(rmcp::ErrorData::invalid_params(
            "unknown tool: x",
            None,
        )));
        assert_eq!(
            protocol,
            BackendError::Protocol("unknown tool: x (code -32602)".into())
        );
        assert!(matches!(
            map_service_error(ServiceError::TransportClosed),
            BackendError::OutcomeUnknown(_)
        ));
        assert!(matches!(
            map_service_error(ServiceError::UnexpectedResponse),
            BackendError::OutcomeUnknown(_)
        ));
    }

    #[test]
    fn debug_hides_env_values() {
        let mut config = StdioServerConfig::new("server");
        config.env.push(("TOKEN".into(), "secret-value-123".into()));
        let debug = format!("{config:?}");
        assert!(debug.contains("TOKEN"));
        assert!(!debug.contains("secret-value-123"));
    }
}
