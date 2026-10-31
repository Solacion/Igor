#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::settings::{
    CommandError, PermissionRequest, SaveServerRequest, ServerRequest, Settings, SettingsView,
    ToolView,
};

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
        .setup(|app| {
            let settings = Arc::clone(app.state::<Arc<Settings>>().inner());
            tauri::async_runtime::spawn(async move { settings.connect_all().await });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_status,
            settings_view,
            settings_save_server,
            settings_test_server,
            settings_remove_server,
            settings_reconnect_server,
            settings_set_permission,
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
