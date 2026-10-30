fn main() -> Result<(), Box<dyn std::error::Error>> {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "desktop_status",
            "start_session",
            "send_message",
            "resolve_approval",
            "answer_clarification",
            "cancel_turn",
            "close_session",
        ]),
    ))?;
    Ok(())
}
