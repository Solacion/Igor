#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod composition;
mod sessions;
mod settings;
mod shutdown;
use igor_core::events::{EventEnvelope, TurnOutcome};
use igor_core::session::EventSink;

use sessions::{
    ApprovalRequest, BridgeError, ClarificationRequest, DesktopRuntime, MessageRequest,
    SessionInfo, SessionRequest, StartRequest,
};

use tauri::ipc::Channel;

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
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<SessionInfo, BridgeError> {
    runtime
        .start(request, &settings, |cancel| {
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
    settings: State<'_, Arc<Settings>>,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<TurnOutcome, BridgeError> {
    runtime.send_configured(request, &settings).await
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

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::settings::{
    CommandError, PermissionRequest, SaveServerRequest, ServerRequest, Settings, SettingsView,
    ToolView,
};

#[derive(Serialize)]
struct ProviderView {
    id: String,
    model: String,
    unavailable: Option<String>,
}
#[derive(Serialize)]
struct ProvidersView {
    providers: Vec<ProviderView>,
}
#[tauri::command]
async fn conversation_providers(
    settings: State<'_, Arc<Settings>>,
) -> Result<ProvidersView, BridgeError> {
    let snapshot = settings
        .snapshot()
        .await
        .map_err(|_| BridgeError::Configuration)?;
    let providers = snapshot.config.providers().iter().map(|p| ProviderView {
        id: p.id.clone(), model: p.model.clone(),
        unavailable: p.api_key.as_ref().filter(|key| key.resolve().is_err()).map(|key| format!("Environment variable {} is missing or unavailable. Set it before starting Igor.", key.env_var())),
    }).collect();
    Ok(ProvidersView { providers })
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

// Read-only metadata.
#[tauri::command]
fn desktop_status(request: StatusRequest) -> DesktopStatus {
    match request.scope {
        StatusScope::Runtime => DesktopStatus {
            app_version: env!("CARGO_PKG_VERSION"),
            core_event_schema: igor_core::events::EVENT_SCHEMA_VERSION,
        },
    }
}

#[tauri::command]
async fn settings_view(settings: State<'_, Arc<Settings>>) -> Result<SettingsView, CommandError> {
    settings.view().await
}

#[tauri::command]
async fn settings_save_server(
    settings: State<'_, Arc<Settings>>,
    request: SaveServerRequest,
) -> Result<SettingsView, CommandError> {
    settings
        .save_server(request.replacing.as_deref(), request.entry)
        .await
}

#[tauri::command]
async fn settings_test_server(
    settings: State<'_, Arc<Settings>>,
    request: SaveServerRequest,
) -> Result<Vec<ToolView>, CommandError> {
    settings
        .test_server(request.replacing.as_deref(), request.entry)
        .await
}

#[tauri::command]
async fn settings_remove_server(
    settings: State<'_, Arc<Settings>>,
    request: ServerRequest,
) -> Result<SettingsView, CommandError> {
    settings.remove_server(&request.id).await
}

#[tauri::command]
async fn settings_reconnect_server(
    settings: State<'_, Arc<Settings>>,
    request: ServerRequest,
) -> Result<SettingsView, CommandError> {
    settings.reconnect(&request.id).await
}

#[tauri::command]
async fn settings_set_permission(
    settings: State<'_, Arc<Settings>>,
    request: PermissionRequest,
) -> Result<SettingsView, CommandError> {
    settings
        .set_permission(&request.server, &request.tool, request.class)
        .await
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Arc::new(Settings::new(igor_config::config_path(None)));
    tauri::Builder::default()
        .manage(settings)
        .manage(Arc::new(DesktopRuntime::default()))
        .manage(Arc::new(shutdown::ExitState::default()))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep the main window/event loop alive while the exit callback reaps children.
                api.prevent_close();
                window.app_handle().exit(0);
            }
        })
        .setup(|app| {
            let settings = Arc::clone(app.state::<Arc<Settings>>().inner());
            tauri::async_runtime::spawn(async move { settings.connect_all().await });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_status,
            conversation_providers,
            start_session,
            send_message,
            resolve_approval,
            answer_clarification,
            cancel_turn,
            close_session,
            settings_view,
            settings_save_server,
            settings_test_server,
            settings_remove_server,
            settings_reconnect_server,
            settings_set_permission,
        ])
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                let exit = Arc::clone(app.state::<Arc<shutdown::ExitState>>().inner());
                if exit.finished() { return; }
                api.prevent_exit();
                if !exit.begin() { return; }
                let settings = Arc::clone(app.state::<Arc<Settings>>().inner());
                let runtime = Arc::clone(app.state::<Arc<DesktopRuntime>>().inner());
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title("Closing Igor — stopping servers…");
                }
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if !shutdown::shutdown(settings, runtime).await {
                        eprintln!("Desktop shutdown reached its 15-second deadline; server cleanup may be incomplete.");
                    }
                    exit.finish();
                    app.exit(code.unwrap_or(0));
                });
            }
        });
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
