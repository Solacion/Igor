//! Safety properties checked across the real MCP boundary: the `igor-cli` binary with a scripted
//! model and real `igor-fixture-server` processes. Each server's call log is the oracle for what
//! reached it, so these tests prove what was executed, not what the core intended.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type BoxResult<T> = Result<T, Box<dyn std::error::Error>>;

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

/// One configured server for a run.
struct Server<'a> {
    id: &'a str,
    /// Fixture file, or `None` for a server whose command does not exist.
    fixture: Option<PathBuf>,
    /// `tool = "class"` lines for `[permissions.<id>]`.
    permissions: &'a [(&'a str, &'a str)],
    call_timeout_ms: Option<u64>,
}

struct Run {
    output: Output,
    stdout: String,
    dir: PathBuf,
}

impl Run {
    /// Fails if a fixture server started for this run is still running after `igor-cli` exited.
    fn assert_no_server_left_running(&self) -> TestResult {
        let output = Command::new("pgrep")
            .arg("-f")
            .arg(self.dir.display().to_string())
            .output()?;
        let pids = String::from_utf8_lossy(&output.stdout);
        assert!(
            pids.trim().is_empty(),
            "server processes left running: {pids}"
        );
        Ok(())
    }

    /// Calls logged by server `id`, as `(tool, arguments)`.
    fn calls(&self, id: &str) -> BoxResult<Vec<(String, Value)>> {
        let path = self.dir.join(format!("{id}.jsonl"));
        let text = std::fs::read_to_string(path).unwrap_or_default();
        text.lines()
            .map(|line| {
                let record: Value = serde_json::from_str(line)?;
                let tool = record["tool"].as_str().unwrap_or_default().to_owned();
                Ok((tool, record["arguments"].clone()))
            })
            .collect()
    }
}

