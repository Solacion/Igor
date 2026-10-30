//! End-to-end runs of the `igor-cli` binary: scripted model, real `igor-fixture-server` over
//! stdio, terminal approvals on stdin. The fixture server's call log is the oracle for what
//! reached the server.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds the fixture server binary once and returns its path.
fn fixture_server() -> Result<PathBuf, String> {
    static BIN: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BIN.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
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

struct Run {
    dir: PathBuf,
    output: Output,
    stdout: String,
    calls: Vec<Value>,
}

impl Run {
    fn logged_tools(&self) -> Vec<&str> {
        self.calls
            .iter()
            .filter_map(|record| record["tool"].as_str())
            .collect()
    }
}

/// Runs `igor-cli` with the sales fixture server, `script` and `stdin`, in a fresh directory.
fn run(name: &str, script: &Value, stdin: &str) -> Result<Run, Box<dyn std::error::Error>> {
    run_with(name, script, stdin, &[])
}

/// As [`run`], with `extra` arguments before the request. Relative paths in `extra` resolve in
/// the run's directory.
fn run_with(
    name: &str,
    script: &Value,
    stdin: &str,
    extra: &[&str],
) -> Result<Run, Box<dyn std::error::Error>> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("igor-cli")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let call_log = dir.join("calls.jsonl");
    let fixture = workspace_root().join("crates/igor-fixture-server/fixtures/sales.json");
    let quote = |path: &Path| serde_json::to_string(&path.display().to_string());
    let config = format!(
        r#"[[servers]]
id = "sales"
transport = "stdio"
command = {command}
args = ["--fixture", {fixture}, "--call-log", {call_log}]

[permissions.sales]
get_stock_levels = "read"
get_sales_history = "read"
create_draft_order = "write"
"#,
        command = quote(&fixture_server()?)?,
        fixture = quote(&fixture)?,
        call_log = quote(&call_log)?,
    );
    let config_path = dir.join("igor.toml");
    std::fs::write(&config_path, config)?;
    let script_path = dir.join("script.json");
    std::fs::write(&script_path, script.to_string())?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_igor-cli"))
        .arg("--config")
        .arg(&config_path)
        .args(["--provider", "scripted", "--script"])
        .arg(&script_path)
        .args(extra)
        .current_dir(&dir)
        .arg("Prepare a draft order for supplier-x for delivery on 2026-07-15.")
        .env_remove("IGOR_CONFIG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(stdin.as_bytes())?;
    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let calls = std::fs::read_to_string(&call_log)
        .unwrap_or_default()
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("script did not match"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    Ok(Run {
        dir,
        output,
        stdout,
        calls,
    })
}

/// The documented example script: reads stock and sales, proposes a draft order, then answers.
fn order_script() -> Result<Value, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(workspace_root().join("examples/order-script.json"))?;
    Ok(serde_json::from_str(&text)?)
}

#[test]
fn approved_draft_order_reaches_the_server_once() -> TestResult {
    let run = run("approve", &order_script()?, "y\n")?;
    assert!(run.output.status.success(), "{}", run.stdout);
    assert_eq!(
        run.logged_tools(),
        [
            "get_stock_levels",
            "get_sales_history",
            "create_draft_order"
        ]
    );
    assert_eq!(run.calls[2]["arguments"]["lines"][0]["quantity"], json!(12));
    // Every read matched an exact recording: the example script stays inside the fixture data.
    // Any draft order is accepted, so the write matches the fallback.
    for call in &run.calls {
        let expected = if call["tool"] == "create_draft_order" {
            "fallback"
        } else {
            "exact"
        };
        assert_eq!(call["match"], json!(expected), "{call}");
    }
    assert!(
        run.stdout.contains("Approval required [approval-1]"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("tool:        sales/create_draft_order"));
    assert!(run.stdout.contains("action hash: "));
    assert!(run.stdout.contains("[approval-1] approved"));
    assert!(run.stdout.contains("[completed]"));
    Ok(())
}

#[test]
fn export_records_every_event_without_business_data() -> TestResult {
    let run = run_with(
        "export",
        &order_script()?,
        "y\n",
        &["--export", "activity.jsonl"],
    )?;
    assert!(run.output.status.success(), "{}", run.stdout);
    let export = std::fs::read_to_string(run.dir.join("activity.jsonl"))?;
    let events = export
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    let sequence: Vec<_> = events.iter().map(|e| e["sequence"].clone()).collect();
    let expected: Vec<_> = (0..events.len()).map(|n| json!(n)).collect();
    assert_eq!(sequence, expected);
    let kinds: Vec<_> = events
        .iter()
        .filter_map(|e| e["event"]["event"].as_str())
        .collect();
    for kind in [
        "tool_call_proposed",
        "approval_requested",
        "approval_resolved",
    ] {
        assert!(kinds.contains(&kind), "{kinds:?}");
    }
    assert_eq!(kinds.last(), Some(&"turn_finished"));
    for secret in ["supplier-x", "COLA-330", "Hot week ahead", "Checking stock"] {
        assert!(!export.contains(secret), "{secret} leaked:\n{export}");
    }
    assert!(export.contains("create_draft_order"));
    Ok(())
}

#[test]
fn denied_draft_order_never_reaches_the_server() -> TestResult {
    let run = run("deny", &order_script()?, "n\n")?;
    assert!(run.output.status.success(), "{}", run.stdout);
    assert_eq!(
        run.logged_tools(),
        ["get_stock_levels", "get_sales_history"]
    );
    assert!(
        run.stdout.contains("[approval-1] denied, not executed"),
        "{}",
        run.stdout
    );
    Ok(())
}

#[test]
fn end_of_input_at_the_approval_prompt_denies() -> TestResult {
    let run = run("eof", &order_script()?, "")?;
    assert!(run.output.status.success(), "{}", run.stdout);
    assert!(!run.logged_tools().contains(&"create_draft_order"));
    Ok(())
}

#[test]
fn clarification_answer_is_used_in_the_next_tool_call() -> TestResult {
    let script = json!({ "turns": [
        { "tool_calls": [
            { "id": "q1", "name": "igor_ask_operator",
              "input": { "question": "Which stock date should I use?" } }
        ] },
        { "tool_calls": [
            { "id": "t1", "name": "sales__get_stock_levels",
              "input": { "supplier_id": "supplier-x", "date": "2026-07-13" } }
        ] },
        { "text": "Stock retrieved for the date you gave." }
    ] });
    let run = run("clarify", &script, "2026-07-13\n")?;
    assert!(run.output.status.success(), "{}", run.stdout);
    assert!(
        run.stdout
            .contains("IGOR asks: Which stock date should I use?")
    );
    assert_eq!(run.logged_tools(), ["get_stock_levels"]);
    assert_eq!(run.calls[0]["arguments"]["date"], json!("2026-07-13"));
    Ok(())
}

#[test]
fn unanswered_clarification_cancels_with_exit_code_4() -> TestResult {
    let script = json!({ "turns": [
        { "tool_calls": [
            { "id": "q1", "name": "igor_ask_operator", "input": { "question": "Which supplier?" } }
        ] }
    ] });
    let run = run("clarify-eof", &script, "")?;
    assert_eq!(run.output.status.code(), Some(4), "{}", run.stdout);
    assert!(run.calls.is_empty());
    Ok(())
}
