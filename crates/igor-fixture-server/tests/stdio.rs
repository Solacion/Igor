//! Spawns the fixture server binary and talks MCP to it over stdio with the `rmcp` client.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock};
use rmcp::service::{RunningService, ServiceError};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value, json};
use tokio::process::Command;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Client = RunningService<RoleClient, ()>;

const SERVER: &str = env!("CARGO_BIN_EXE_igor-fixture-server");

fn sales_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sales.json")
}

/// A fresh per-test directory under Cargo's integration-test temp dir.
fn test_dir(name: &str) -> std::io::Result<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("igor-fixture-server")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

async fn connect(fixture: &Path, call_log: &Path) -> Result<Client, Box<dyn std::error::Error>> {
    let command = Command::new(SERVER).configure(|cmd| {
        cmd.arg("--fixture")
            .arg(fixture)
            .arg("--call-log")
            .arg(call_log)
            .env_clear();
    });
    Ok(().serve(TokioChildProcess::new(command)?).await?)
}

async fn call(
    client: &Client,
    tool: &str,
    arguments: Value,
) -> Result<CallToolResult, ServiceError> {
    let mut params = CallToolRequestParams::new(tool.to_owned());
    params.arguments = arguments.as_object().cloned();
    match client.peer().call_tool_once(params).await? {
        CallToolResponse::Complete(result) => Ok(result),
        _ => Err(ServiceError::UnexpectedResponse),
    }
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

fn error_code(result: &CallToolResult) -> Option<String> {
    let body: Value = serde_json::from_str(&text(result)).ok()?;
    body["error"]["code"].as_str().map(str::to_owned)
}

fn read_log(path: &Path) -> std::io::Result<Vec<Map<String, Value>>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| value.as_object().cloned())
        .collect())
}

#[tokio::test]
async fn serves_sales_fixture_and_logs_every_call() -> TestResult {
    let dir = test_dir("sales")?;
    let log_path = dir.join("calls.jsonl");
    let client = connect(&sales_fixture(), &log_path).await?;

    let info = client.peer().peer_info().ok_or("no server info")?;
    let server = info.server_info.as_ref().ok_or("no implementation info")?;
    assert_eq!(server.name, "igor-fixture-sales");
    assert_eq!(info.protocol_version.as_str(), "2025-11-25");

    let tools = client.peer().list_all_tools().await?;
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert_eq!(
        names,
        [
            "get_stock_levels",
            "get_sales_history",
            "create_draft_order"
        ]
    );
    let draft = tools
        .iter()
        .find(|t| t.name == "create_draft_order")
        .ok_or("missing write tool")?;
    let hints = draft.annotations.as_ref().ok_or("missing annotations")?;
    assert_eq!(hints.read_only_hint, Some(false));

    // Exact match, with keys in a different order from the fixture.
    let stock = call(
        &client,
        "get_stock_levels",
        json!({ "date": "2026-07-13", "supplier_id": "supplier-x" }),
    )
    .await?;
    assert_eq!(stock.is_error, Some(false));
    let structured = stock.structured_content.as_ref().ok_or("no structured")?;
    assert_eq!(structured["products"][1]["sku"], "WATER-500-24");
    assert_eq!(
        serde_json::from_str::<Value>(&text(&stock))?,
        *structured,
        "text content mirrors structured content"
    );

    // Recorded error result for an unknown supplier.
    let unknown = call(
        &client,
        "get_stock_levels",
        json!({ "supplier_id": "supplier-unknown", "date": "2026-07-13" }),
    )
    .await?;
    assert_eq!(unknown.is_error, Some(true));
    assert_eq!(error_code(&unknown).as_deref(), Some("unknown_supplier"));

    // "*" fallback: a date outside the fixture range is reported as missing data.
    let missing = call(
        &client,
        "get_sales_history",
        json!({ "supplier_id": "supplier-x", "from": "2026-08-01", "to": "2026-08-07" }),
    )
    .await?;
    assert_eq!(missing.is_error, Some(true));
    assert_eq!(error_code(&missing).as_deref(), Some("no_data"));

    // Write tool: draft only.
    let order = call(
        &client,
        "create_draft_order",
        json!({
            "supplier_id": "supplier-x",
            "delivery_date": "2026-07-15",
            "lines": [{ "sku": "WATER-500-24", "quantity": 20, "rationale": "Hot week ahead." }]
        }),
    )
    .await?;
    assert_eq!(order.is_error, Some(false));
    assert_eq!(
        order.structured_content,
        Some(json!({ "draft_id": "draft-0001", "status": "draft", "submitted": false }))
    );

    // Unknown tool: JSON-RPC invalid params, not a tool result.
    match call(&client, "delete_everything", json!({})).await {
        Err(ServiceError::McpError(error)) => assert_eq!(error.code.0, -32602),
        other => return Err(format!("expected protocol error, got {other:?}").into()),
    }

    client.cancel().await?;

    let log = read_log(&log_path)?;
    let summary: Vec<(u64, &str, &str, bool)> = log
        .iter()
        .map(|r| {
            (
                r["seq"].as_u64().unwrap_or(0),
                r["tool"].as_str().unwrap_or(""),
                r["match"].as_str().unwrap_or(""),
                r["matched"].as_bool().unwrap_or(false),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (1, "get_stock_levels", "exact", true),
            (2, "get_stock_levels", "exact", true),
            (3, "get_sales_history", "fallback", true),
            (4, "create_draft_order", "fallback", true),
            (5, "delete_everything", "unknown_tool", false),
        ]
    );
    assert_eq!(
        Value::Object(log[0]["arguments"].as_object().cloned().ok_or("args")?),
        json!({ "date": "2026-07-13", "supplier_id": "supplier-x" })
    );
    Ok(())
}

#[tokio::test]
async fn unrecorded_call_is_an_error_result_and_logged_unmatched() -> TestResult {
    let dir = test_dir("unrecorded")?;
    let fixture = dir.join("fixture.json");
    std::fs::write(
        &fixture,
        json!({
            "server": { "name": "test", "version": "1" },
            "tools": [{ "name": "lookup", "inputSchema": { "type": "object" } }],
            "calls": [{ "tool": "lookup", "arguments": { "id": 1 }, "result": { "content": [] } }]
        })
        .to_string(),
    )?;
    let log_path = dir.join("calls.jsonl");
    let client = connect(&fixture, &log_path).await?;

    let result = call(&client, "lookup", json!({ "id": 2, "extra": true })).await?;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        text(&result),
        r#"no fixture recorded for tool "lookup" with arguments {"extra":true,"id":2}"#
    );
    client.cancel().await?;

    let log = read_log(&log_path)?;
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["match"], "unrecorded");
    assert_eq!(log[0]["matched"], false);
    Ok(())
}

