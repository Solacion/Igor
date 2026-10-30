//! Runtime events shared by every client.
//!
//! Events are transport-neutral and serialisable so the CLI, the desktop and a future HTTP API
//! observe one stream. Every event is delivered in an [`EventEnvelope`] carrying the schema version
//! and a per-session sequence number. Variants are added as features land, so clients must ignore
//! unknown variants; renaming or removing one is a breaking change and bumps the version.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::{ServerId, ToolId};

/// Version of the event schema, bumped on breaking changes.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

/// A runtime event as delivered to clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Schema version of `event`; [`EVENT_SCHEMA_VERSION`] when produced by this crate.
    pub schema_version: u32,
    /// Per-session sequence number, starting at 0 and increasing by one per event.
    pub sequence: u64,
    /// The event.
    pub event: RuntimeEvent,
}

impl EventEnvelope {
    /// Wraps an event with the current schema version.
    pub fn new(sequence: u64, event: RuntimeEvent) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            sequence,
            event,
        }
    }
}

/// Something that happened in a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuntimeEvent {
    /// A server's connection state changed.
    ConnectionChanged {
        /// Server concerned.
        server: ServerId,
        /// New state.
        state: ConnectionState,
        /// Reason for a failure or disconnect, when known. Diagnostic text without secrets.
        detail: Option<String>,
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
    /// Stopped deliberately.
    Disconnected,
    /// Failed to start, crashed or stopped responding.
    Failed,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_carries_schema_version_and_sequence() -> Result<(), serde_json::Error> {
        let envelope = EventEnvelope::new(3, RuntimeEvent::AssistantText { text: "hi".into() });
        let json = serde_json::to_value(&envelope)?;
        assert_eq!(json["schema_version"], EVENT_SCHEMA_VERSION);
        assert_eq!(json["sequence"], 3);
        assert_eq!(json["event"]["event"], "assistant_text");
        Ok(())
    }
}
