//! One desktop-owned session, with short state locks and no frontend tool-dispatch path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use igor_core::CancellationToken;
use igor_core::approval::{ActionHash, Decision};
use igor_core::events::{EventEnvelope, TurnOutcome};
use igor_core::session::{EventSink, Session, SessionHandle};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::composition::{Prepared, prepare};

const MAX_TEXT_BYTES: usize = 32 * 1024;
const MAX_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum BridgeError {
    #[error("Invalid request")]
    InvalidRequest,
    #[error("A session or turn is already active")]
    Busy,
    #[error("The session is not available")]
    NoSession,
    #[error("There is no active turn")]
    NoTurn,
    #[error("Configuration could not be loaded")]
    Configuration,
    #[error("The configured provider could not be prepared")]
    Provider,
    #[error("The operation was cancelled")]
    Cancelled,
    #[error("Output containing a credential was blocked")]
    SensitiveOutput,
    #[error("Activity could not be exported completely")]
    Export,
    #[error("The desktop runtime is unavailable")]
    Unavailable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub provider_id: String,
}

impl StartRequest {
    pub fn validate(&self) -> Result<(), BridgeError> {
        if !valid_id(&self.provider_id) {
            return Err(BridgeError::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    pub session_id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequest {
    pub session_id: String,
    pub approval_id: String,
    pub action_hash: ActionHash,
    pub decision: Decision,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClarificationRequest {
    pub session_id: String,
    pub request_id: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub server_id: String,
    pub connected: bool,
    pub tool_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub provider_id: String,
    pub model: String,
    pub servers: Vec<ServerStatus>,
    pub revision: u64,
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_ID_BYTES && !value.chars().any(char::is_control)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_TEXT_BYTES && !value.contains('\0')
}

struct Active {
    activity: Arc<Mutex<crate::activity_export::ActivityLog>>,
    id: String,
    session: Arc<AsyncMutex<Session>>,
    handle: SessionHandle,
    lifetime: CancellationToken,
    turn: Option<CancellationToken>,
    sensitive_output: Arc<AtomicBool>,
    secrets: Arc<Mutex<Vec<igor_config::Secret>>>,
    _servers: Vec<Arc<crate::settings::SharedBackend>>,
}

impl Drop for Active {
    fn drop(&mut self) {
        self.lifetime.cancel();
    }
}

struct Starting {
    id: String,
    cancel: CancellationToken,
}

#[derive(Default)]
struct State {
    next_id: u64,
    starting: Option<Starting>,
    active: Option<Active>,
}

#[derive(Default)]
pub struct DesktopRuntime {
    state: Mutex<State>,
}

impl DesktopRuntime {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, BridgeError> {
        self.state.lock().map_err(|_| BridgeError::Unavailable)
    }

    fn reserve(&self) -> Result<(String, CancellationToken), BridgeError> {
        let mut state = self.lock()?;
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.lifetime.is_cancelled())
        {
            state.active = None;
        }
        if state.starting.is_some() || state.active.is_some() {
            return Err(BridgeError::Busy);
        }
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or(BridgeError::Unavailable)?;
        let id = format!("desktop-{}", state.next_id);
        let cancel = CancellationToken::new();
        state.starting = Some(Starting {
            id: id.clone(),
            cancel: cancel.clone(),
        });
        Ok((id, cancel))
    }

    pub async fn start(
        &self,
        request: StartRequest,
        settings: &crate::settings::Settings,
        sink: impl FnOnce(CancellationToken) -> Arc<dyn EventSink>,
    ) -> Result<SessionInfo, BridgeError> {
        request.validate()?;
        let (id, cancel) = self.reserve()?;
        let _lease = StartLease {
            runtime: self,
            id: id.clone(),
            cancel: cancel.clone(),
        };
        let sink = sink(cancel.clone());
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(BridgeError::Cancelled),
            result = prepare(request, &cancel, settings) => result,
        };
        match result {
            Ok(prepared) => self.install(id, cancel, prepared, sink),
            Err(error) => {
                let mut state = self.lock()?;
                if state.starting.as_ref().is_some_and(|start| start.id == id) {
                    state.starting = None;
                }
                Err(error)
            }
        }
    }

    fn install(
        &self,
        id: String,
        cancel: CancellationToken,
        mut prepared: Prepared,
        sink: Arc<dyn EventSink>,
    ) -> Result<SessionInfo, BridgeError> {
        let activity = Arc::new(Mutex::new(crate::activity_export::ActivityLog::default()));
        let sink = Arc::new(crate::activity_export::RecordingSink {
            next: sink,
            log: activity.clone(),
        });
        let mut state = self.lock()?;
        if cancel.is_cancelled() || !state.starting.as_ref().is_some_and(|start| start.id == id) {
            return Err(BridgeError::Cancelled);
        }
        let sensitive_output = Arc::new(AtomicBool::new(false));
        let secrets = Arc::new(Mutex::new(prepared.secrets));
        prepared.parts.events = Arc::new(CredentialGuard {
            next: sink,
            secrets: secrets.clone(),
            cancel: cancel.clone(),
            blocked: sensitive_output.clone(),
        });
        let (session, handle) = Session::new(prepared.parts, prepared.config);
        state.active = Some(Active {
            activity,
            id: id.clone(),
            session: Arc::new(AsyncMutex::new(session)),
            handle,
            lifetime: cancel,
            turn: None,
            sensitive_output,
            secrets,
            _servers: prepared.servers,
        });
        state.starting = None;
        Ok(SessionInfo {
            session_id: id,
            provider_id: prepared.provider_id,
            model: prepared.model,
            servers: prepared.statuses,
            revision: prepared.revision,
        })
    }

    pub async fn send_configured(
        &self,
        request: MessageRequest,
        settings: &crate::settings::Settings,
    ) -> Result<TurnOutcome, BridgeError> {
        let snapshot = settings
            .snapshot()
            .await
            .map_err(|_| BridgeError::Configuration)?;
        self.send_with_snapshot(request, Some(snapshot)).await
    }

    #[cfg(test)]
    pub async fn send(&self, request: MessageRequest) -> Result<TurnOutcome, BridgeError> {
        self.send_with_snapshot(request, None).await
    }

    async fn send_with_snapshot(
        &self,
        request: MessageRequest,
        snapshot: Option<crate::settings::Snapshot>,
    ) -> Result<TurnOutcome, BridgeError> {
        if !valid_text(&request.text) {
            return Err(BridgeError::InvalidRequest);
        }
        let (session, cancel, blocked, secrets) = {
            let mut state = self.lock()?;
            let active = active_mut(&mut state, &request.session_id)?;
            if active.turn.is_some() {
                return Err(BridgeError::Busy);
            }
            let cancel = active.lifetime.child_token();
            active.turn = Some(cancel.clone());
            (
                active.session.clone(),
                cancel,
                active.sensitive_output.clone(),
                active.secrets.clone(),
            )
        };
        let _lease = TurnLease {
            runtime: self,
            id: request.session_id.clone(),
            cancel: cancel.clone(),
        };
        // This async lock serializes only the core conversation, not approvals or cancellation.
        let mut session = session.lock().await;
        if let Some(snapshot) = snapshot {
            // The turn reservation prevents another send from racing this replacement.
            // Keep prior credentials guarded too: earlier messages remain in history.
            let mut added_secrets = Vec::new();
            for server in snapshot.config.servers() {
                for reference in server.env.values() {
                    if let Ok(secret) = reference.resolve() {
                        added_secrets.push(secret);
                    }
                }
            }
            {
                let mut guarded = secrets.lock().map_err(|_| BridgeError::Unavailable)?;
                for secret in added_secrets {
                    if !guarded.iter().any(|old| old.expose() == secret.expose()) {
                        guarded.push(secret);
                    }
                }
            }
            let classes = snapshot
                .descriptors
                .iter()
                .map(|tool| {
                    (
                        tool.id.clone(),
                        snapshot.config.permissions().classify(&tool.id),
                    )
                })
                .collect();
            // Release old leases; Settings retains current connections and owns exit cleanup.
            let replaced = session.replace_tools(
                igor_core::registry::ToolRegistry::build(snapshot.descriptors, &classes),
                snapshot.backends,
            );
            session.set_unavailable_servers(snapshot.unavailable_servers);
            {
                let mut state = self.lock()?;
                let active = active_mut(&mut state, &request.session_id)?;
                active._servers = snapshot.servers;
            }
            drop(replaced);
        }
        let outcome = session.send_message(request.text, cancel).await;
        drop(session);
        let mut state = self.lock()?;
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.id == request.session_id)
            && blocked.load(Ordering::Acquire)
        {
            state.active = None;
        }
        if blocked.load(Ordering::Acquire) {
            Err(BridgeError::SensitiveOutput)
        } else {
            Ok(outcome)
        }
    }

    pub fn approve(&self, request: ApprovalRequest) -> Result<(), BridgeError> {
        if !valid_id(&request.approval_id) {
            return Err(BridgeError::InvalidRequest);
        }
        let mut state = self.lock()?;
        let active = active_mut(&mut state, &request.session_id)?;
        if active.turn.is_none() {
            return Err(BridgeError::NoTurn);
        }
        active
            .handle
            .resolve_approval(request.approval_id, request.action_hash, request.decision)
            .map_err(|_| BridgeError::NoSession)
    }

    pub fn clarify(&self, request: ClarificationRequest) -> Result<(), BridgeError> {
        if !valid_id(&request.request_id) || !valid_text(&request.answer) {
            return Err(BridgeError::InvalidRequest);
        }
        let mut state = self.lock()?;
        let active = active_mut(&mut state, &request.session_id)?;
        if active.turn.is_none() {
            return Err(BridgeError::NoTurn);
        }
        active
            .handle
            .answer_clarification(request.request_id, request.answer)
            .map_err(|_| BridgeError::NoSession)
    }

    pub fn export_activity(&self, request: SessionRequest) -> Result<Vec<u8>, BridgeError> {
        let mut state = self.lock()?;
        let active = active_mut(&mut state, &request.session_id)?;
        if active.turn.is_some() {
            return Err(BridgeError::Busy);
        }
        let bytes = active
            .activity
            .lock()
            .map_err(|_| BridgeError::Export)?
            .snapshot()?;
        Ok(bytes)
    }

    pub fn cancel(&self, request: SessionRequest) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        let active = active_mut(&mut state, &request.session_id)?;
        if let Some(turn) = &active.turn {
            turn.cancel();
        }
        Ok(())
    }

    pub fn close(&self, request: SessionRequest) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        if !valid_id(&request.session_id) {
            return Err(BridgeError::InvalidRequest);
        }
        if !state
            .active
            .as_ref()
            .is_some_and(|active| active.id == request.session_id)
        {
            return Err(BridgeError::NoSession);
        }
        state.active = None;
        Ok(())
    }