fn run(name: &str, servers: &[Server<'_>], script: &Value, stdin: &str) -> BoxResult<Run> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("igor-cli-boundary")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let quote = |text: String| serde_json::to_string(&text);
    let server_bin = fixture_server()?;
    let mut config = String::new();
    for server in servers {
        let (command, args) = match &server.fixture {
            Some(fixture) => (
                quote(server_bin.display().to_string())?,
                format!(
                    "[\"--fixture\", {}, \"--call-log\", {}]",
                    quote(fixture.display().to_string())?,
                    quote(
                        dir.join(format!("{}.jsonl", server.id))
                            .display()
                            .to_string()
                    )?
                ),
            ),
            None => (
                quote(dir.join("no-such-server").display().to_string())?,
                "[]".to_owned(),
            ),
        };
        config.push_str(&format!(
            "[[servers]]\nid = \"{}\"\ntransport = \"stdio\"\ncommand = {command}\nargs = {args}\n",
            server.id
        ));
        if let Some(ms) = server.call_timeout_ms {
            config.push_str(&format!("call_timeout_ms = {ms}\n"));
        }
        config.push('\n');
    }
    for server in servers {
        config.push_str(&format!("[permissions.{}]\n", server.id));
        for (tool, class) in server.permissions {
            config.push_str(&format!("{tool} = \"{class}\"\n"));
        }
        config.push('\n');
    }
    let config_path = dir.join("igor.toml");
    std::fs::write(&config_path, config)?;
    let script_path = dir.join("script.json");
    std::fs::write(&script_path, script.to_string())?;

    let mut child = Command::new(env!("CARGO_BIN_EXE_igor-cli"))
        .arg("--config")
        .arg(&config_path)
        .args(["--provider", "scripted", "--script"])
        .arg(&script_path)
        .arg("Do the task.")
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("script did not match"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    Ok(Run {
        output,
        stdout,
        dir,
    })
}

fn sales_fixture() -> PathBuf {
    workspace_root().join("crates/igor-fixture-server/fixtures/sales.json")
}

/// Writes a fixture with one `notes` server: a `save_note` write tool with `fault`, and a
/// `list_notes` read tool.
fn faulty_notes(dir_name: &str, fault: Value) -> BoxResult<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("igor-cli-boundary-fixtures")
        .join(dir_name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("notes.json");
    let fixture = json!({
        "server": { "name": "notes", "version": "1" },
        "tools": [
            { "name": "save_note", "inputSchema": { "type": "object" } },
            { "name": "list_notes", "inputSchema": { "type": "object" } }
        ],
        "calls": [
            { "tool": "save_note", "arguments": "*", "fault": fault },
            { "tool": "list_notes", "arguments": "*", "result": { "content": [] } }
        ]
    });
    std::fs::write(&path, fixture.to_string())?;
    Ok(path)
}

const SALES_READ: &[(&str, &str)] = &[("get_stock_levels", "read")];
const SALES_WRITE: &[(&str, &str)] = &[("create_draft_order", "write")];
const NOTES: &[(&str, &str)] = &[("save_note", "write"), ("list_notes", "read")];

fn order(quantity: u32) -> Value {
    json!({
        "supplier_id": "supplier-x",
        "delivery_date": "2026-07-15",
        "lines": [{ "sku": "COLA-330-24", "quantity": quantity, "rationale": "test" }]
    })
}

#[test]
fn same_tool_name_on_two_servers_reaches_only_the_named_server() -> TestResult {
    let servers = [
        Server {
            id: "north",
            fixture: Some(sales_fixture()),
            permissions: SALES_READ,
            call_timeout_ms: None,
        },
        Server {
            id: "south",
            fixture: Some(sales_fixture()),
            permissions: SALES_READ,
            call_timeout_ms: None,
        },
    ];
    let script = json!({ "turns": [
        { "tool_calls": [{ "id": "t1", "name": "south__get_stock_levels",
                           "input": { "supplier_id": "supplier-x", "date": "2026-07-13" } }] },
        { "text": "Done." }
    ] });
    let run = run("collision", &servers, &script, "")?;
    assert!(run.output.status.success(), "{}", run.stdout);
    assert_eq!(run.calls("south")?.len(), 1);
    assert!(run.calls("north")?.is_empty(), "north must not be called");
    run.assert_no_server_left_running()?;
    Ok(())
}

#[test]
fn approval_binds_the_exact_arguments() -> TestResult {
    let servers = [Server {
        id: "sales",
        fixture: Some(sales_fixture()),
        permissions: SALES_WRITE,
        call_timeout_ms: None,
    }];
    let script = json!({ "turns": [
        { "tool_calls": [{ "id": "t1", "name": "sales__create_draft_order", "input": order(12) }] },
        { "tool_calls": [{ "id": "t2", "name": "sales__create_draft_order", "input": order(14) }] },
        { "text": "Done." }
    ] });
    // Deny the first proposal, approve the changed one.
    let run = run("binding", &servers, &script, "n\ny\n")?;
    assert!(run.output.status.success(), "{}", run.stdout);
    let hashes: Vec<_> = run
        .stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("action hash: "))
        .collect();
    assert_eq!(hashes.len(), 2, "{}", run.stdout);
    assert_ne!(
        hashes[0], hashes[1],
        "changed arguments need a fresh approval"
    );
    let calls = run.calls("sales")?;
    assert_eq!(calls.len(), 1, "only the approved proposal runs");
    assert_eq!(calls[0].1["lines"][0]["quantity"], json!(14));
    run.assert_no_server_left_running()?;
    Ok(())
}

#[test]
fn unclassified_and_denied_tools_never_reach_the_server() -> TestResult {
    let servers = [Server {
        id: "sales",
        fixture: Some(sales_fixture()),
        permissions: &[("get_stock_levels", "denied")],
        call_timeout_ms: None,
    }];
    let script = json!({ "turns": [
        { "tool_calls": [
            { "id": "t1", "name": "sales__get_stock_levels",
              "input": { "supplier_id": "supplier-x", "date": "2026-07-13" } },
            { "id": "t2", "name": "sales__create_draft_order", "input": order(12) }
        ] },
        { "text": "Could not do it." }
    ] });
    let run = run("unclassified", &servers, &script, "")?;
    assert_eq!(run.output.status.code(), Some(5), "{}", run.stdout);
    assert!(
        run.stdout.contains("[completed with gaps: "),
        "{}",
        run.stdout
    );
    assert_eq!(run.stdout.matches("refused").count(), 2, "{}", run.stdout);
    assert!(!run.stdout.contains("Approval required"));
    assert!(run.calls("sales")?.is_empty());
    run.assert_no_server_left_running()?;
    Ok(())
}

#[test]
fn arguments_failing_the_input_schema_never_reach_the_server() -> TestResult {
    let servers = [Server {
        id: "sales",
        fixture: Some(sales_fixture()),
        permissions: SALES_READ,
        call_timeout_ms: None,
    }];
    let script = json!({ "turns": [
        { "tool_calls": [
            // Missing the required `date`.
            { "id": "t1", "name": "sales__get_stock_levels", "input": { "supplier_id": "supplier-x" } },
            // Wrong type.
            { "id": "t2", "name": "sales__get_stock_levels",
              "input": { "supplier_id": 7, "date": "2026-07-13" } },
            // Extra property; the schema sets additionalProperties: false.
            { "id": "t3", "name": "sales__get_stock_levels",
              "input": { "supplier_id": "supplier-x", "date": "2026-07-13", "extra": true } },
            // Unknown tool.
            { "id": "t4", "name": "sales__drop_tables", "input": {} }
        ] },
        { "text": "None of those calls were valid." }
    ] });
    let run = run("schema", &servers, &script, "")?;
    assert_eq!(run.output.status.code(), Some(5), "{}", run.stdout);
    assert!(
        run.stdout.contains("[completed with gaps: "),
        "{}",
        run.stdout
    );
    assert_eq!(run.stdout.matches("refused").count(), 4, "{}", run.stdout);
    assert!(
        run.calls("sales")?.is_empty(),
        "invalid calls must not be dispatched"
    );
    run.assert_no_server_left_running()?;
    Ok(())
}

#[test]
fn hung_write_is_outcome_unknown_and_never_retried() -> TestResult {
    let servers = [Server {
        id: "notes",
        fixture: Some(faulty_notes("hang", json!({ "hang": true }))?),
        permissions: NOTES,
        call_timeout_ms: Some(500),
    }];
    let script = json!({ "turns": [
        { "tool_calls": [{ "id": "t1", "name": "notes__save_note", "input": { "text": "hi" } }] },
        { "text": "The save may or may not have happened." }
    ] });
    let run = run("hang", &servers, &script, "y\n")?;
    assert_eq!(run.output.status.code(), Some(5), "{}", run.stdout);
    assert!(
        run.stdout.contains("[completed with gaps: "),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("[call-1] failed, OUTCOME UNKNOWN"),
        "{}",
        run.stdout
    );
    let calls = run.calls("notes")?;
    assert_eq!(
        calls.len(),
        1,
        "a write with unknown outcome must not be retried"
    );
    run.assert_no_server_left_running()?;
    Ok(())
}

#[test]
fn server_crash_after_a_write_is_reported_and_later_calls_are_not_executed() -> TestResult {
    let servers = [Server {
        id: "notes",
        fixture: Some(faulty_notes(
            "crash",
            json!({ "crash_after_record": true }),
        )?),
        permissions: NOTES,
        call_timeout_ms: Some(5_000),
    }];
    let script = json!({ "turns": [
        { "tool_calls": [{ "id": "t1", "name": "notes__save_note", "input": { "text": "hi" } }] },
        { "tool_calls": [{ "id": "t2", "name": "notes__list_notes", "input": {} }] },
        { "text": "The notes server stopped." }
    ] });
    let run = run("crash", &servers, &script, "y\n")?;
    assert_eq!(run.output.status.code(), Some(5), "{}", run.stdout);
    assert!(
        run.stdout.contains("[completed with gaps: "),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("[call-1] failed, OUTCOME UNKNOWN"),
        "{}",
        run.stdout
    );
    assert_eq!(
        run.stdout.matches("[server notes: Failed]").count(),
        1,
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("[call-2] failed, not executed"),
        "{}",
        run.stdout
    );
    let tools: Vec<_> = run
        .calls("notes")?
        .into_iter()
        .map(|(tool, _)| tool)
        .collect();
    assert_eq!(
        tools,
        ["save_note"],
        "the write was recorded once; nothing after the crash"
    );
    run.assert_no_server_left_running()?;
    Ok(())
}

#[test]
fn an_unavailable_server_is_reported_and_the_others_still_work() -> TestResult {
    let servers = [
        Server {
            id: "sales",
            fixture: Some(sales_fixture()),
            permissions: SALES_READ,
            call_timeout_ms: None,
        },
        Server {
            id: "broken",
            fixture: None,
            permissions: &[("anything", "read")],
            call_timeout_ms: None,
        },
    ];
    let script = json!({ "turns": [
        { "tool_calls": [
            { "id": "t1", "name": "broken__anything", "input": {} },
            { "id": "t2", "name": "sales__get_stock_levels",
              "input": { "supplier_id": "supplier-x", "date": "2026-07-13" } }
        ] },
        { "text": "The broken server is unavailable; stock was read from sales." }
    ] });
    let run = run("unavailable", &servers, &script, "")?;
    assert_eq!(run.output.status.code(), Some(5), "{}", run.stdout);
    assert!(
        run.stdout.contains("[completed with gaps: "),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("[server broken: unavailable]"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("[call-1] refused"), "{}", run.stdout);
    assert_eq!(run.calls("sales")?.len(), 1);
    run.assert_no_server_left_running()?;
    Ok(())
}
