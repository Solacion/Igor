//! `McpBackend` against the real `igor-fixture-server` binary over stdio.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use igor_core::CancellationToken;
use igor_core::tools::{BackendError, ServerId, ToolBackend, ToolContent};
use igor_mcp::{ConnectError, McpBackend, StdioServerConfig};
use serde_json::{Map, Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds the fixture server once and returns its path. A dev-dependency would build only the
/// library, not the binary.
fn fixture_server() -> Result<PathBuf, String> {
    static BIN: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BIN.get_or_init(|| {
        let output = std::process::Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "-p",
                "igor-fixture-server",
                "--bin",
                "igor-fixture-server",
                "--message-format=json",
            ])
            .current_dir(workspace_root())
            .output()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|msg| msg["reason"] == "compiler-artifact" && msg["executable"].is_string())
            .and_then(|msg| msg["executable"].as_str().map(PathBuf::from))
            .ok_or_else(|| "cargo reported no fixture server executable".to_owned())
    })
    .clone()
}

fn test_dir(name: &str) -> std::io::Result<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("igor-mcp")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn sales_fixture() -> PathBuf {
    workspace_root().join("crates/igor-fixture-server/fixtures/sales.json")
}

/// A fixture with a slow tool, for timeout and cancellation tests.
fn slow_fixture(dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join("slow.json");
    let fixture = json!({
        "server": { "name": "slow", "version": "1" },
        "tools": [
            { "name": "slow", "inputSchema": { "type": "object" } },
            { "name": "fast", "inputSchema": { "type": "object" } }
        ],
        "calls": [
            { "tool": "slow", "arguments": {}, "result": { "content": [] },
              "fault": { "delay_ms": 10_000 } },
            { "tool": "fast", "arguments": {}, "result": { "content": [] } }
        ]
    });
    std::fs::write(&path, fixture.to_string())?;
    Ok(path)
}

fn config(fixture: &Path, call_log: &Path) -> Result<StdioServerConfig, String> {
    let mut config = StdioServerConfig::new(fixture_server()?);
    config.args = vec![
        "--fixture".into(),
        fixture.into(),
        "--call-log".into(),
        call_log.into(),
    ];
    Ok(config)
}

fn logged_tools(call_log: &Path) -> std::io::Result<Vec<String>> {
    Ok(std::fs::read_to_string(call_log)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|record| record["tool"].as_str().map(str::to_owned))
        .collect())
}

fn args(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[tokio::test]
async fn lists_and_calls_tools() -> TestResult {
    let dir = test_dir("lists_and_calls")?;
    let log = dir.join("calls.jsonl");
    let backend =
        McpBackend::connect(ServerId::new("sales")?, &config(&sales_fixture(), &log)?).await?;
    assert!(backend.is_connected());

    let tools = backend.list_tools().await?;
    let ids: Vec<String> = tools.iter().map(|t| t.id.to_string()).collect();
    assert_eq!(
        ids,
        [
            "sales/get_stock_levels",
            "sales/get_sales_history",
            "sales/create_draft_order"
        ]
    );
    assert_eq!(tools[0].hints.read_only, Some(true));
    assert_eq!(tools[2].hints.read_only, Some(false));
    assert_eq!(tools[2].hints.destructive, Some(false));
    assert_eq!(
        tools[0].input_schema["required"],
        json!(["supplier_id", "date"])
    );

    let stock = backend
        .call_tool(
            "get_stock_levels",
            args(json!({ "supplier_id": "supplier-x", "date": "2026-07-13" })),
            CancellationToken::new(),
        )
        .await?;
    assert!(!stock.is_error);
    let structured = stock.structured.ok_or("no structured content")?;
    assert_eq!(structured["supplier"]["id"], "supplier-x");
    assert!(matches!(
        stock.content.as_slice(),
        [ToolContent::Text { .. }]
    ));

    let missing = backend
        .call_tool(
            "get_sales_history",
            args(json!({ "supplier_id": "supplier-x", "from": "2026-08-01", "to": "2026-08-02" })),
            CancellationToken::new(),
        )
        .await?;
    assert!(missing.is_error, "missing data is a tool error result");

    let unknown = backend
        .call_tool("no_such_tool", Map::new(), CancellationToken::new())
        .await;
    assert!(
        matches!(&unknown, Err(BackendError::Protocol(m)) if m.contains("-32602")),
        "{unknown:?}"
    );
    assert_eq!(
        logged_tools(&log)?,
        ["get_stock_levels", "get_sales_history", "no_such_tool"]
    );
    Ok(())
}

#[tokio::test]
async fn timeout_after_send_is_outcome_unknown() -> TestResult {
    let dir = test_dir("timeout")?;
    let log = dir.join("calls.jsonl");
    let mut config = config(&slow_fixture(&dir)?, &log)?;
    config.call_timeout = Duration::from_millis(300);
    let backend = McpBackend::connect(ServerId::new("slow")?, &config).await?;

    let result = backend
        .call_tool("slow", Map::new(), CancellationToken::new())
        .await;
    match &result {
        Err(error @ BackendError::OutcomeUnknown(_)) => {
            assert!(!error.definitely_not_executed());
        }
        other => return Err(format!("expected OutcomeUnknown, got {other:?}").into()),
    }

    // The session survives; the server received the slow call exactly once (no retry).
    backend
        .call_tool("fast", Map::new(), CancellationToken::new())
        .await?;
    assert_eq!(logged_tools(&log)?, ["slow", "fast"]);
    Ok(())
}

#[tokio::test]
async fn cancellation_before_and_after_send() -> TestResult {
    let dir = test_dir("cancellation")?;
    let log = dir.join("calls.jsonl");
    let backend =
        McpBackend::connect(ServerId::new("slow")?, &config(&slow_fixture(&dir)?, &log)?).await?;

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let before = backend.call_tool("slow", Map::new(), cancelled).await;
    assert_eq!(before, Err(BackendError::CancelledBeforeSend));

    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        trigger.cancel();
    });
    let after = backend.call_tool("slow", Map::new(), cancel).await;
    canceller.await?;
    assert!(
        matches!(after, Err(BackendError::OutcomeUnknown(_))),
        "{after:?}"
    );

    backend
        .call_tool("fast", Map::new(), CancellationToken::new())
        .await?;
    // The call cancelled before sending never reached the server.
    assert_eq!(logged_tools(&log)?, ["slow", "fast"]);
    Ok(())
}

