#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity_export;
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

#[tauri::command]
async fn export_activity(
    app: tauri::AppHandle,
    request: SessionRequest,
    runtime: State<'_, Arc<DesktopRuntime>>,
) -> Result<String, BridgeError> {
    let bytes = runtime.export_activity(request)?;
    let directory = app.path().app_data_dir().map_err(|_| BridgeError::Export)?;
    tauri::async_runtime::spawn_blocking(move || {
        activity_export::save(&directory, &bytes)
            .map(|path| path.display().to_string())
            .map_err(|_| BridgeError::Export)
    })
    .await
    .map_err(|_| BridgeError::Export)?
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
    revision: String,
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
    provider_options(&settings)
}

fn provider_options(settings: &Settings) -> Result<ProvidersView, BridgeError> {
    let configured = settings
        .provider_choices()
        .map_err(|_| BridgeError::Configuration)?;
    let providers = configured.iter().map(|p| ProviderView {
        revision: settings::provider_revision(p),
        id: p.id.clone(), model: p.model.clone(),
        unavailable: p.api_key.as_ref().filter(|key| settings::key_variable_missing(key.env_var())).map(|key| format!("Environment variable {} is missing or unavailable. Set it before starting Igor.", key.env_var())),
    }).collect();
    Ok(ProvidersView { providers })
}

#[tauri::command]
async fn settings_providers(
    settings: State<'_, Arc<Settings>>,
) -> Result<settings::ProviderSettingsView, CommandError> {
    settings.provider_view()
}
#[tauri::command]
async fn settings_save_provider(
    settings: State<'_, Arc<Settings>>,
    request: settings::SaveProviderRequest,
) -> Result<settings::ProviderSettingsView, CommandError> {
    settings
        .save_provider(request.replacing.as_deref(), request.entry)
        .await
}
#[tauri::command]
async fn settings_remove_provider(
    settings: State<'_, Arc<Settings>>,
    request: settings::ProviderRequest,
) -> Result<settings::ProviderSettingsView, CommandError> {
    settings.remove_provider(&request.id).await
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
            export_activity,
            settings_providers,
            settings_save_provider,
            settings_remove_provider,
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
    #[test]
    fn provider_ipc_never_contains_environment_value() -> Result<(), Box<dyn std::error::Error>> {
        for mode in ["set", "empty", "missing"] {
            let mut child = std::process::Command::new(std::env::current_exe()?);
            child
                .args(["--exact", "tests::provider_ipc_sentinel_probe"])
                .env("IGOR_PROVIDER_PROBE_MODE", mode);
            match mode {
                "set" => {
                    child.env(
                        "IGOR_DESKTOP_SENTINEL_KEY_20261031",
                        "sk-sentinel-never-return-this-value",
                    );
                }
                "empty" => {
                    child.env("IGOR_DESKTOP_SENTINEL_KEY_20261031", "");
                }
                _ => {
                    child.env_remove("IGOR_DESKTOP_SENTINEL_KEY_20261031");
                }
            }
            assert!(
                child.output()?.status.success(),
                "provider presence/IPC probe failed"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn provider_ipc_sentinel_probe() -> Result<(), Box<dyn std::error::Error>> {
        let Ok(mode) = std::env::var("IGOR_PROVIDER_PROBE_MODE") else {
            return Ok(());
        };
        let dir = std::env::temp_dir().join(format!("igor-provider-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        let settings = Settings::new(Some(path.clone()));
        let entry = igor_config::ProviderEntry {
            id: "cloud".into(),
            kind: igor_config::ProviderKind::Anthropic,
            model: "test".into(),
            base_url: None,
            api_key_env: Some("IGOR_DESKTOP_SENTINEL_KEY_20261031".into()),
        };
        let request = serde_json::json!({"request":{"replacing":null,"entry":entry}});
        let saved = settings.save_provider(None, entry.clone()).await?;
        assert_eq!(saved.providers[0].key_missing.is_none(), mode == "set");
        let read = settings.provider_view()?;
        let picker = provider_options(&settings)?;
        let edited = settings.save_provider(Some("cloud"), entry).await?;
        let removed = settings.remove_provider("cloud").await?;
        let payloads =
            serde_json::json!([request, saved, read, picker, edited, removed]).to_string();
        assert!(!payloads.contains("sk-sentinel-never-return-this-value"));
        assert!(!std::fs::read_to_string(path)?.contains("sk-sentinel-never-return-this-value"));
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
