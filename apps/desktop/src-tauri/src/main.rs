#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};

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
        .invoke_handler(tauri::generate_handler![desktop_status])
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