#[tokio::test]
async fn disconnected_backend_does_not_send() -> TestResult {
    let dir = test_dir("disconnect")?;
    let log = dir.join("calls.jsonl");
    let backend =
        McpBackend::connect(ServerId::new("sales")?, &config(&sales_fixture(), &log)?).await?;
    backend.disconnect();
    assert!(!backend.is_connected());
    assert_eq!(backend.list_tools().await, Err(BackendError::NotConnected));
    let call = backend
        .call_tool("get_stock_levels", Map::new(), CancellationToken::new())
        .await;
    assert_eq!(call, Err(BackendError::NotConnected));
    assert!(logged_tools(&log)?.is_empty());
    Ok(())
}

/// The child sees only allowlisted and explicitly set variables. A shell wrapper records the
/// child's environment, then runs the fixture server with it.
#[tokio::test]
async fn child_environment_is_allowlisted() -> TestResult {
    let dir = test_dir("environment")?;
    let env_dump = dir.join("env.txt");
    let mut config = StdioServerConfig::new("/bin/sh");
    config.args = vec![
        "-c".into(),
        r#"/usr/bin/env > "$0"; exec "$@""#.into(),
        env_dump.clone().into(),
        fixture_server()?.into(),
    ];
    config.env_allowlist = vec!["PATH".into(), "IGOR_MCP_TEST_UNSET".into()];
    config.env = vec![("IGOR_FIXTURE_FILE".into(), sales_fixture().into())];
    let backend = McpBackend::connect(ServerId::new("sales")?, &config).await?;
    assert_eq!(backend.list_tools().await?.len(), 3);

    let names: BTreeSet<String> = std::fs::read_to_string(&env_dump)?
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_owned()))
        .collect();
    // Variables the shell itself sets.
    let shell = ["PWD", "SHLVL", "_", "OLDPWD"];
    let unexpected: Vec<&String> = names
        .iter()
        .filter(|n| !["PATH", "IGOR_FIXTURE_FILE"].contains(&n.as_str()))
        .filter(|n| !shell.contains(&n.as_str()))
        .collect();
    assert!(unexpected.is_empty(), "inherited: {unexpected:?}");
    assert!(names.contains("IGOR_FIXTURE_FILE"));
    assert_eq!(
        names.contains("PATH"),
        std::env::var_os("PATH").is_some(),
        "allowlisted PATH is passed when set"
    );
    Ok(())
}

