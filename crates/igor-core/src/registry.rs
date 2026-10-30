//! Tool registry: host aliases, permission gating and argument validation.
//!
//! The registry is built from the tools discovered on every connected server and the host's
//! classification map. It gives each tool a provider-safe alias, offers only permitted tools to
//! the model, and is the single place where a model's tool call is turned into a call that may
//! be dispatched: [`ToolRegistry::prepare_call`]. A model-supplied name is only ever looked up in
//! the alias table, never parsed.

use std::collections::{HashMap, HashSet};
use std::fmt;

use jsonschema::Validator;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::canonical::{to_hex, write_canonical, write_string};
use crate::policy::Classification;
use crate::provider::ToolSpec;
use crate::tools::{ToolDescriptor, ToolId};

/// Maximum length of a tool alias, accepted by every supported provider.
pub const MAX_ALIAS_LEN: usize = 64;

/// Maximum size of a tool's `inputSchema` in canonical JSON bytes. Larger tools are excluded.
pub const MAX_SCHEMA_BYTES: usize = 64 * 1024;

/// Separates the server and tool parts of an alias.
const ALIAS_SEPARATOR: &str = "__";

/// Hex digits of the tool-identity hash appended to an alias that needs disambiguation.
const ALIAS_HASH_HEX_LEN: usize = 16;

/// Maximum number of schema violations reported for one call.
const MAX_REPORTED_ARGUMENT_ERRORS: usize = 10;

/// SHA-256 over the canonical JSON of a tool's server, name, description and input schema.
///
/// A change to any of them changes the fingerprint, so an approval bound to it does not carry
/// over to a changed tool definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToolFingerprint([u8; 32]);

impl ToolFingerprint {
    /// Raw digest bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ToolFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_hex(&self.0))
    }
}

/// A tool accepted into the registry.
#[derive(Debug)]
pub struct RegisteredTool {
    id: ToolId,
    alias: String,
    description: Option<String>,
    input_schema: Value,
    classification: Classification,
    fingerprint: ToolFingerprint,
    validator: Validator,
}

impl RegisteredTool {
    /// Server-qualified identity.
    pub fn id(&self) -> &ToolId {
        &self.id
    }

    /// Host-generated alias shown to the model.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Server-supplied description. Untrusted text.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Input JSON Schema.
    pub fn input_schema(&self) -> &Value {
        &self.input_schema
    }

    /// Host classification; [`Classification::Unclassified`] when the host gave none.
    pub fn classification(&self) -> Classification {
        self.classification
    }

    /// Tool-definition fingerprint.
    pub fn fingerprint(&self) -> ToolFingerprint {
        self.fingerprint
    }
}

/// Why a discovered tool was not registered.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExclusionReason {
    /// The server reported the same tool name more than once.
    #[error("the server reported this tool more than once")]
    DuplicateTool,
    /// The input schema exceeds [`MAX_SCHEMA_BYTES`].
    #[error("input schema is {bytes} bytes, above the limit of {MAX_SCHEMA_BYTES}")]
    SchemaTooLarge {
        /// Canonical size of the schema.
        bytes: usize,
    },
    /// The input schema is not a JSON object with `"type": "object"`.
    #[error("input schema must be a JSON object schema with \"type\": \"object\"")]
    SchemaNotObject,
    /// The input schema could not be compiled, e.g. it is invalid or uses a remote `$ref`.
    #[error("input schema cannot be compiled: {message}")]
    SchemaInvalid {
        /// Compiler message.
        message: String,
    },
    /// The tool's alias collides with another tool's alias.
    #[error("no unique alias could be generated")]
    AliasCollision,
}

/// A discovered tool that is neither offered nor callable, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedTool {
    /// The tool.
    pub id: ToolId,
    /// Why it was excluded.
    pub reason: ExclusionReason,
}

/// A model's tool call was refused before dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CallRejection {
    /// The name matches no alias in this registry. Untrusted model text.
    #[error("unknown tool {alias:?}")]
    UnknownTool {
        /// The name the model used.
        alias: String,
    },
    /// The tool exists but its classification does not allow calls.
    #[error("tool {tool} is not permitted: {classification:?}")]
    NotPermitted {
        /// The tool.
        tool: ToolId,
        /// Its classification.
        classification: Classification,
    },
    /// The arguments do not satisfy the tool's input schema.
    #[error("invalid arguments for {tool}: {}", .errors.join("; "))]
    InvalidArguments {
        /// The tool.
        tool: ToolId,
        /// Schema violations, at most ten.
        errors: Vec<String>,
    },
}

