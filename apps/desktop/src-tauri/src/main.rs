#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod composition;
mod sessions;

use igor_core::events::{EventEnvelope, TurnOutcome};
use igor_core::session::EventSink;
use serde::{Deserialize, Serialize};
use sessions::{
    ApprovalRequest, BridgeError, ClarificationRequest, DesktopRuntime, MessageRequest,
    SessionInfo, SessionRequest, StartRequest,
};
use std::sync::Arc;
use tauri::{Manager, State, ipc::Channel};

struct ChannelSink {
    channel: Channel<EventEnvelope>,
    cancel: igor_core::CancellationToken,
}
impl EventSink for ChannelSink {
    fn emit(&self, event: EventEnvelope) {
        if self.channel.send(event).is_err() {
            self.cancel.cancel();
        }
    }
}

#[tauri::command]
async fn start_session(
    request: StartRequest,
    events: Channel<EventEnvelope>,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<SessionInfo, BridgeError> {
    runtime
        .start(request, |cancel| {
            Arc::new(ChannelSink {
                channel: events,
                cancel,
            })
        })
        .await
}
#[tauri::command]
async fn send_message(
    request: MessageRequest,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<TurnOutcome, BridgeError> {
    runtime.send(request).await
}
#[tauri::command]
fn resolve_approval(
    request: ApprovalRequest,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<(), BridgeError> {
    runtime.approve(request)
}
#[tauri::command]
fn answer_clarification(
    request: ClarificationRequest,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<(), BridgeError> {
    runtime.clarify(request)
}
#[tauri::command]
fn cancel_turn(
    request: SessionRequest,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<(), BridgeError> {
    runtime.cancel(request)
}
#[tauri::command]
fn close_session(
    request: SessionRequest,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<(), BridgeError> {
    runtime.close(request)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StatusScope {
    Runtime,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRequest {
    scope: StatusScope,
}

#[derive(Debug, Serialize)]
struct DesktopStatus {
    app_version: &'static str,
    core_event_schema: u32,
}

// Read-only metadata. No configuration, files, provider calls or tool dispatch are exposed.
#[tauri::command]
fn desktop_status(request: StatusRequest) -> DesktopStatus {
    match request.scope {
        StatusScope::Runtime => DesktopStatus {
            app_version: env!("CARGO_PKG_VERSION"),
            core_event_schema: igor_core::events::EVENT_SCHEMA_VERSION,
        },
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .manage(Arc::new(DesktopRuntime::default()))
        .on_window_event(|window, event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                window.state::<Arc<DesktopRuntime>>().shutdown();
            }
        })
        .invoke_handler(tauri::generate_handler![
            desktop_status,
            start_session,
            send_message,
            resolve_approval,
            answer_clarification,
            cancel_turn,
            close_session
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_rejects_unknown_scopes_and_fields() {
        for input in [
            r#"{}"#,
            r#"{"scope":"files"}"#,
            r#"{"scope":"runtime","path":"secret"}"#,
        ] {
            assert!(serde_json::from_str::<StatusRequest>(input).is_err());
        }
    }

    #[test]
    fn status_reads_the_embedded_core_schema() -> Result<(), serde_json::Error> {
        let input = serde_json::from_str(r#"{"scope":"runtime"}"#)?;
        let status = desktop_status(input);
        assert_eq!(
            status.core_event_schema,
            igor_core::events::EVENT_SCHEMA_VERSION
        );
        let value = serde_json::to_value(status)?;
        assert_eq!(value.as_object().map(|object| object.len()), Some(2));
        Ok(())
    }
}
