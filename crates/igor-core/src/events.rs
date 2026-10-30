//! Runtime events shared by every client.
//!
//! Events are transport-neutral and serialisable so the CLI, the desktop and a future HTTP API
//! observe one stream. Variants are added as features land; renaming or removing one is a breaking
//! change for clients.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::{ServerId, ToolId};

/// Version of the event schema, bumped on breaking changes.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// Something that happened in a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RuntimeEvent {
    /// A server's connection state changed.
    ConnectionChanged {
        /// Server concerned.
        server: ServerId,
        /// New state.
        state: ConnectionState,
    },
    /// The model proposed a tool call.
    ToolCallProposed {
        /// Session-unique call identifier.
        call_id: String,
        /// Tool to call.
        tool: ToolId,
        /// Proposed arguments.
        arguments: Value,
    },
    /// A tool call finished with a response from the server.
    ToolCallCompleted {
        /// Call identifier.
        call_id: String,
        /// The server reported an error result.
        is_error: bool,
    },
    /// A tool call did not produce a response.
    ToolCallFailed {
        /// Call identifier.
        call_id: String,
        /// What is known about whether the tool ran.
        outcome: CallFailure,
        /// Diagnostic message.
        message: String,
    },
    /// The model produced text for the operator.
    AssistantText {
        /// The text. Untrusted model output: render as plain text.
        text: String,
    },
}

/// Connection state of a tool server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// Starting or initialising.
    Connecting,
    /// Ready for calls.
    Connected,
    /// Stopped or crashed.
    Disconnected,
}

/// What is known about a tool call that did not return a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallFailure {
    /// The tool definitely did not run: refused by policy, denied, invalid or never sent.
    NotExecuted,
    /// The request may have reached the server; the effect is unknown. Never retried
    /// automatically.
    Unknown,
}