/// A tool call that passed alias resolution, the permission check and schema validation.
///
/// Only [`ToolRegistry::prepare_call`] creates one. A [`Classification::Write`] call still needs
/// operator approval before dispatch.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedCall {
    tool: ToolId,
    classification: Classification,
    fingerprint: ToolFingerprint,
    arguments: Map<String, Value>,
}

impl ValidatedCall {
    /// Tool to call.
    pub fn tool(&self) -> &ToolId {
        &self.tool
    }

    /// Classification at validation time: [`Classification::Read`] or [`Classification::Write`].
    pub fn classification(&self) -> Classification {
        self.classification
    }

    /// Fingerprint of the tool definition the arguments were validated against.
    pub fn fingerprint(&self) -> ToolFingerprint {
        self.fingerprint
    }

    /// Validated arguments.
    pub fn arguments(&self) -> &Map<String, Value> {
        &self.arguments
    }

    /// Consumes the call and returns its arguments for dispatch.
    pub fn into_arguments(self) -> Map<String, Value> {
        self.arguments
    }
}

/// Tools from every connected server, keyed by identity and alias.
#[derive(Debug)]
pub struct ToolRegistry {
    /// Registered tools sorted by [`ToolId`].
    tools: Vec<RegisteredTool>,
    /// Alias to index in `tools`.
    by_alias: HashMap<String, usize>,
    excluded: Vec<ExcludedTool>,
}

impl ToolRegistry {
    /// Builds a registry from tools discovered on any number of servers.
    ///
    /// `classes` is the host configuration; a tool missing from it is
    /// [`Classification::Unclassified`]. Every tool with a valid definition gets an alias, but
    /// only [`Classification::Read`] and [`Classification::Write`] tools are offered or callable.
    /// The result does not depend on the order of `descriptors`.
    pub fn build(
        descriptors: impl IntoIterator<Item = ToolDescriptor>,
        classes: &HashMap<ToolId, Classification>,
    ) -> Self {
        let mut descriptors: Vec<ToolDescriptor> = descriptors.into_iter().collect();
        descriptors.sort_by(|a, b| a.id.cmp(&b.id));

        let mut excluded = Vec::new();
        let unique = remove_duplicates(descriptors, &mut excluded);

        let mut base_counts: HashMap<String, usize> = HashMap::new();
        for descriptor in &unique {
            *base_counts.entry(base_alias(&descriptor.id).0).or_default() += 1;
        }

        let mut tools = Vec::with_capacity(unique.len());
        let mut by_alias = HashMap::with_capacity(unique.len());
        let mut used_aliases = HashSet::with_capacity(unique.len());
        for descriptor in unique {
            let (base, exact) = base_alias(&descriptor.id);
            let shared = base_counts.get(&base).is_some_and(|&n| n > 1);
            let alias = if exact && !shared && base.len() <= MAX_ALIAS_LEN {
                base
            } else {
                hashed_alias(&base, &descriptor.id)
            };
            match register(descriptor, alias, &mut used_aliases, classes) {
                Ok(tool) => {
                    by_alias.insert(tool.alias.clone(), tools.len());
                    tools.push(tool);
                }
                Err(exclusion) => excluded.push(exclusion),
            }
        }

        Self {
            tools,
            by_alias,
            excluded,
        }
    }

