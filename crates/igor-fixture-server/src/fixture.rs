//! Fixture file loading, validation and argument matching.
//!
//! The file format is documented in `docs/specs/contracts/fixture-format.md`.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock, Tool};
use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::canonical::canonical_object;

/// Literal that marks a per-tool fallback entry in place of an arguments object.
pub(crate) const WILDCARD: &str = "*";

/// A fixture file failed to load or is inconsistent.
#[derive(Debug, Error)]
pub enum FixtureError {
    /// The file could not be read.
    #[error("cannot read fixture file {path}: {source}")]
    Read {
        /// Path that was read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file is not valid fixture JSON.
    #[error("invalid fixture file {path}: {source}")]
    Parse {
        /// Path that was parsed.
        path: String,
        /// Underlying parse error.
        source: serde_json::Error,
    },
    /// The file parsed but its content is inconsistent.
    #[error("invalid fixture: {0}")]
    Invalid(String),
}

/// How a call was matched against the recorded entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    /// The canonical arguments equal a recorded entry.
    Exact,
    /// No exact entry matched; the tool's `"*"` fallback entry was used.
    Fallback,
    /// The tool exists but no entry matched.
    Unrecorded,
    /// The tool is not in the fixture's tool list.
    UnknownTool,
}

impl MatchKind {
    /// Name written to the call log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Fallback => "fallback",
            Self::Unrecorded => "unrecorded",
            Self::UnknownTool => "unknown_tool",
        }
    }

    /// True when a recorded entry answered the call.
    pub fn is_matched(self) -> bool {
        matches!(self, Self::Exact | Self::Fallback)
    }
}

/// Largest `oversized_result` a fixture may request, in bytes (64 MiB).
pub(crate) const MAX_OVERSIZED_RESULT_BYTES: usize = 64 * 1024 * 1024;

/// What the server does for a matched call.
#[derive(Debug, Clone)]
pub(crate) enum Response {
    /// Return this `tools/call` result.
    Result(CallToolResult),
    /// Never respond (`hang` fault).
    Hang,
    /// Exit the process before the call is logged (`crash_before_response` fault).
    CrashBeforeLog,
    /// Exit the process without responding (`crash_after_record` fault).
    CrashAfterRecord,
    /// Write a line that is not JSON-RPC instead of a response, then never respond
    /// (`malformed_response` fault).
    Malformed,
}

/// A recorded response, with its fault already applied.
#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    /// What to send back.
    pub response: Response,
    /// Delay before the response (`delay_ms` fault).
    pub delay: Option<Duration>,
    /// Send `notifications/tools/list_changed` before the response (`notify_tools_changed`
    /// fault).
    pub notify_tools_changed: bool,
}

/// Outcome of looking up a call.
#[derive(Debug)]
pub(crate) struct Lookup<'a> {
    /// How the call matched.
    pub kind: MatchKind,
    /// Canonical JSON text of the call's arguments.
    pub canonical_arguments: String,
    /// The recorded response, when one matched.
    pub recorded: Option<&'a Recorded>,
}

#[derive(Debug, Default)]
struct ToolEntries {
    exact: HashMap<String, Recorded>,
    fallback: Option<Recorded>,
}