#[tokio::test]
async fn applies_delay_and_tool_error_faults() -> TestResult {
    const DELAY: Duration = Duration::from_millis(300);
    let dir = test_dir("faults")?;
    let fixture = dir.join("fixture.json");
    std::fs::write(
        &fixture,
        json!({
            "server": { "name": "test", "version": "1" },
            "tools": [
                { "name": "slow", "inputSchema": { "type": "object" } },
                { "name": "broken", "inputSchema": { "type": "object" } }
            ],
            "calls": [
                { "tool": "slow", "arguments": {},
                  "result": { "content": [{ "type": "text", "text": "done" }] },
                  "fault": { "delay_ms": DELAY.as_millis() as u64 } },
                { "tool": "broken", "arguments": "*", "fault": { "tool_error": "upstream failed" } }
            ]
        })
        .to_string(),
    )?;
    let log_path = dir.join("calls.jsonl");
    let client = connect(&fixture, &log_path).await?;

    let started = Instant::now();
    let slow = call(&client, "slow", json!({})).await?;
    assert!(
        started.elapsed() >= DELAY,
        "response arrived before the delay"
    );
    assert_eq!(text(&slow), "done");

    let broken = call(&client, "broken", json!({ "any": "thing" })).await?;
    assert_eq!(broken.is_error, Some(true));
    assert_eq!(text(&broken), "upstream failed");
    client.cancel().await?;

    let log = read_log(&log_path)?;
    let matches: Vec<&str> = log.iter().filter_map(|r| r["match"].as_str()).collect();
    assert_eq!(matches, ["exact", "fallback"]);
    Ok(())
}

#[tokio::test]
async fn invalid_fixture_fails_at_start() -> TestResult {
    let dir = test_dir("invalid")?;
    let fixture = dir.join("fixture.json");
    std::fs::write(
        &fixture,
        json!({
            "server": { "name": "test", "version": "1" },
            "tools": [],
            "calls": [{ "tool": "missing", "arguments": {}, "result": { "content": [] } }]
        })
        .to_string(),
    )?;
    let output = Command::new(SERVER)
        .arg("--fixture")
        .arg(&fixture)
        .env_clear()
        .output()
        .await?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout is reserved for MCP");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("not in tools"), "stderr: {stderr}");
    Ok(())
}
