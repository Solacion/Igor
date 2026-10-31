//! Runtime events shared by every client.
//!
//! Events are transport-neutral and serialisable so the CLI, the desktop and a future HTTP API
//! observe one stream. Every event is delivered in an [`EventEnvelope`] carrying the schema version
//! and a per-session sequence number. Variants are added as features land, so clients must ignore
//! unknown variants; renaming or removing one is a breaking change and bumps the version.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::approval::ApprovalRequest;
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
    /// A server reported that its tool list changed. Calls to it are refused until the host
    /// refreshes its tools with [`crate::session::Session::replace_tools`].
    ToolsChanged {
        /// Server concerned.
        server: ServerId,
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
    /// A tool call was refused before dispatch: unknown tool, not permitted or invalid
    /// arguments. Nothing was sent.
    ToolCallRejected {
        /// Call identifier.
        call_id: String,
        /// Tool name the model used, cut to 64 characters. Untrusted model text.
        alias: String,
        /// The tool, when the name resolved to one.
        tool: Option<ToolId>,
        /// Why the call was refused.
        reason: String,
    },
    /// A permitted tool call is being sent to its server.
    ToolCallStarted {
        /// Call identifier.
        call_id: String,
    },
    /// A tool call finished with a response from the server.
    ToolCallCompleted {
        /// Call identifier.
        call_id: String,
        /// The server reported an error result.
        is_error: bool,
        /// The result was cut at the size limit before it reached the model.
        #[serde(default)]
        truncated: bool,
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
    /// A write call is waiting for the operator. Answer with
    /// [`crate::session::SessionHandle::resolve_approval`].
    ApprovalRequested {
        /// What the operator is asked to approve, including the hash to echo.
        request: ApprovalRequest,
    },
    /// A pending approval ended.
    ApprovalResolved {
        /// The approval.
        approval_id: String,
        /// Call identifier.
        call_id: String,
        /// How it ended.
        outcome: ApprovalOutcome,
    },
    /// The model asked the operator a question. Answer with
    /// [`crate::session::SessionHandle::answer_clarification`].
    ClarificationRequested {
        /// Identifier to answer.
        request_id: String,
        /// The question. Untrusted model output: render as plain text.
        question: String,
    },
    /// The operator answered a clarification request.
    ClarificationAnswered {
        /// The request answered.
        request_id: String,
    },
    /// An operator input was refused, e.g. an unknown or already used approval, or a wrong
    /// action hash.
    InputRejected {
        /// The approval or clarification id given.
        id: String,
        /// Why it was refused.
        reason: String,
    },
    /// The turn started by a user message ended.
    TurnFinished {
        /// How it ended.
        outcome: TurnOutcome,
    },
}

/// How a pending approval ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    /// The operator approved; the call is dispatched.
    Approved,
    /// The operator denied; nothing runs.
    Denied,
    /// The turn was cancelled; nothing runs.
    Cancelled,
    /// The tool definition or classification changed; nothing runs.
    Invalidated,
}

/// How a turn ended. Clients must show anything other than `Completed` as not a full answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TurnOutcome {
    /// The model finished its answer, and every tool call in the turn succeeded.
    Completed,
    /// The model finished its answer, but some tool calls did not succeed: they failed, were
    /// refused, or returned an error result such as missing data. The answer may rest on
    /// incomplete data. Calls the operator denied are not counted.
    CompletedWithGaps {
        /// Tool calls in the turn that did not return a successful result.
        unsuccessful_calls: u32,
    },
    /// A limit stopped the turn before the model finished.
    LimitReached {
        /// The limit hit.
        limit: LimitKind,
    },
    /// The operator or client cancelled the turn.
    Cancelled,
    /// The model request failed.
    Failed {
        /// Diagnostic message without secrets.
        message: String,
    },
    /// The configured model cannot call tools, so the turn was not sent. Choose a model with
    /// tool calling.
    ToolsUnsupported {
        /// Diagnostic message without secrets, naming the model.
        message: String,
    },
}

/// A limit that can end a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    /// Model requests per user message.
    ModelRequests,
    /// Tool calls per user message.
    ToolCalls,
    /// The model's output token limit.
    OutputTokens,
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