/// A loaded, validated fixture set for one server.
#[derive(Debug)]
pub struct Fixture {
    /// Server name reported in `initialize`.
    pub(crate) server_name: String,
    /// Server version reported in `initialize`.
    pub(crate) server_version: String,
    /// Tool definitions returned verbatim by `tools/list`.
    pub(crate) tools: Vec<Tool>,
    entries: HashMap<String, ToolEntries>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    server: ServerSection,
    tools: Vec<Tool>,
    #[serde(default)]
    calls: Vec<CallEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerSection {
    name: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CallEntry {
    tool: String,
    arguments: Arguments,
    #[serde(default)]
    result: Option<CallToolResult>,
    #[serde(default)]
    fault: Fault,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Arguments {
    Exact(Map<String, Value>),
    Wildcard(String),
}

/// Supported faults. Unknown fault names are rejected at load time.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Fault {
    delay_ms: Option<u64>,
    #[serde(default)]
    notify_tools_changed: bool,
    tool_error: Option<String>,
    oversized_result: Option<usize>,
    #[serde(default)]
    hang: bool,
    #[serde(default)]
    crash_before_response: bool,
    #[serde(default)]
    crash_after_record: bool,
    #[serde(default)]
    malformed_response: bool,
}

impl Fixture {
    /// Reads and validates a fixture file.
    pub fn load(path: &Path) -> Result<Self, FixtureError> {
        let display = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| FixtureError::Read {
            path: display.clone(),
            source,
        })?;
        Self::parse(&text, display)
    }

    /// Parses and validates fixture JSON text; `path` labels parse errors.
    fn parse(text: &str, path: String) -> Result<Self, FixtureError> {
        let file: FixtureFile =
            serde_json::from_str(text).map_err(|source| FixtureError::Parse { path, source })?;
        Self::from_file(file)
    }

    /// Parses inline fixture JSON in unit tests.
    #[cfg(test)]
    fn from_json(text: &str) -> Result<Self, FixtureError> {
        Self::parse(text, "<inline>".into())
    }

    fn from_file(file: FixtureFile) -> Result<Self, FixtureError> {
        let mut entries: HashMap<String, ToolEntries> = HashMap::new();
        for tool in &file.tools {
            if entries
                .insert(tool.name.to_string(), ToolEntries::default())
                .is_some()
            {
                return Err(FixtureError::Invalid(format!(
                    "tool {:?} is listed more than once",
                    tool.name
                )));
            }
        }
        for (index, call) in file.calls.into_iter().enumerate() {
            let tool_entries = entries.get_mut(&call.tool).ok_or_else(|| {
                FixtureError::Invalid(format!(
                    "calls[{index}] refers to tool {:?}, which is not in tools",
                    call.tool
                ))
            })?;
            let recorded = recorded_response(index, call.result, call.fault)?;
            match call.arguments {
                Arguments::Exact(map) => {
                    let key = canonical_object(&map);
                    if tool_entries.exact.insert(key.clone(), recorded).is_some() {
                        return Err(FixtureError::Invalid(format!(
                            "calls[{index}] duplicates arguments {key} for tool {:?}",
                            call.tool
                        )));
                    }
                }
                Arguments::Wildcard(marker) if marker == WILDCARD => {
                    if tool_entries.fallback.replace(recorded).is_some() {
                        return Err(FixtureError::Invalid(format!(
                            "calls[{index}] is a second \"*\" entry for tool {:?}",
                            call.tool
                        )));
                    }
                }
                Arguments::Wildcard(other) => {
                    return Err(FixtureError::Invalid(format!(
                        "calls[{index}].arguments must be an object or \"*\", found {other:?}"
                    )));
                }
            }
        }
        Ok(Self {
            server_name: file.server.name,
            server_version: file.server.version,
            tools: file.tools,
            entries,
        })
    }

    /// Finds the recorded response for a call. A missing `arguments` object is treated as `{}`.
    pub(crate) fn lookup(&self, tool: &str, arguments: Option<&Map<String, Value>>) -> Lookup<'_> {
        let canonical_arguments = match arguments {
            Some(map) => canonical_object(map),
            None => canonical_object(&Map::new()),
        };
        let Some(tool_entries) = self.entries.get(tool) else {
            return Lookup {
                kind: MatchKind::UnknownTool,
                canonical_arguments,
                recorded: None,
            };
        };
        let (kind, recorded) = match tool_entries.exact.get(&canonical_arguments) {
            Some(recorded) => (MatchKind::Exact, Some(recorded)),
            None => match &tool_entries.fallback {
                Some(recorded) => (MatchKind::Fallback, Some(recorded)),
                None => (MatchKind::Unrecorded, None),
            },
        };
        Lookup {
            kind,
            canonical_arguments,
            recorded,
        }
    }
}

