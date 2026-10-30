//! Deterministic test support: a scripted model provider, a recording tool backend and a manual
//! clock.
//!
//! Available to other crates through the non-default `testing` feature, so the runtime does not
//! ship it:
//!
//! ```toml
//! [dev-dependencies]
//! igor-core = { path = "../igor-core", features = ["testing"] }
//! ```

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use async_trait::async_trait;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::CancellationToken;
use crate::clock::Clock;
use crate::provider::{
    ContentBlock, ModelRequest, ModelResponse, Provider, ProviderError, ProviderErrorKind,
    StopReason,
};
use crate::tools::{BackendError, ServerId, ToolBackend, ToolContent, ToolDescriptor, ToolOutput};

/// Check run against the request a scripted turn receives. Returns a message on failure.
type RequestCheck = Box<dyn Fn(&ModelRequest) -> Result<(), String> + Send + Sync>;

/// What a scripted turn returns.
#[derive(Debug)]
enum Reply {
    Respond(ModelResponse),
    Fail(ProviderError),
    UntilCancelled,
}

/// One expected model request and the reply to it.
pub struct ScriptedTurn {
    check: Option<RequestCheck>,
    reply: Reply,
}

impl ScriptedTurn {
    /// Replies with `response`.
    pub fn respond(response: ModelResponse) -> Self {
        Self {
            check: None,
            reply: Reply::Respond(response),
        }
    }

    /// Replies with final text, stopping with [`StopReason::EndTurn`].
    pub fn text(text: impl Into<String>) -> Self {
        Self::respond(ModelResponse {
            content: vec![ContentBlock::Text { text: text.into() }],
            stop_reason: StopReason::EndTurn,
        })
    }

    /// Replies with one tool call, stopping with [`StopReason::ToolUse`]. `name` and `input` are
    /// passed through unchecked, so unknown aliases and malformed arguments can be scripted. Use
    /// [`Self::respond`] for several calls in one turn.
    pub fn tool_call(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        Self::respond(ModelResponse {
            content: vec![ContentBlock::ToolUse {
                id: id.into(),
                name: name.into(),
                input,
            }],
            stop_reason: StopReason::ToolUse,
        })
    }

    /// Fails with `error`.
    pub fn fail(error: ProviderError) -> Self {
        Self {
            check: None,
            reply: Reply::Fail(error),
        }
    }

    /// Waits until the request is cancelled, then fails with [`ProviderErrorKind::Cancelled`].
    pub fn until_cancelled() -> Self {
        Self {
            check: None,
            reply: Reply::UntilCancelled,
        }
    }

    /// Adds a check on the request this turn receives. When it returns an error, the request
    /// fails with [`ProviderErrorKind::InvalidRequest`] and [`ScriptedProvider::verify`] reports
    /// the message.
    pub fn expecting(
        mut self,
        check: impl Fn(&ModelRequest) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.check = Some(Box::new(check));
        self
    }
}

impl fmt::Debug for ScriptedTurn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptedTurn")
            .field("checked", &self.check.is_some())
            .field("reply", &self.reply)
            .finish()
    }
}

/// The scripted provider was not used as scripted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("scripted provider: {}", .problems.join("; "))]
pub struct ScriptError {
    /// Every problem found, in order.
    pub problems: Vec<String>,
}

#[derive(Debug, Default)]
struct State {
    turns: VecDeque<ScriptedTurn>,
    requests: Vec<ModelRequest>,
    problems: Vec<String>,
}

/// A [`Provider`] that replays scripted turns in order and records every request.
///
/// A request that arrives after the script is exhausted, or fails a turn's check, is answered
/// with an [`ProviderErrorKind::InvalidRequest`] error and recorded as a problem. Call
/// [`Self::verify`] at the end of a test to fail on problems or unconsumed turns.
#[derive(Debug, Default)]
pub struct ScriptedProvider {
    state: Mutex<State>,
}

impl ScriptedProvider {
    /// Creates a provider that replays `turns` in order.
    pub fn new(turns: impl IntoIterator<Item = ScriptedTurn>) -> Self {
        Self {
            state: Mutex::new(State {
                turns: turns.into_iter().collect(),
                ..State::default()
            }),
        }
    }

    /// Every request received so far, in order, including rejected ones.
    pub fn requests(&self) -> Vec<ModelRequest> {
        self.lock().requests.clone()
    }

    /// Fails when a request was unexpected or failed its check, or when turns were not consumed.
    pub fn verify(&self) -> Result<(), ScriptError> {
        let state = self.lock();
        let mut problems = state.problems.clone();
        if !state.turns.is_empty() {
            problems.push(format!(
                "{} scripted turn(s) not consumed after {} request(s)",
                state.turns.len(),
                state.requests.len()
            ));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(ScriptError { problems })
        }
    }

