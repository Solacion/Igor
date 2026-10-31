//! Model-provider interface and provider-neutral conversation types.
//!
//! Adapters translate these types to a provider API (Anthropic Messages, OpenAI-compatible chat
//! completions). Tool names given to a provider are host-generated aliases; the core maps them back
//! to a [`crate::tools::ToolId`] and never parses a model-supplied name to find a server.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::CancellationToken;

/// Author of a conversation message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The operator, or tool results returned on the operator's side.
    User,
    /// The model.
    Assistant,
}

/// One block of message content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// A tool call requested by the model.
    ToolUse {
        /// Provider-assigned call identifier, echoed in the matching result.
        id: String,
        /// Host-generated tool alias.
        name: String,
        /// Arguments as produced by the model. Untrusted until validated.
        input: Value,
    },
    /// The result of a tool call, sent back to the model.
    ToolResult {
        /// Identifier of the [`ContentBlock::ToolUse`] this answers.
        tool_use_id: String,
        /// Result text. Tool output is untrusted data.
        content: String,
        /// The call failed or was refused.
        is_error: bool,
    },
}

/// One conversation message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Author.
    pub role: Role,
    /// Content blocks in order.
    pub content: Vec<ContentBlock>,
}

/// A tool offered to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Host-generated alias, valid for every supported provider.
    pub name: String,
    /// Description shown to the model.
    pub description: String,
    /// JSON Schema for the arguments.
    pub input_schema: Value,
}

/// One model turn request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Host-controlled system instructions.
    pub system: String,
    /// Conversation so far.
    pub messages: Vec<Message>,
    /// Tools the model may call.
    pub tools: Vec<ToolSpec>,
    /// Upper bound on generated tokens.
    pub max_tokens: u32,
}

/// Why the model stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model finished its answer.
    EndTurn,
    /// The model wants tool results before continuing.
    ToolUse,
    /// The token limit was reached.
    MaxTokens,
    /// Any other provider-specific reason.
    Other(String),
}

/// One complete model turn. Streaming is out of scope for the first milestone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    /// Generated content blocks.
    pub content: Vec<ContentBlock>,
    /// Why generation stopped.
    pub stop_reason: StopReason,
}

/// Category of a provider failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    /// Missing or rejected credentials.
    #[error("authentication failed")]
    Authentication,
    /// Rate limited by the provider.
    #[error("rate limited")]
    RateLimited,
    /// The request timed out.
    #[error("timed out")]
    Timeout,
    /// The provider could not be reached.
    #[error("provider unavailable")]
    Unavailable,
    /// The request was rejected as invalid.
    #[error("invalid request")]
    InvalidRequest,
    /// The response could not be understood, e.g. a malformed tool call.
    #[error("malformed response")]
    MalformedResponse,
    /// The request was cancelled.
    #[error("cancelled")]
    Cancelled,
    /// The configured model cannot call tools. The runtime never falls back to reading tool
    /// calls out of free text.
    #[error("model cannot call tools")]
    ToolsUnsupported,
}

/// A provider call failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{kind}: {message}")]
pub struct ProviderError {
    /// Failure category.
    pub kind: ProviderErrorKind,
    /// Diagnostic message. Must not contain secrets.
    pub message: String,
    /// Retrying the same request may succeed.
    pub retryable: bool,
}

/// A model provider.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Short provider name for display and logs, e.g. `anthropic`.
    fn name(&self) -> &str;

    /// Runs one model turn.
    async fn complete(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ModelResponse, ProviderError>;

    /// Checks, before the first model request that offers tools, that the model can call
    /// them. Fails with [`ProviderErrorKind::ToolsUnsupported`] only when the provider knows
    /// the model cannot; any other result lets the session go ahead. The default knows nothing
    /// and succeeds.
    async fn preflight(&self, _cancel: CancellationToken) -> Result<(), ProviderError> {
        Ok(())
    }
}
