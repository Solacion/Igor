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

/// A recorded response, with its fault already applied.
#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    /// Result returned to the client.
    pub result: CallToolResult,
    /// Delay before the result is returned (`delay_ms` fault).
    pub delay: Option<Duration>,
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
    pub server_name: String,
    /// Server version reported in `initialize`.
    pub server_version: String,
    /// Tool definitions returned verbatim by `tools/list`.
    pub tools: Vec<Tool>,
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

/// Faults supported by this version. Unknown fault names are rejected at load time.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Fault {
    delay_ms: Option<u64>,
    tool_error: Option<String>,
}

impl Fixture {
    /// Reads and validates a fixture file.
    pub fn load(path: &Path) -> Result<Self, FixtureError> {
        let display = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| FixtureError::Read {
            path: display.clone(),
            source,
        })?;
        let file: FixtureFile =
            serde_json::from_str(&text).map_err(|source| FixtureError::Parse {
                path: display,
                source,
            })?;
        Self::from_file(file)
    }

    /// Parses and validates fixture JSON text.
    pub fn from_json(text: &str) -> Result<Self, FixtureError> {
        let file: FixtureFile =
            serde_json::from_str(text).map_err(|source| FixtureError::Parse {
                path: "<inline>".into(),
                source,
            })?;
        Self::from_file(file)
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
    let result = match (result, fault.tool_error) {
        (Some(result), None) => result,
        (None, Some(message)) => CallToolResult::error(vec![ContentBlock::text(message)]),
        (Some(_), Some(_)) => {
            return Err(FixtureError::Invalid(format!(
                "calls[{index}] has both result and fault.tool_error; use one"
            )));
        }
        (None, None) => {
            return Err(FixtureError::Invalid(format!(
                "calls[{index}] needs a result or fault.tool_error"
            )));
        }
    };
    Ok(Recorded {
        result,
        delay: fault.delay_ms.map(Duration::from_millis),
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
        assert_eq!(other.recorded.and_then(|r| r.result.is_error), Some(true));
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
        ];
        for calls in cases {
            assert!(fixture(calls.clone()).is_err(), "should reject {calls}");
        }
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