    /// Locks the state. A panic in a request check poisons the lock; the state stays usable.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records `request` and takes the reply for it, without awaiting.
    fn take_reply(
        &self,
        request: ModelRequest,
        cancel: &CancellationToken,
    ) -> Result<Reply, ProviderError> {
        let mut state = self.lock();
        let number = state.requests.len() + 1;
        if cancel.is_cancelled() {
            state.requests.push(request);
            return Err(cancelled());
        }
        let Some(turn) = state.turns.pop_front() else {
            let problem = format!("unexpected request {number}: the script has no more turns");
            state.requests.push(request);
            return Err(script_failure(&mut state, problem));
        };
        let checked = turn.check.as_ref().map_or(Ok(()), |check| check(&request));
        state.requests.push(request);
        match checked {
            Ok(()) => Ok(turn.reply),
            Err(message) => {
                let problem = format!("request {number} failed its check: {message}");
                Err(script_failure(&mut state, problem))
            }
        }
    }
}

#[async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &str {
        "scripted"
    }

    async fn complete(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ModelResponse, ProviderError> {
        match self.take_reply(request, &cancel)? {
            Reply::Respond(response) => Ok(response),
            Reply::Fail(error) => Err(error),
            Reply::UntilCancelled => {
                cancel.cancelled().await;
                Err(cancelled())
            }
        }
    }
}

fn cancelled() -> ProviderError {
    ProviderError {
        kind: ProviderErrorKind::Cancelled,
        message: "request cancelled".to_owned(),
        retryable: false,
    }
}

/// Records `problem` and returns the error answered to the caller.
fn script_failure(state: &mut State, problem: String) -> ProviderError {
    let error = ProviderError {
        kind: ProviderErrorKind::InvalidRequest,
        message: format!("scripted provider: {problem}"),
        retryable: false,
    };
    state.problems.push(problem);
    error
}

/// A [`Clock`] that returns a set time until changed.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
}

impl ManualClock {
    /// Creates a clock reading `start`.
    pub fn new(start: SystemTime) -> Self {
        Self {
            now: Mutex::new(start),
        }
    }

    /// Sets the time returned from now on.
    pub fn set(&self, now: SystemTime) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = now;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// How a [`RecordingBackend`] answers calls to one tool.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeReply {
    /// Return this output.
    Output(ToolOutput),
    /// Fail with this error.
    Error(BackendError),
    /// Wait until cancelled, then fail with [`BackendError::OutcomeUnknown`], like a request
    /// that reached a server which never answered.
    Hang,
}

/// One call received by a [`RecordingBackend`].
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedCall {
    /// Server-local tool name.
    pub name: String,
    /// Arguments exactly as received.
    pub arguments: Map<String, Value>,
}

