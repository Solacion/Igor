//! Raw configuration as written in the file, before validation.
//!
//! Unknown keys are errors. Values whose meaning depends on validation (ids, kinds, classes,
//! secret references) are kept as plain data here so validation can report them with a field
//! path and a typed problem.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    #[serde(default)]
    pub providers: Vec<RawProvider>,
    #[serde(default)]
    pub servers: Vec<RawServer>,
    #[serde(default)]
    pub permissions: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub limits: RawLimits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawProvider {
    pub id: String,
    pub kind: String,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<RawSecret>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawServer {
    pub id: String,
    pub transport: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, RawSecret>,
    pub startup_timeout_ms: Option<u64>,
    pub call_timeout_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLimits {
    pub max_iterations: Option<u64>,
    pub max_result_bytes: Option<u64>,
    pub tool_call_timeout_ms: Option<u64>,
}

/// A secret field as written. A literal value is recorded only as the fact that one was given:
/// the value itself is dropped here so it can never reach an error message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RawSecret {
    Literal,
    Env(String),
    Keychain(String),
}

impl<'de> Deserialize<'de> for RawSecret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RawSecretVisitor)
    }
}

struct RawSecretVisitor;

impl RawSecretVisitor {
    fn literal<E>(self) -> Result<RawSecret, E> {
        Ok(RawSecret::Literal)
    }
}

impl<'de> Visitor<'de> for RawSecretVisitor {
    type Value = RawSecret;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a secret reference such as { env = \"VARIABLE_NAME\" } or { keychain = \"NAME\" }",
        )
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<RawSecret, E> {
        self.literal()
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<RawSecret, E> {
        self.literal()
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<RawSecret, E> {
        self.literal()
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<RawSecret, E> {
        self.literal()
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<RawSecret, E> {
        self.literal()
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawSecret, A::Error> {
        let mut source = None;
        while let Some(key) = map.next_key::<String>()? {
            let make: fn(String) -> RawSecret = match key.as_str() {
                "env" => RawSecret::Env,
                "keychain" => RawSecret::Keychain,
                _ => {
                    return Err(de::Error::custom(format_args!(
                        "unsupported secret source `{key}`: use `env` or `keychain`"
                    )));
                }
            };
            if source.is_some() {
                return Err(de::Error::custom(
                    "a secret reference has exactly one source: `env` or `keychain`",
                ));
            }
            source = Some(make(map.next_value::<String>()?));
        }
        source.ok_or_else(|| de::Error::missing_field("env"))
    }
}
