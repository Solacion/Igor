//! Contract check: every fixture file in `fixtures/` matches its contract in
//! `docs/specs/contracts/<name>.md`.
//!
//! The tool definitions must equal the contract's, recorded arguments must satisfy the input
//! schema, success results must satisfy the output schema and repeat `structuredContent` as their
//! text block, and error results must use a code from the tool's error table.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A tool as specified in a contract document.
struct ContractTool {
    definition: Value,
    error_codes: BTreeSet<String>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Fixture files with their contract documents, matched by file name.
fn fixture_files() -> Result<Vec<(PathBuf, PathBuf)>, Box<dyn std::error::Error>> {
    let mut pairs = Vec::new();
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("fixture file name is not UTF-8")?;
        let contract = workspace_root()
            .join("docs/specs/contracts")
            .join(format!("{stem}.md"));
        pairs.push((path, contract));
    }
    pairs.sort();
    Ok(pairs)
}

/// Reads the tools of a contract: each `` ## `name` `` section's first JSON block is the tool
/// definition, and the backticked codes in its `### Errors` table are the error codes.
fn parse_contract(text: &str) -> Result<BTreeMap<String, ContractTool>, String> {
    let mut tools = BTreeMap::new();
    for section in text.split("\n## `").skip(1) {
        let name = section.split('`').next().unwrap_or_default().to_owned();
        let json = section
            .split("```json\n")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .ok_or_else(|| format!("{name}: no JSON tool definition"))?;
        let definition: Value =
            serde_json::from_str(json).map_err(|e| format!("{name}: definition: {e}"))?;
        if definition["name"] != name.as_str() {
            return Err(format!("{name}: heading and definition name differ"));
        }
        let errors = section
            .split("### Errors")
            .nth(1)
            .ok_or_else(|| format!("{name}: no error table"))?
            .split("\n#")
            .next()
            .unwrap_or_default();
        let error_codes = errors
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|rest| rest.split('`').next())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if error_codes.is_empty() {
            return Err(format!("{name}: empty error table"));
        }
        tools.insert(
            name,
            ContractTool {
                definition,
                error_codes,
            },
        );
    }
    Ok(tools)
}

fn single_text(result: &Value) -> Result<Value, String> {
    match result["content"].as_array().map(Vec::as_slice) {
        Some([block]) if block["type"] == "text" => block["text"]
            .as_str()
            .ok_or_else(|| "text block has no text".to_owned())
            .and_then(|text| {
                serde_json::from_str(text).map_err(|e| format!("text is not JSON: {e}"))
            }),
        _ => Err("content must be exactly one text block".to_owned()),
    }
}

/// Problems with one recorded result.
fn check_result(tool: &ContractTool, result: &Value) -> Result<(), String> {
    let text = single_text(result)?;
    if result["isError"] == true {
        if result.get("structuredContent").is_some() {
            return Err("error result has structuredContent".to_owned());
        }
        let code = text["error"]["code"]
            .as_str()
            .ok_or("error text has no error.code")?;
        if !text["error"]["message"].is_string() {
            return Err("error text has no error.message".to_owned());
        }
        if !tool.error_codes.contains(code) {
            return Err(format!("error code {code:?} is not in the contract"));
        }
        return Ok(());
    }
    if result["isError"] != false {
        return Err("isError must be true or false".to_owned());
    }
    let structured = &result["structuredContent"];
    let validator = jsonschema::validator_for(&tool.definition["outputSchema"])
        .map_err(|e| format!("outputSchema: {e}"))?;
    if let Some(error) = validator.iter_errors(structured).next() {
        return Err(format!(
            "structuredContent: {error} at {}",
            error.instance_path()
        ));
    }
    if &text != structured {
        return Err("text block differs from structuredContent".to_owned());
    }
    Ok(())
}