#[tokio::test]
async fn startup_failures_are_reported() -> TestResult {
    let missing = StdioServerConfig::new("/nonexistent/igor-server");
    let error = McpBackend::connect(ServerId::new("missing")?, &missing).await;
    assert!(
        matches!(error, Err(ConnectError::Spawn { .. })),
        "{error:?}"
    );

    // A process that exits without speaking MCP.
    let mut exits = StdioServerConfig::new("/usr/bin/true");
    exits.startup_timeout = Duration::from_secs(5);
    let error = McpBackend::connect(ServerId::new("exits")?, &exits).await;
    assert!(
        matches!(error, Err(ConnectError::Initialize { .. })),
        "{error:?}"
    );

    // A process that never answers `initialize`.
    let mut silent = StdioServerConfig::new("/bin/sleep");
    silent.args = vec!["30".into()];
    silent.startup_timeout = Duration::from_millis(300);
    let error = McpBackend::connect(ServerId::new("silent")?, &silent).await;
    assert!(
        matches!(error, Err(ConnectError::Timeout { .. })),
        "{error:?}"
    );
    Ok(())
}

/// A fixture with one tool per protocol-level fault.
fn fault_fixture(dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join("faults.json");
    let tools: Vec<Value> = [
        "hang",
        "early",
        "crash",
        "malformed",
        "oversized",
        "fast",
        "changes",
    ]
    .iter()
    .map(|name| json!({ "name": name, "inputSchema": { "type": "object" } }))
    .collect();
    let fixture = json!({
        "server": { "name": "faults", "version": "1" },
        "tools": tools,
        "calls": [
            { "tool": "hang", "arguments": {}, "fault": { "hang": true } },
            { "tool": "early", "arguments": {}, "fault": { "crash_before_response": true } },
            { "tool": "crash", "arguments": {}, "fault": { "crash_after_record": true } },
            { "tool": "malformed", "arguments": {}, "fault": { "malformed_response": true } },
            { "tool": "oversized", "arguments": {}, "fault": { "oversized_result": 1_048_576 } },
            { "tool": "fast", "arguments": {}, "result": { "content": [] } },
            { "tool": "changes", "arguments": {}, "result": { "content": [] },
              "fault": { "notify_tools_changed": true } }
        ]
    });
    std::fs::write(&path, fixture.to_string())?;
    Ok(path)
}

async fn fault_backend(
    name: &str,
    call_timeout: Duration,
) -> Result<(McpBackend, PathBuf), Box<dyn std::error::Error>> {
    let dir = test_dir(name)?;
    let log = dir.join("calls.jsonl");
    let mut config = config(&fault_fixture(&dir)?, &log)?;
    config.call_timeout = call_timeout;
    Ok((
        McpBackend::connect(ServerId::new("faults")?, &config).await?,
        log,
    ))
}

#[tokio::test]
async fn hang_is_outcome_unknown_after_timeout() -> TestResult {
    let (backend, log) = fault_backend("hang", Duration::from_millis(300)).await?;
    let result = backend
        .call_tool("hang", Map::new(), CancellationToken::new())
        .await;
    assert!(
        matches!(result, Err(BackendError::OutcomeUnknown(_))),
        "{result:?}"
    );
    backend
        .call_tool("fast", Map::new(), CancellationToken::new())
        .await?;
    assert_eq!(logged_tools(&log)?, ["hang", "fast"]);
    Ok(())
}

#[tokio::test]
async fn crash_after_record_is_outcome_unknown_then_not_connected() -> TestResult {
    let (backend, log) = fault_backend("crash", Duration::from_secs(10)).await?;
    let result = backend
        .call_tool("crash", Map::new(), CancellationToken::new())
        .await;
    assert!(
        matches!(result, Err(BackendError::OutcomeUnknown(_))),
        "{result:?}"
    );
    // The server recorded the call before exiting: the write may have happened.
    assert_eq!(logged_tools(&log)?, ["crash"]);
    let next = backend
        .call_tool("fast", Map::new(), CancellationToken::new())
        .await;
    assert_eq!(next, Err(BackendError::NotConnected));
    Ok(())
}

#[tokio::test]
async fn crash_before_response_is_not_logged() -> TestResult {
    let (backend, log) = fault_backend("early", Duration::from_secs(10)).await?;
    let result = backend
        .call_tool("early", Map::new(), CancellationToken::new())
        .await;
    assert!(
        matches!(result, Err(BackendError::OutcomeUnknown(_))),
        "{result:?}"
    );
    // The server exited before recording the call.
    assert!(logged_tools(&log)?.is_empty());
    assert!(!backend.is_connected());
    Ok(())
}