    /// Tool definitions to send to the model: [`Classification::Read`] and
    /// [`Classification::Write`] tools only, in [`ToolId`] order.
    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .filter(|tool| tool.classification.is_offered())
            .map(|tool| ToolSpec {
                name: tool.alias.clone(),
                description: tool.description.clone().unwrap_or_default(),
                input_schema: tool.input_schema.clone(),
            })
            .collect()
    }

    /// Every registered tool, offered or not, in [`ToolId`] order.
    pub fn tools(&self) -> &[RegisteredTool] {
        &self.tools
    }

    /// Discovered tools that were not registered.
    pub fn excluded(&self) -> &[ExcludedTool] {
        &self.excluded
    }

    /// Looks up a registered tool by identity.
    pub fn get(&self, id: &ToolId) -> Option<&RegisteredTool> {
        self.tools
            .binary_search_by(|tool| tool.id.cmp(id))
            .ok()
            .and_then(|index| self.tools.get(index))
    }

    /// Looks up a registered tool by exact alias.
    pub fn resolve_alias(&self, alias: &str) -> Option<&RegisteredTool> {
        self.by_alias
            .get(alias)
            .and_then(|&index| self.tools.get(index))
    }

    /// Turns a model tool call into a [`ValidatedCall`]: the enforcement point every dispatch
    /// must pass through.
    ///
    /// Resolves `alias` through the alias table, refuses tools that are not
    /// [`Classification::Read`] or [`Classification::Write`], and validates `arguments` against
    /// the tool's input schema. Arguments must be a JSON object.
    pub fn prepare_call(
        &self,
        alias: &str,
        arguments: Value,
    ) -> Result<ValidatedCall, CallRejection> {
        let tool = self
            .resolve_alias(alias)
            .ok_or_else(|| CallRejection::UnknownTool {
                alias: alias.to_owned(),
            })?;
        if !tool.classification.is_offered() {
            return Err(CallRejection::NotPermitted {
                tool: tool.id.clone(),
                classification: tool.classification,
            });
        }
        let errors: Vec<String> = tool
            .validator
            .iter_errors(&arguments)
            .take(MAX_REPORTED_ARGUMENT_ERRORS)
            .map(|error| {
                let path = error.instance_path().as_str();
                let path = if path.is_empty() { "/" } else { path };
                format!("{path}: {error}")
            })
            .collect();
        if !errors.is_empty() {
            return Err(CallRejection::InvalidArguments {
                tool: tool.id.clone(),
                errors,
            });
        }
        let Value::Object(arguments) = arguments else {
            return Err(CallRejection::InvalidArguments {
                tool: tool.id.clone(),
                errors: vec!["/: arguments must be a JSON object".to_owned()],
            });
        };
        Ok(ValidatedCall {
            tool: tool.id.clone(),
            classification: tool.classification,
            fingerprint: tool.fingerprint,
            arguments,
        })
    }
}

/// Removes every descriptor whose identity occurs more than once in `sorted`, recording one
/// exclusion per identity. Keeping none of them makes the result independent of input order.
fn remove_duplicates(
    sorted: Vec<ToolDescriptor>,
    excluded: &mut Vec<ExcludedTool>,
) -> Vec<ToolDescriptor> {
    let mut unique: Vec<ToolDescriptor> = Vec::with_capacity(sorted.len());
    let mut duplicate_of_last = false;
    for descriptor in sorted {
        if unique.last().is_some_and(|last| last.id == descriptor.id) {
            if !duplicate_of_last {
                duplicate_of_last = true;
                excluded.push(ExcludedTool {
                    id: descriptor.id.clone(),
                    reason: ExclusionReason::DuplicateTool,
                });
            }
            continue;
        }
        if duplicate_of_last {
            unique.pop();
            duplicate_of_last = false;
        }
        unique.push(descriptor);
    }
    if duplicate_of_last {
        unique.pop();
    }
    unique
}

/// Returns `<server>__<tool>` with characters outside `[A-Za-z0-9_-]` replaced by `_`, and
/// whether the tool name needed no replacement.
fn base_alias(id: &ToolId) -> (String, bool) {
    let mut alias = String::with_capacity(id.server.as_str().len() + 2 + id.name.len());
    alias.push_str(id.server.as_str());
    alias.push_str(ALIAS_SEPARATOR);
    let mut exact = !id.name.is_empty();
    for c in id.name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            alias.push(c);
        } else {
            alias.push('_');
            exact = false;
        }
    }
    (alias, exact)
}

/// Shortens `base` and appends a hash of the tool identity, keeping the alias within
/// [`MAX_ALIAS_LEN`]. `base` is ASCII, so any byte index is a character boundary.
fn hashed_alias(base: &str, id: &ToolId) -> String {
    let digest = Sha256::digest(id.to_string().as_bytes());
    let hash = to_hex(&digest);
    let keep = base.len().min(MAX_ALIAS_LEN - ALIAS_HASH_HEX_LEN - 1);
    let prefix = base.get(..keep).unwrap_or(base);
    format!(
        "{prefix}_{}",
        hash.get(..ALIAS_HASH_HEX_LEN).unwrap_or(&hash)
    )
}

