//! Sentinel-secret test (#10): a session with a provider error and a tool failure must not leak
//! the API key anywhere the operator or a log can see it. Runs the `igor-cli` binary with the
//! real Anthropic adapter against a loopback HTTP stub, and a real fixture MCP server.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SENTINEL: &str = "IGOR-TEST-SECRET-7f3a";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_server() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let status = Command::new(env!("CARGO"))
        .args(["build", "--locked", "-q", "-p", "igor-fixture-server"])
        .current_dir(workspace_root())
        .status()?;
    if !status.success() {
        return Err("cannot build igor-fixture-server".into());
    }
    Ok(workspace_root().join("target/debug/igor-fixture-server"))
}

/// Serves `replies` in order, one per connection, and records each request's headers.
fn stub(replies: Vec<(u16, Value)>) -> std::io::Result<(String, Arc<Mutex<Vec<String>>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    std::thread::spawn(move || {
        for (status, body) in replies {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut headers = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                headers.push_str(&line);
            }
            let mut request_body = vec![0; length];
            let _ = reader.read_exact(&mut request_body);
            if let Ok(mut seen) = record.lock() {
                seen.push(headers);
            }
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = reader.get_mut().write_all(response.as_bytes());
        }
    });
    Ok((url, seen))
}

#[test]
fn the_api_key_never_appears_in_output_events_or_logs() -> TestResult {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("igor-cli-secrets");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let fixture = dir.join("notes.json");
    std::fs::write(
        &fixture,
        json!({
            "server": { "name": "notes", "version": "1" },
            "tools": [{ "name": "list_notes", "inputSchema": { "type": "object", "properties": {} } }],
            "calls": [{ "tool": "list_notes", "arguments": "*",
                        "fault": { "tool_error": "storage unavailable" } }]
        })
        .to_string(),
    )?;
    // Turn: the model calls a tool that fails, then the provider rejects the next request with a
    // body that echoes the key, as a misbehaving proxy might.
    let (url, seen) = stub(vec![
        (
            200,
            json!({
                "id": "msg_1", "type": "message", "role": "assistant", "model": "stub",
                "content": [{ "type": "tool_use", "id": "toolu_1", "name": "notes__list_notes",
                              "input": {} }],
                "stop_reason": "tool_use", "stop_sequence": null,
                "usage": { "input_tokens": 1, "output_tokens": 1 }
            }),
        ),
        (
            400,
            json!({ "type": "error", "error": { "type": "invalid_request_error",
                    "message": format!("invalid x-api-key: {SENTINEL}") } }),
        ),
    ])?;
    let quote = |path: &Path| serde_json::to_string(&path.display().to_string());
    let config = format!(
        r#"[[providers]]
id = "claude"
kind = "anthropic"
model = "stub-model"
base_url = "{url}"
api_key = {{ env = "ANTHROPIC_API_KEY" }}

[[servers]]
id = "notes"
transport = "stdio"
command = {command}
args = ["--fixture", {fixture}, "--call-log", {log}]

[permissions.notes]
list_notes = "read"
"#,
        command = quote(&fixture_server()?)?,
        fixture = quote(&fixture)?,
        log = quote(&dir.join("calls.jsonl"))?,
    );
    std::fs::write(dir.join("igor.toml"), config)?;

    let output = Command::new(env!("CARGO_BIN_EXE_igor-cli"))
        .arg("--config")
        .arg(dir.join("igor.toml"))
        .args(["--provider", "claude", "--export"])
        .arg(dir.join("activity.jsonl"))
        .arg("What do my notes say?")
        .env_remove("IGOR_CONFIG")
        .env("ANTHROPIC_API_KEY", SENTINEL)
        .stdin(Stdio::null())
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // The run exercised both failure paths.
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("returned an error"), "{stdout}");
    assert!(stdout.contains("[failed: "), "{stdout}");
    // The echoed error reached the operator, with the key replaced.
    assert!(stdout.contains("invalid x-api-key: [REDACTED]"), "{stdout}");
    // The key was used where it belongs: the x-api-key header of both requests.
    let headers = seen.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert_eq!(headers.len(), 2);
    assert!(
        headers
            .iter()
            .all(|h| h.contains(&format!("x-api-key: {SENTINEL}")))
    );

    // And nowhere else.
    let export = std::fs::read_to_string(dir.join("activity.jsonl"))?;
    let calls = std::fs::read_to_string(dir.join("calls.jsonl"))?;
    assert!(export.contains("tool_call_completed") && export.contains("turn_finished"));
    for (place, text) in [
        ("stdout", stdout.as_ref()),
        ("stderr", stderr.as_ref()),
        ("activity export", export.as_str()),
        ("server call log", calls.as_str()),
    ] {
        assert!(
            !text.contains(SENTINEL),
            "the key leaked into {place}:\n{text}"
        );
    }
    Ok(())
}

#[test]
fn a_server_secret_printed_to_stderr_is_redacted() -> TestResult {
    const SERVER_SECRET: &str = "IGOR-TEST-SECRET-server-91c2";
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("igor-cli-stderr-secret");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("script.json"),
        json!({ "turns": [{ "text": "The broken server is unavailable." }] }).to_string(),
    )?;
    std::fs::write(
        dir.join("igor.toml"),
        r#"[[servers]]
id = "broken"
transport = "stdio"
command = "/bin/sh"
args = ["-c", "echo \"auth with $TOKEN failed\" >&2; exit 1"]
env = { TOKEN = { env = "IGOR_TEST_SERVER_TOKEN" } }
"#,
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_igor-cli"))
        .arg("--config")
        .arg(dir.join("igor.toml"))
        .args(["--provider", "scripted", "--script"])
        .arg(dir.join("script.json"))
        .arg("--export")
        .arg(dir.join("activity.jsonl"))
        .arg("Anything?")
        .env_remove("IGOR_CONFIG")
        .env("IGOR_TEST_SERVER_TOKEN", SERVER_SECRET)
        .stdin(Stdio::null())
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("[server broken: unavailable]"), "{stdout}");
    assert!(
        stdout.contains("  | auth with [REDACTED] failed"),
        "{stdout}"
    );
    let export = std::fs::read_to_string(dir.join("activity.jsonl"))?;
    for (name, text) in [
        ("stdout", &*stdout),
        ("stderr", &*stderr),
        ("export", &export),
    ] {
        assert!(
            !text.contains(SERVER_SECRET),
            "{name} leaks the server secret:\n{text}"
        );
    }
    Ok(())
}