fn recorded_response(
    index: usize,
    result: Option<CallToolResult>,
    fault: Fault,
) -> Result<Recorded, FixtureError> {
    let mut responses = Vec::new();
    if let Some(result) = result {
        responses.push(Response::Result(result));
    }
    if let Some(message) = fault.tool_error {
        responses.push(Response::Result(CallToolResult::error(vec![
            ContentBlock::text(message),
        ])));
    }
    if let Some(bytes) = fault.oversized_result {
        if bytes > MAX_OVERSIZED_RESULT_BYTES {
            return Err(FixtureError::Invalid(format!(
                "calls[{index}].fault.oversized_result exceeds {MAX_OVERSIZED_RESULT_BYTES} bytes"
            )));
        }
        responses.push(Response::Result(CallToolResult::success(vec![
            ContentBlock::text("x".repeat(bytes)),
        ])));
    }
    if fault.hang {
        responses.push(Response::Hang);
    }
    if fault.crash_before_response {
        responses.push(Response::CrashBeforeLog);
    }
    if fault.crash_after_record {
        responses.push(Response::CrashAfterRecord);
    }
    if fault.malformed_response {
        responses.push(Response::Malformed);
    }
    let response = match <[Response; 1]>::try_from(responses) {
        Ok([response]) => response,
        Err(_) => {
            return Err(FixtureError::Invalid(format!(
                "calls[{index}] needs exactly one of result, fault.tool_error, \
fault.oversized_result, fault.hang, fault.crash_before_response, fault.crash_after_record \
or fault.malformed_response"
            )));
        }
    };
    Ok(Recorded {
        response,
        delay: fault.delay_ms.map(Duration::from_millis),
        notify_tools_changed: fault.notify_tools_changed,
    })
}