/// Validates one tool definition and creates its registry entry.
fn register(
    descriptor: ToolDescriptor,
    alias: String,
    used_aliases: &mut HashSet<String>,
    classes: &HashMap<ToolId, Classification>,
) -> Result<RegisteredTool, ExcludedTool> {
    let ToolDescriptor {
        id,
        description,
        input_schema,
        hints: _,
    } = descriptor;
    let exclude = |id: ToolId, reason| ExcludedTool { id, reason };

    let canonical_schema = {
        let mut out = String::new();
        write_canonical(&mut out, &input_schema);
        out
    };
    if canonical_schema.len() > MAX_SCHEMA_BYTES {
        let bytes = canonical_schema.len();
        return Err(exclude(id, ExclusionReason::SchemaTooLarge { bytes }));
    }
    if input_schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(exclude(id, ExclusionReason::SchemaNotObject));
    }
    let validator = match jsonschema::validator_for(&input_schema) {
        Ok(validator) => validator,
        Err(error) => {
            let message = error.to_string();
            return Err(exclude(id, ExclusionReason::SchemaInvalid { message }));
        }
    };
    if !used_aliases.insert(alias.clone()) {
        return Err(exclude(id, ExclusionReason::AliasCollision));
    }

    let fingerprint = fingerprint(&id, description.as_deref(), &canonical_schema);
    let classification = classes
        .get(&id)
        .copied()
        .unwrap_or(Classification::Unclassified);
    Ok(RegisteredTool {
        id,
        alias,
        description,
        input_schema,
        classification,
        fingerprint,
        validator,
    })
}