/// An in-memory [`ToolBackend`] that records every call it receives, standing in for a server's
/// call log in core tests.
///
/// Unless configured with [`Self::with_reply`], a call returns the text `ok: <tool name>`.
#[derive(Debug)]
pub struct RecordingBackend {
    server: ServerId,
    tools: Vec<ToolDescriptor>,
    replies: HashMap<String, FakeReply>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl RecordingBackend {
    /// Creates a backend for `server` that lists no tools.
    pub fn new(server: ServerId) -> Self {
        Self {
            server,
            tools: Vec::new(),
            replies: HashMap::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Sets the tools returned by [`ToolBackend::list_tools`].
    pub fn with_tools(mut self, tools: Vec<ToolDescriptor>) -> Self {
        self.tools = tools;
        self
    }

    /// Sets how calls to `name` are answered.
    pub fn with_reply(mut self, name: impl Into<String>, reply: FakeReply) -> Self {
        self.replies.insert(name.into(), reply);
        self
    }

    /// Every call received so far, in order.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl ToolBackend for RecordingBackend {
    fn server_id(&self) -> &ServerId {
        &self.server
    }

    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, BackendError> {
        Ok(self.tools.clone())
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
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(RecordedCall {
                name: name.to_owned(),
                arguments,
            });
        match self.replies.get(name) {
            Some(FakeReply::Output(output)) => Ok(output.clone()),
            Some(FakeReply::Error(error)) => Err(error.clone()),
            Some(FakeReply::Hang) => {
                cancel.cancelled().await;
                Err(BackendError::OutcomeUnknown(
                    "cancelled after sending".to_owned(),
                ))
            }
            None => Ok(ToolOutput {
                content: vec![ToolContent::Text {
                    text: format!("ok: {name}"),
                }],
                structured: None,
                is_error: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use serde_json::json;

    use super::*;
    use crate::provider::{Message, Role};
    use crate::tools::InvalidServerId;

    fn request(text: &str) -> ModelRequest {
        ModelRequest {
            system: "system".to_owned(),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: text.to_owned(),
                }],
            }],
            tools: Vec::new(),
            max_tokens: 100,
        }
    }

    #[tokio::test]
    async fn replays_turns_in_order_and_records_requests() -> Result<(), Box<dyn std::error::Error>>
    {
        let provider = ScriptedProvider::new([
            ScriptedTurn::tool_call("call-1", "sales__lookup", json!({ "sku": "A1" })),
            ScriptedTurn::text("done"),
        ]);
        let first = provider
            .complete(request("one"), CancellationToken::new())
            .await?;
        assert_eq!(first.stop_reason, StopReason::ToolUse);
        let second = provider
            .complete(request("two"), CancellationToken::new())
            .await?;
        assert_eq!(
            second.content,
            [ContentBlock::Text {
                text: "done".to_owned()
            }]
        );
        assert_eq!(provider.requests(), [request("one"), request("two")]);
        provider.verify()?;
        Ok(())
    }

    #[tokio::test]
    async fn extra_request_fails_and_is_reported() {
        let provider = ScriptedProvider::new([]);
        let result = provider
            .complete(request("one"), CancellationToken::new())
            .await;
        assert!(matches!(
            result,
            Err(ProviderError {
                kind: ProviderErrorKind::InvalidRequest,
                ..
            })
        ));
        let error = provider
            .verify()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("unexpected request 1"), "{error}");
    }

    #[test]
    fn unconsumed_turns_are_reported() {
        let provider = ScriptedProvider::new([ScriptedTurn::text("never sent")]);
        let error = provider
            .verify()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("1 scripted turn(s) not consumed"), "{error}");
    }

    #[tokio::test]
    async fn failed_check_is_reported() {
        let provider = ScriptedProvider::new([ScriptedTurn::text("hi").expecting(|request| {
            if request.tools.is_empty() {
                Err("expected tools to be offered".to_owned())
            } else {
                Ok(())
            }
        })]);
        let result = provider
            .complete(request("one"), CancellationToken::new())
            .await;
        assert!(result.is_err());
        let error = provider
            .verify()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        assert!(error.contains("expected tools to be offered"), "{error}");
    }

    #[tokio::test]
    async fn scripted_errors_are_returned() {
        let error = ProviderError {
            kind: ProviderErrorKind::RateLimited,
            message: "slow down".to_owned(),
            retryable: true,
        };
        let provider = ScriptedProvider::new([ScriptedTurn::fail(error.clone())]);
        let result = provider
            .complete(request("one"), CancellationToken::new())
            .await;
        assert_eq!(result, Err(error));
        assert_eq!(provider.verify(), Ok(()));
    }

    #[tokio::test]
    async fn until_cancelled_waits_for_cancellation() {
        let provider = ScriptedProvider::new([ScriptedTurn::until_cancelled()]);
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(provider.complete(request("one"), cancel.clone()), async {
            cancel.cancel();
        });
        assert!(matches!(
            result,
            Err(ProviderError {
                kind: ProviderErrorKind::Cancelled,
                ..
            })
        ));
        assert_eq!(provider.verify(), Ok(()));
    }

    #[tokio::test]
    async fn cancelled_request_does_not_consume_a_turn() {
        let provider = ScriptedProvider::new([ScriptedTurn::text("later")]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = provider.complete(request("one"), cancel).await;
        assert!(matches!(
            result,
            Err(ProviderError {
                kind: ProviderErrorKind::Cancelled,
                ..
            })
        ));
        assert_eq!(provider.requests().len(), 1);
        assert!(provider.verify().is_err());
    }

    #[tokio::test]
    async fn recording_backend_records_calls_and_replies() -> Result<(), Box<dyn std::error::Error>>
    {
        let backend = RecordingBackend::new(ServerId::new("sales")?).with_reply(
            "broken",
            FakeReply::Error(BackendError::Protocol("unknown tool".to_owned())),
        );
        let arguments = json!({ "sku": "A1" })
            .as_object()
            .cloned()
            .unwrap_or_default();
        let output = backend
            .call_tool("lookup", arguments.clone(), CancellationToken::new())
            .await?;
        assert_eq!(
            output.content,
            [ToolContent::Text {
                text: "ok: lookup".to_owned()
            }]
        );
        let failed = backend
            .call_tool("broken", Map::new(), CancellationToken::new())
            .await;
        assert!(failed.is_err());
        let calls = backend.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].arguments, arguments);
        Ok(())
    }

    #[tokio::test]
    async fn recording_backend_hang_reports_unknown_outcome() -> Result<(), InvalidServerId> {
        let backend =
            RecordingBackend::new(ServerId::new("sales")?).with_reply("slow", FakeReply::Hang);
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(
            backend.call_tool("slow", Map::new(), cancel.clone()),
            async {
                cancel.cancel();
            }
        );
        assert!(matches!(result, Err(BackendError::OutcomeUnknown(_))));
        assert_eq!(backend.calls().len(), 1);
        Ok(())
    }

    #[test]
    fn manual_clock_returns_the_set_time() {
        let start = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let clock = ManualClock::new(start);
        assert_eq!(clock.now(), start);
        let later = start + Duration::from_secs(5);
        clock.set(later);
        assert_eq!(clock.now(), later);
    }
}