/// Text of the `isError` result returned for an unrecorded call.
pub(crate) fn unrecorded_message(tool: &str, canonical_arguments: &str) -> String {
    format!("no fixture recorded for tool {tool:?} with arguments {canonical_arguments}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fixture(calls: Value) -> Result<Fixture, FixtureError> {
        let file = json!({
            "server": { "name": "test", "version": "1" },
            "tools": [
                { "name": "lookup", "inputSchema": { "type": "object" } },
                { "name": "empty", "inputSchema": { "type": "object" } }
            ],
            "calls": calls
        });
        Fixture::from_json(&file.to_string())
    }

    fn args(value: Value) -> Option<Map<String, Value>> {
        value.as_object().cloned()
    }

    #[test]
    fn matches_exact_arguments_regardless_of_key_order() -> Result<(), FixtureError> {
        let f = fixture(json!([{
            "tool": "lookup",
            "arguments": { "a": 1, "b": "x" },
            "result": { "content": [{ "type": "text", "text": "hit" }] }
        }]))?;
        let found = f.lookup("lookup", args(json!({ "b": "x", "a": 1.0 })).as_ref());
        assert_eq!(found.kind, MatchKind::Exact);
        assert!(found.recorded.is_some());
        Ok(())
    }

    #[test]
    fn extra_or_missing_keys_do_not_match() -> Result<(), FixtureError> {
        let f = fixture(json!([{
            "tool": "lookup",
            "arguments": { "a": 1 },
            "result": { "content": [] }
        }]))?;
        let extra = f.lookup("lookup", args(json!({ "a": 1, "b": 2 })).as_ref());
        assert_eq!(extra.kind, MatchKind::Unrecorded);
        let missing = f.lookup("lookup", None);
        assert_eq!(missing.kind, MatchKind::Unrecorded);
        assert_eq!(missing.canonical_arguments, "{}");
        Ok(())
    }

    #[test]
    fn uses_fallback_only_without_exact_match() -> Result<(), FixtureError> {
        let f = fixture(json!([
            { "tool": "lookup", "arguments": { "a": 1 }, "result": { "content": [] } },
            { "tool": "lookup", "arguments": "*", "fault": { "tool_error": "no data" } }
        ]))?;
        assert_eq!(
            f.lookup("lookup", args(json!({ "a": 1 })).as_ref()).kind,
            MatchKind::Exact
        );
        let other = f.lookup("lookup", args(json!({ "a": 2 })).as_ref());
        assert_eq!(other.kind, MatchKind::Fallback);
        assert!(matches!(
            other.recorded.map(|r| &r.response),
            Some(Response::Result(result)) if result.is_error == Some(true)
        ));
        Ok(())
    }

    #[test]
    fn distinguishes_unknown_tool_from_unrecorded_call() -> Result<(), FixtureError> {
        let f = fixture(json!([]))?;
        assert_eq!(f.lookup("empty", None).kind, MatchKind::Unrecorded);
        assert_eq!(f.lookup("missing", None).kind, MatchKind::UnknownTool);
        Ok(())
    }

    #[test]
    fn rejects_inconsistent_fixtures() {
        let cases = [
            json!([{ "tool": "nope", "arguments": {}, "result": { "content": [] } }]),
            json!([{ "tool": "lookup", "arguments": "any", "result": { "content": [] } }]),
            json!([{ "tool": "lookup", "arguments": {} }]),
            json!([{ "tool": "lookup", "arguments": {}, "result": { "content": [] },
                     "fault": { "tool_error": "x" } }]),
            json!([{ "tool": "lookup", "arguments": {}, "result": { "content": [] },
                     "fault": { "hang": true } }]),
            json!([
                { "tool": "lookup", "arguments": { "a": 1 }, "result": { "content": [] } },
                { "tool": "lookup", "arguments": { "a": 1.0 }, "result": { "content": [] } }
            ]),
            json!([
                { "tool": "lookup", "arguments": "*", "result": { "content": [] } },
                { "tool": "lookup", "arguments": "*", "result": { "content": [] } }
            ]),
            json!([{ "tool": "lookup", "arguments": {},
                     "fault": { "hang": true, "crash_after_record": true } }]),
            json!([{ "tool": "lookup", "arguments": {}, "fault": { "explode": true } }]),
            json!([{ "tool": "lookup", "arguments": {},
                     "fault": { "oversized_result": MAX_OVERSIZED_RESULT_BYTES + 1 } }]),
        ];
        for calls in cases {
            assert!(fixture(calls.clone()).is_err(), "should reject {calls}");
        }
    }

    #[test]
    fn parses_each_fault() -> Result<(), FixtureError> {
        let f = fixture(json!([
            { "tool": "lookup", "arguments": { "f": "hang" }, "fault": { "hang": true } },
            { "tool": "lookup", "arguments": { "f": "early" }, "fault": { "crash_before_response": true } },
            { "tool": "lookup", "arguments": { "f": "crash" }, "fault": { "crash_after_record": true } },
            { "tool": "lookup", "arguments": { "f": "bad" }, "fault": { "malformed_response": true } },
            { "tool": "lookup", "arguments": { "f": "big" }, "fault": { "oversized_result": 10 } }
        ]))?;
        let response = |name: &str| {
            f.lookup("lookup", args(json!({ "f": name })).as_ref())
                .recorded
                .map(|r| r.response.clone())
        };
        assert!(matches!(response("hang"), Some(Response::Hang)));
        assert!(matches!(response("early"), Some(Response::CrashBeforeLog)));
        assert!(matches!(
            response("crash"),
            Some(Response::CrashAfterRecord)
        ));
        assert!(matches!(response("bad"), Some(Response::Malformed)));
        let Some(Response::Result(big)) = response("big") else {
            return Err(FixtureError::Invalid("oversized_result not parsed".into()));
        };
        assert_eq!(
            serde_json::to_value(&big.content).ok(),
            Some(json!([{ "type": "text", "text": "xxxxxxxxxx" }]))
        );
        Ok(())
    }

    #[test]
    fn records_delay_fault() -> Result<(), FixtureError> {
        let f = fixture(json!([{
            "tool": "lookup",
            "arguments": {},
            "result": { "content": [] },
            "fault": { "delay_ms": 25 }
        }]))?;
        let found = f.lookup("lookup", None);
        assert_eq!(
            found.recorded.and_then(|r| r.delay),
            Some(Duration::from_millis(25))
        );
        Ok(())
    }
}