/// Hashes the canonical JSON object
/// `{"description":…,"input_schema":…,"name":…,"server":…}`, keys in JCS order.
fn fingerprint(id: &ToolId, description: Option<&str>, canonical_schema: &str) -> ToolFingerprint {
    let mut canonical = String::with_capacity(canonical_schema.len() + 64);
    canonical.push_str("{\"description\":");
    match description {
        Some(text) => write_string(&mut canonical, text),
        None => canonical.push_str("null"),
    }
    canonical.push_str(",\"input_schema\":");
    canonical.push_str(canonical_schema);
    canonical.push_str(",\"name\":");
    write_string(&mut canonical, &id.name);
    canonical.push_str(",\"server\":");
    write_string(&mut canonical, id.server.as_str());
    canonical.push('}');
    ToolFingerprint(Sha256::digest(canonical.as_bytes()).into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::tools::{InvalidServerId, ServerId, ToolHints};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn id(server: &str, name: &str) -> Result<ToolId, InvalidServerId> {
        Ok(ToolId {
            server: ServerId::new(server)?,
            name: name.to_owned(),
        })
    }

    fn descriptor(tool: &ToolId, schema: Value) -> ToolDescriptor {
        ToolDescriptor {
            id: tool.clone(),
            description: Some(format!("{tool} description")),
            input_schema: schema,
            hints: ToolHints::default(),
        }
    }

    fn lookup_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "sku": { "type": "string" },
                "count": { "type": "integer" }
            },
            "required": ["sku"],
            "additionalProperties": false
        })
    }

    fn is_valid_alias(alias: &str) -> bool {
        (1..=MAX_ALIAS_LEN).contains(&alias.len())
            && alias
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }

    /// Two servers with a `lookup` tool, a denied tool and an unclassified tool.
    fn two_server_registry() -> Result<ToolRegistry, InvalidServerId> {
        let sales = id("sales", "lookup")?;
        let weather = id("weather", "lookup")?;
        let denied = id("sales", "delete_all")?;
        let unclassified = id("weather", "reset")?;
        let classes = HashMap::from([
            (sales.clone(), Classification::Read),
            (weather.clone(), Classification::Write),
            (denied.clone(), Classification::Denied),
        ]);
        let descriptors = [&sales, &weather, &denied, &unclassified]
            .into_iter()
            .map(|tool| descriptor(tool, lookup_schema()));
        Ok(ToolRegistry::build(descriptors, &classes))
    }

    #[test]
    fn same_tool_name_on_two_servers_routes_unambiguously() -> TestResult {
        let registry = two_server_registry()?;
        let sales = registry.prepare_call("sales__lookup", json!({ "sku": "A1" }))?;
        let weather = registry.prepare_call("weather__lookup", json!({ "sku": "A1" }))?;
        assert_eq!(sales.tool(), &id("sales", "lookup")?);
        assert_eq!(sales.classification(), Classification::Read);
        assert_eq!(weather.tool(), &id("weather", "lookup")?);
        assert_eq!(weather.classification(), Classification::Write);
        Ok(())
    }

    #[test]
    fn only_read_and_write_tools_are_offered() -> TestResult {
        let registry = two_server_registry()?;
        let names: Vec<String> = registry.tool_specs().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["sales__lookup", "weather__lookup"]);
        assert_eq!(registry.tools().len(), 4);
        Ok(())
    }

    #[test]
    fn denied_and_unclassified_tools_are_refused() -> TestResult {
        let registry = two_server_registry()?;
        let denied = registry.prepare_call("sales__delete_all", json!({ "sku": "A1" }));
        assert_eq!(
            denied,
            Err(CallRejection::NotPermitted {
                tool: id("sales", "delete_all")?,
                classification: Classification::Denied,
            })
        );
        let unclassified = registry.prepare_call("weather__reset", json!({ "sku": "A1" }));
        assert_eq!(
            unclassified,
            Err(CallRejection::NotPermitted {
                tool: id("weather", "reset")?,
                classification: Classification::Unclassified,
            })
        );
        Ok(())
    }

    #[test]
    fn model_names_are_looked_up_never_parsed() -> TestResult {
        let registry = two_server_registry()?;
        for name in [
            "sales/lookup",
            "sales.lookup",
            "lookup",
            "SALES__lookup",
            "",
        ] {
            assert_eq!(
                registry.prepare_call(name, json!({ "sku": "A1" })),
                Err(CallRejection::UnknownTool {
                    alias: name.to_owned()
                }),
                "{name:?} must not resolve"
            );
        }
        Ok(())
    }

    #[test]
    fn invalid_arguments_are_rejected() -> TestResult {
        let registry = two_server_registry()?;
        let cases = [
            ("missing required field", json!({ "count": 1 })),
            ("wrong type", json!({ "sku": 7 })),
            ("extra property", json!({ "sku": "A1", "extra": true })),
            ("not an object", json!(["A1"])),
        ];
        for (case, arguments) in cases {
            let result = registry.prepare_call("sales__lookup", arguments);
            assert!(
                matches!(result, Err(CallRejection::InvalidArguments { ref errors, .. }) if !errors.is_empty()),
                "{case}: {result:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn valid_arguments_are_passed_through() -> TestResult {
        let registry = two_server_registry()?;
        let call = registry.prepare_call("sales__lookup", json!({ "sku": "A1", "count": 2 }))?;
        let expected = json!({ "sku": "A1", "count": 2 });
        assert_eq!(Some(call.arguments()), expected.as_object());
        Ok(())
    }

    #[test]
    fn aliases_are_valid_and_disambiguated() -> TestResult {
        let long_name = "x".repeat(80);
        let tools = [
            id("sales", "get.price")?,
            id("sales", "get_price")?,
            id("sales", "läge")?,
            id("sales", &long_name)?,
            id("a", "b__c")?,
            id("a_", "_c")?,
        ];
        let descriptors = tools.iter().map(|tool| descriptor(tool, lookup_schema()));
        let registry = ToolRegistry::build(descriptors, &HashMap::new());
        assert!(registry.excluded().is_empty(), "{:?}", registry.excluded());

        let aliases: HashSet<&str> = registry.tools().iter().map(RegisteredTool::alias).collect();
        assert_eq!(aliases.len(), tools.len());
        for tool in registry.tools() {
            assert!(
                is_valid_alias(tool.alias()),
                "invalid alias {}",
                tool.alias()
            );
            let resolved = registry.resolve_alias(tool.alias()).map(RegisteredTool::id);
            assert_eq!(resolved, Some(tool.id()));
        }
        // `get.price` and `get_price` sanitise to the same base, so both are hashed.
        for tool in &tools[..2] {
            let alias = registry
                .get(tool)
                .map(RegisteredTool::alias)
                .unwrap_or_default();
            assert!(alias.starts_with("sales__get_price_"), "{alias}");
        }
        Ok(())
    }

    #[test]
    fn aliases_are_stable_across_builds_and_input_order() -> TestResult {
        let tools = [
            id("sales", "lookup")?,
            id("weather", "lookup")?,
            id("sales", "a.b")?,
        ];
        let forward = ToolRegistry::build(
            tools.iter().map(|tool| descriptor(tool, lookup_schema())),
            &HashMap::new(),
        );
        let reverse = ToolRegistry::build(
            tools
                .iter()
                .rev()
                .map(|tool| descriptor(tool, lookup_schema())),
            &HashMap::new(),
        );
        let aliases = |registry: &ToolRegistry| -> Vec<(ToolId, String)> {
            registry
                .tools()
                .iter()
                .map(|tool| (tool.id().clone(), tool.alias().to_owned()))
                .collect()
        };
        assert_eq!(aliases(&forward), aliases(&reverse));
        // Pinned values: a change here changes aliases across releases.
        assert_eq!(
            forward.get(&tools[0]).map(RegisteredTool::alias),
            Some("sales__lookup")
        );
        assert_eq!(
            forward.get(&tools[2]).map(RegisteredTool::alias),
            Some("sales__a_b_e4f3b08958aff978")
        );
        Ok(())
    }

    #[test]
    fn invalid_schemas_are_excluded_and_reported() -> TestResult {
        let not_object = id("s", "not_object")?;
        let uncompilable = id("s", "uncompilable")?;
        let remote_ref = id("s", "remote_ref")?;
        let too_large = id("s", "too_large")?;
        let big_enum: Vec<Value> = (0..MAX_SCHEMA_BYTES / 4).map(|n| json!(n)).collect();
        let descriptors = [
            descriptor(&not_object, json!({ "type": "string" })),
            descriptor(
                &uncompilable,
                json!({ "type": "object", "properties": { "a": { "type": 5 } } }),
            ),
            descriptor(
                &remote_ref,
                json!({ "type": "object", "properties": { "a": { "$ref": "https://example.invalid/a.json" } } }),
            ),
            descriptor(
                &too_large,
                json!({ "type": "object", "properties": { "a": { "enum": big_enum } } }),
            ),
        ];
        let classes = HashMap::from([
            (not_object.clone(), Classification::Read),
            (uncompilable.clone(), Classification::Read),
            (remote_ref.clone(), Classification::Read),
            (too_large.clone(), Classification::Read),
        ]);
        let registry = ToolRegistry::build(descriptors, &classes);
        assert!(registry.tools().is_empty());
        assert!(registry.tool_specs().is_empty());

        let reason = |tool: &ToolId| {
            registry
                .excluded()
                .iter()
                .find(|excluded| &excluded.id == tool)
                .map(|excluded| excluded.reason.clone())
        };
        assert_eq!(reason(&not_object), Some(ExclusionReason::SchemaNotObject));
        assert!(matches!(
            reason(&uncompilable),
            Some(ExclusionReason::SchemaInvalid { .. })
        ));
        assert!(matches!(
            reason(&remote_ref),
            Some(ExclusionReason::SchemaInvalid { .. })
        ));
        assert!(matches!(
            reason(&too_large),
            Some(ExclusionReason::SchemaTooLarge { .. })
        ));
        Ok(())
    }

    #[test]
    fn duplicate_tools_are_excluded_regardless_of_order() -> TestResult {
        let tool = id("sales", "lookup")?;
        let other = id("sales", "other")?;
        let mut second = descriptor(&tool, lookup_schema());
        second.description = Some("a different definition".to_owned());
        let classes = HashMap::from([(tool.clone(), Classification::Read)]);
        let registry = ToolRegistry::build(
            [
                descriptor(&tool, lookup_schema()),
                descriptor(&other, lookup_schema()),
                second,
            ],
            &classes,
        );
        assert!(registry.get(&tool).is_none());
        assert!(registry.get(&other).is_some());
        assert_eq!(
            registry.excluded(),
            [ExcludedTool {
                id: tool,
                reason: ExclusionReason::DuplicateTool
            }]
        );
        Ok(())
    }

    #[test]
    fn fingerprint_changes_with_the_definition_only() -> TestResult {
        let tool = id("sales", "lookup")?;
        let fingerprint_of = |descriptor: ToolDescriptor| {
            ToolRegistry::build([descriptor], &HashMap::new())
                .tools()
                .first()
                .map(RegisteredTool::fingerprint)
        };
        let base = fingerprint_of(descriptor(&tool, lookup_schema()));
        assert!(base.is_some());

        let reordered: Value = serde_json::from_str(
            r#"{"additionalProperties":false,"required":["sku"],"properties":{"count":{"type":"integer"},"sku":{"type":"string"}},"type":"object"}"#,
        )?;
        assert_eq!(fingerprint_of(descriptor(&tool, reordered)), base);

        let mut changed_schema = lookup_schema();
        changed_schema["required"] = json!(["sku", "count"]);
        assert_ne!(fingerprint_of(descriptor(&tool, changed_schema)), base);

        let mut changed_description = descriptor(&tool, lookup_schema());
        changed_description.description = None;
        assert_ne!(fingerprint_of(changed_description), base);

        let other_server = id("weather", "lookup")?;
        assert_ne!(
            fingerprint_of(descriptor(&other_server, lookup_schema())),
            base
        );
        Ok(())
    }
}