#[tokio::test]
async fn malformed_response_is_outcome_unknown() -> TestResult {
    let (backend, log) = fault_backend("malformed", Duration::from_millis(500)).await?;
    let result = backend
        .call_tool("malformed", Map::new(), CancellationToken::new())
        .await;
    assert!(
        matches!(result, Err(BackendError::OutcomeUnknown(_))),
        "{result:?}"
    );
    // rmcp skips the line that is not JSON-RPC; the session stays up and the call times out.
    assert!(backend.is_connected());
    assert_eq!(logged_tools(&log)?, ["malformed"]);
    Ok(())
}

#[tokio::test]
async fn oversized_result_is_returned_unbounded() -> TestResult {
    let (backend, _log) = fault_backend("oversized", Duration::from_secs(10)).await?;
    let output = backend
        .call_tool("oversized", Map::new(), CancellationToken::new())
        .await?;
    match output.content.as_slice() {
        [ToolContent::Text { text }] => assert_eq!(text.len(), 1_048_576),
        other => return Err(format!("unexpected content {other:?}").into()),
    }
    Ok(())
}

#[tokio::test]
async fn tool_list_changed_notifications_bump_the_version() -> TestResult {
    let (backend, _log) = fault_backend("changes", Duration::from_secs(10)).await?;
    assert_eq!(backend.tool_list_version(), 0);
    backend
        .call_tool("fast", Map::new(), CancellationToken::new())
        .await?;
    assert_eq!(
        backend.tool_list_version(),
        0,
        "an ordinary call changes nothing"
    );
    backend
        .call_tool("changes", Map::new(), CancellationToken::new())
        .await?;
    // The notification is handled on the client's own task; allow it to arrive.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while backend.tool_list_version() == 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(backend.tool_list_version(), 1);
    Ok(())
}

const STDERR_SECRET: &str = "IGOR-TEST-SECRET-5e1d";

/// A `/bin/sh -c script` server given `TOKEN` as a secret.
fn shell(script: &str) -> StdioServerConfig {
    let mut config = StdioServerConfig::new("/bin/sh");
    config.args = vec!["-c".into(), script.into()];
    config.env = vec![("TOKEN".to_owned(), STDERR_SECRET.into())];
    config.startup_timeout = Duration::from_secs(5);
    config
}

#[tokio::test]
async fn a_failed_start_carries_its_redacted_stderr() -> TestResult {
    let crashing = shell(r#"echo "boot: token=$TOKEN" >&2; echo "module not found" >&2; exit 3"#);
    let error = McpBackend::connect(ServerId::new("crashing")?, &crashing)
        .await
        .err()
        .ok_or("a crashing server connected")?;
    assert!(
        matches!(error, ConnectError::Initialize { .. }),
        "{error:?}"
    );
    assert_eq!(
        error.stderr(),
        ["boot: token=[REDACTED]", "module not found"]
    );
    assert!(!format!("{error} {error:?}").contains(STDERR_SECRET));

    let mut silent = shell(r#"echo "waiting with $TOKEN" >&2; exec sleep 30"#);
    silent.startup_timeout = Duration::from_millis(300);
    let error = McpBackend::connect(ServerId::new("silent")?, &silent)
        .await
        .err()
        .ok_or("a silent server connected")?;
    assert!(matches!(error, ConnectError::Timeout { .. }), "{error:?}");
    assert_eq!(error.stderr(), ["waiting with [REDACTED]"]);
    Ok(())
}

#[tokio::test]
async fn a_running_server_keeps_its_recent_redacted_stderr() -> TestResult {
    let server = fixture_server()?;
    let script = format!(
        r#"for n in $(seq 1 60); do echo "line $n" >&2; done; echo "using $TOKEN" >&2; exec "{}" --fixture "{}""#,
        server.display(),
        sales_fixture().display()
    );
    let backend = McpBackend::connect(ServerId::new("sales")?, &shell(&script)).await?;
    assert!(!backend.list_tools().await?.is_empty());
    let mut lines = Vec::new();
    for _ in 0..50 {
        lines = ToolBackend::recent_stderr(&backend);
        if lines.iter().any(|line| line.starts_with("using")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(lines.len(), igor_mcp::MAX_STDERR_LINES, "{lines:?}");
    assert_eq!(lines.first().map(String::as_str), Some("line 12"));
    assert!(lines.contains(&"using [REDACTED]".to_owned()), "{lines:?}");
    assert!(lines.iter().all(|line| !line.contains(STDERR_SECRET)));
    backend.shutdown().await;
    Ok(())
}