    pub fn shutdown(&self) {
        // Shutdown still cancels resources if an unrelated caller poisoned the lock.
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(error) => error.into_inner(),
        };
        if let Some(start) = state.starting.take() {
            start.cancel.cancel();
        }
        state.active = None;
    }
}

// Invocation futures can be dropped when the webview disappears. Always release reservations.
struct StartLease<'a> {
    runtime: &'a DesktopRuntime,
    id: String,
    cancel: CancellationToken,
}
impl Drop for StartLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.runtime.lock()
            && state
                .starting
                .as_ref()
                .is_some_and(|start| start.id == self.id)
        {
            self.cancel.cancel();
            state.starting = None;
        }
    }
}
struct TurnLease<'a> {
    runtime: &'a DesktopRuntime,
    id: String,
    cancel: CancellationToken,
}
impl Drop for TurnLease<'_> {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Ok(mut state) = self.runtime.lock()
            && let Some(active) = state.active.as_mut().filter(|active| active.id == self.id)
        {
            active.turn = None;
        }
    }
}

fn active_mut<'a>(state: &'a mut State, id: &str) -> Result<&'a mut Active, BridgeError> {
    if !valid_id(id) {
        return Err(BridgeError::InvalidRequest);
    }
    state
        .active
        .as_mut()
        .filter(|active| active.id == id && !active.lifetime.is_cancelled())
        .ok_or(BridgeError::NoSession)
}

impl Drop for DesktopRuntime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Never rewrite approval arguments: block the event/turn if a resolved credential appears.
struct CredentialGuard {
    next: Arc<dyn EventSink>,
    secrets: Arc<Mutex<Vec<igor_config::Secret>>>,
    cancel: CancellationToken,
    blocked: Arc<AtomicBool>,
}

impl EventSink for CredentialGuard {
    fn emit(&self, event: EventEnvelope) {
        if self.blocked.load(Ordering::Acquire) {
            return;
        }
        let safe = serde_json::to_value(&event).is_ok_and(|value| {
            self.secrets.lock().is_ok_and(|secrets| {
                !secrets
                    .iter()
                    .any(|secret| contains_secret(&value, secret.expose()))
            })
        });
        if safe {
            self.next.emit(event);
        } else {
            self.blocked.store(true, Ordering::Release);
            self.cancel.cancel();
        }
    }
}

fn contains_secret(value: &serde_json::Value, secret: &str) -> bool {
    if secret.is_empty() {
        return false;
    }
    match value {
        serde_json::Value::String(text) => text.contains(secret),
        serde_json::Value::Array(values) => {
            values.iter().any(|value| contains_secret(value, secret))
        }
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains(secret) || contains_secret(value, secret)),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