/// Checks one fixture file against its contract, returning every problem found.
fn check_fixture(fixture: &Value, contract: &BTreeMap<String, ContractTool>) -> Vec<String> {
    let mut problems = Vec::new();
    let tools = fixture["tools"].as_array().cloned().unwrap_or_default();
    let listed: BTreeSet<_> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    let specified: BTreeSet<_> = contract.keys().map(String::as_str).collect();
    if listed != specified {
        problems.push(format!(
            "tools {listed:?} differ from contract {specified:?}"
        ));
    }
    for definition in &tools {
        let name = definition["name"].as_str().unwrap_or_default();
        if let Some(tool) = contract.get(name)
            && &tool.definition != definition
        {
            problems.push(format!("{name}: definition differs from the contract"));
        }
    }
    let calls = fixture["calls"].as_array().cloned().unwrap_or_default();
    for (index, call) in calls.iter().enumerate() {
        let name = call["tool"].as_str().unwrap_or_default();
        let at = format!("calls[{index}] ({name})");
        let Some(tool) = contract.get(name) else {
            problems.push(format!("{at}: tool not in the contract"));
            continue;
        };
        if call["arguments"].is_object() {
            match jsonschema::validator_for(&tool.definition["inputSchema"]) {
                Ok(validator) => {
                    if let Some(error) = validator.iter_errors(&call["arguments"]).next() {
                        problems.push(format!("{at}: arguments: {error}"));
                    }
                }
                Err(error) => problems.push(format!("{name}: inputSchema: {error}")),
            }
        } else if call["arguments"] != "*" {
            problems.push(format!("{at}: arguments must be an object or \"*\""));
        }
        if let Some(result) = call.get("result")
            && let Err(problem) = check_result(tool, result)
        {
            problems.push(format!("{at}: {problem}"));
        }
    }
    problems
}

#[test]
fn every_fixture_matches_its_contract() -> TestResult {
    let pairs = fixture_files()?;
    let names: Vec<_> = pairs
        .iter()
        .filter_map(|(path, _)| path.file_name()?.to_str())
        .collect();
    assert_eq!(names, ["sales.json", "weather.json"]);
    for (fixture_path, contract_path) in pairs {
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&fixture_path)?)?;
        let contract = parse_contract(&std::fs::read_to_string(&contract_path)?)?;
        let problems = check_fixture(&fixture, &contract);
        assert!(
            problems.is_empty(),
            "{} does not match {}:\n{}",
            fixture_path.display(),
            contract_path.display(),
            problems.join("\n")
        );
    }
    Ok(())
}

#[test]
fn check_reports_contract_violations() -> TestResult {
    let contract = parse_contract(&std::fs::read_to_string(
        workspace_root().join("docs/specs/contracts/weather.md"),
    )?)?;
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/weather.json"),
    )?)?;
    let mut broken = fixture.clone();
    broken["tools"][0]["description"] = "changed".into();
    broken["calls"][0]["arguments"]["location"] = "Not A Slug".into();
    broken["calls"][0]["result"]["structuredContent"]["units"]["temperature"] = "kelvin".into();
    broken["calls"][4]["result"]["content"][0]["text"] =
        r#"{"error":{"code":"made_up","message":"x"}}"#.into();
    let problems = check_fixture(&broken, &contract);
    for expected in [
        "definition differs",
        "calls[0] (get_forecast): arguments",
        "calls[0] (get_forecast): structuredContent",
        "\"made_up\" is not in the contract",
    ] {
        assert!(
            problems.iter().any(|p| p.contains(expected)),
            "missing {expected:?} in {problems:#?}"
        );
    }
    Ok(())
}

#[test]
fn weather_forecasts_list_every_day_of_their_period() -> TestResult {
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/weather.json"),
    )?)?;
    for call in fixture["calls"].as_array().ok_or("no calls")? {
        let forecast = &call["result"]["structuredContent"];
        let Some(days) = forecast["days"].as_array() else {
            continue;
        };
        let dates: Vec<_> = days.iter().filter_map(|d| d["date"].as_str()).collect();
        assert_eq!(dates.first().copied(), forecast["from"].as_str(), "{call}");
        assert_eq!(dates.last().copied(), forecast["to"].as_str(), "{call}");
        assert_eq!(forecast["from"], call["arguments"]["from"], "{call}");
        assert_eq!(forecast["to"], call["arguments"]["to"], "{call}");
        assert!(dates.windows(2).all(|w| w[0] < w[1]), "{call}");
    }
    Ok(())
}
