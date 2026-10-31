//! Tool backends and server-qualified tool identity.
//!
//! A [`ToolBackend`] represents one connected tool server, normally an MCP server. The core never
//! sees MCP library types: an MCP client crate adapts its protocol to this trait.

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::CancellationToken;

/// Maximum length of a [`ServerId`].
pub const MAX_SERVER_ID_LEN: usize = 32;

/// Host-assigned identifier of a configured tool server.
///
/// Lowercase ASCII letters, digits, `-` and `_`, starting with a letter, at most
/// [`MAX_SERVER_ID_LEN`] characters. Assigned by host configuration, never by a server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServerId(String);

impl ServerId {
    /// Validates and creates a server identifier.
    pub fn new(id: impl Into<String>) -> Result<Self, InvalidServerId> {
        let id = id.into();
        let valid_start = id.chars().next().is_some_and(|c| c.is_ascii_lowercase());
        let valid_chars = id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if valid_start && valid_chars && id.len() <= MAX_SERVER_ID_LEN {
            Ok(Self(id))
        } else {
            Err(InvalidServerId(id))
        }
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ServerId {
    type Error = InvalidServerId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ServerId> for String {
    fn from(value: ServerId) -> Self {
        value.0
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A server identifier failed validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error(
    "invalid server id {0:?}: use lowercase letters, digits, '-' or '_', starting with a letter, at most {max} characters",
    max = MAX_SERVER_ID_LEN
)]
pub struct InvalidServerId(pub String);

/// Server-qualified tool identity. Two servers may expose tools with the same name; the pair
/// `(server, name)` is always unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ToolId {
    /// Server that exposes the tool.
    pub server: ServerId,
    /// Tool name as reported by the server.
    pub name: String,
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.server, self.name)
    }
}

/// Behaviour hints reported by a server. Untrusted: they may only tighten a host classification,
/// never relax it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolHints {
    /// Server claims the tool does not modify its environment.
    pub read_only: Option<bool>,
    /// Server claims the tool may perform destructive updates.
    pub destructive: Option<bool>,
}

/// A tool discovered on a server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    /// Server-qualified identity.
    pub id: ToolId,
    /// Human-readable description from the server. Untrusted text.
    pub description: Option<String>,
    /// JSON Schema for the tool's arguments.
    pub input_schema: Value,
    /// Untrusted behaviour hints.
    pub hints: ToolHints,
}

/// One item of tool output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    /// Plain text. Untrusted.
    Text {
        /// The text.
        text: String,
    },
    /// Content the core does not interpret (images, resources), summarised for display.
    Unsupported {
        /// Short description of what was omitted, e.g. its MIME type.
        description: String,
    },
}

/// Result of a tool call that reached the server and returned a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Unstructured content blocks.
    pub content: Vec<ToolContent>,
    /// Structured result, when the server provides one.
    pub structured: Option<Value>,
    /// The server reported the call as failed (MCP `isError`).
    pub is_error: bool,
}

/// Failure to obtain a tool response.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackendError {
    /// The server is not connected; the request was not sent.
    #[error("server not connected")]
    NotConnected,
    /// The request was cancelled before it was sent.
    #[error("cancelled before sending")]
    CancelledBeforeSend,
    /// The request may have reached the server but no valid response arrived (timeout,
    /// disconnect, cancellation after sending or malformed response). The outcome is unknown.
    #[error("outcome unknown: {0}")]
    OutcomeUnknown(String),
    /// The server rejected the request at protocol level, e.g. unknown tool.
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl BackendError {
    /// True when the tool definitely did not run, so a retry cannot duplicate an effect.
    pub fn definitely_not_executed(&self) -> bool {
        matches!(self, Self::NotConnected | Self::CancelledBeforeSend)
    }
}

/// One connected tool server.
#[async_trait]
pub trait ToolBackend: Send + Sync {
    /// Host-assigned identifier of this server.
    fn server_id(&self) -> &ServerId;

    /// True while the server can take calls. A backend that cannot tell returns `true`; the
    /// session uses this to report a server that stopped.
    fn is_connected(&self) -> bool {
        true
    }

    /// A number that changes whenever the server reports that its tool list changed. The
    /// session refuses calls to a server whose number differs from when its tools were
    /// registered, until the host refreshes them. The default never changes.
    fn tool_list_version(&self) -> u64 {
        0
    }

    /// Stops the server and waits until it has stopped. Called once when the host shuts down;
    /// later calls fail with [`BackendError::NotConnected`]. The default does nothing.
    async fn shutdown(&self) {}

    /// The server's most recent diagnostic output, such as a stdio server's stderr, oldest
    /// first, with secret values given to the server already removed. Untrusted server output:
    /// display as plain text only. The default has none.
    fn recent_stderr(&self) -> Vec<String> {
        Vec::new()
    }

    /// Lists the tools the server currently exposes.
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, BackendError>;

    /// Calls a tool by its server-local name. Arguments are already validated by the core.
    async fn call_tool(
        &self,
        name: &str,
        arguments: Map<String, Value>,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, BackendError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_id_accepts_valid_ids() {
        for id in ["sales", "weather-2", "pos_live", "a"] {
            assert!(ServerId::new(id).is_ok(), "{id} should be valid");
        }
    }

    #[test]
    fn server_id_rejects_invalid_ids() {
        let too_long = "a".repeat(MAX_SERVER_ID_LEN + 1);
        for id in [
            "",
            "Sales",
            "2nd",
            "sales.tool",
            "sales/tool",
            "sales tool",
            too_long.as_str(),
        ] {
            assert!(ServerId::new(id).is_err(), "{id:?} should be invalid");
        }
    }

    #[test]
    fn server_id_is_validated_when_deserialised() {
        assert!(serde_json::from_str::<ServerId>(r#""sales""#).is_ok());
        assert!(serde_json::from_str::<ServerId>(r#""Sales/Tool""#).is_err());
    }

    #[test]
    fn same_tool_name_on_two_servers_is_distinct() -> Result<(), InvalidServerId> {
        let a = ToolId {
            server: ServerId::new("sales")?,
            name: "lookup".into(),
        };
        let b = ToolId {
            server: ServerId::new("weather")?,
            name: "lookup".into(),
        };
        assert_ne!(a, b);
        assert_eq!(a.to_string(), "sales/lookup");
        Ok(())
    }

    #[test]
    fn only_unsent_failures_are_definitely_not_executed() {
        assert!(BackendError::NotConnected.definitely_not_executed());
        assert!(BackendError::CancelledBeforeSend.definitely_not_executed());
        assert!(!BackendError::OutcomeUnknown("timeout".into()).definitely_not_executed());
        assert!(!BackendError::Protocol("unknown tool".into()).definitely_not_executed());
    }
}
