//! Secret references and resolved secrets.
//!
//! Configuration never holds a secret value. It holds a [`SecretRef`] naming an environment
//! variable, resolved into a [`Secret`] only when the value is needed. A [`Secret`] never prints
//! or serialises its value.

use std::ffi::OsString;
use std::fmt;

use serde::{Serialize, Serializer};
use thiserror::Error;

/// Text shown in place of a secret value.
pub const REDACTED: &str = "[REDACTED]";

/// Reference to a secret held outside the configuration file, in an environment variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef {
    env: String,
}

impl SecretRef {
    /// Creates a reference to an already validated environment variable name.
    pub(crate) fn from_validated_env(env: String) -> Self {
        Self { env }
    }

    /// Name of the environment variable holding the secret.
    pub fn env_var(&self) -> &str {
        &self.env
    }

    /// Reads the secret from IGOR's process environment.
    pub fn resolve(&self) -> Result<Secret, SecretError> {
        self.resolve_with(|name| std::env::var_os(name))
    }

    /// Reads the secret through `lookup`, which maps a variable name to its value. Lets callers
    /// and tests supply an environment without modifying the process environment.
    pub fn resolve_with(
        &self,
        lookup: impl FnOnce(&str) -> Option<OsString>,
    ) -> Result<Secret, SecretError> {
        let var = || self.env.clone();
        let value = lookup(&self.env).ok_or_else(|| SecretError::Missing { var: var() })?;
        let value = value
            .into_string()
            .map_err(|_| SecretError::NotUnicode { var: var() })?;
        if value.is_empty() {
            return Err(SecretError::Empty { var: var() });
        }
        Ok(Secret::new(value))
    }
}

/// A resolved secret value. `Debug`, `Display` and `Serialize` output are redacted; the value is
/// only available through [`Secret::expose`].
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value obtained outside configuration, for example in tests or from an
    /// embedding host. Prefer [`SecretRef::resolve`] for configured secrets.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the secret value. Call only where the value is sent to its destination, such as
    /// an HTTP authentication header, and never pass the result to logs, events or errors.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Secret").field(&REDACTED).finish()
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

/// A secret reference could not be resolved. Names the variable, never a value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretError {
    /// The environment variable is not set.
    #[error("environment variable {var} is not set")]
    Missing {
        /// Variable name.
        var: String,
    },
    /// The environment variable is set but empty.
    #[error("environment variable {var} is empty")]
    Empty {
        /// Variable name.
        var: String,
    },
    /// The environment variable does not hold valid Unicode.
    #[error("environment variable {var} is not valid Unicode")]
    NotUnicode {
        /// Variable name.
        var: String,
    },
}

/// True for portable environment variable names: `[A-Z_][A-Z0-9_]*`, at most 128 characters.
pub(crate) fn is_env_name(name: &str) -> bool {
    const MAX_LEN: usize = 128;
    let valid_start = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase() || c == '_');
    valid_start
        && name.len() <= MAX_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "IGOR-TEST-SECRET-7f3a";

    fn reference() -> SecretRef {
        SecretRef::from_validated_env("IGOR_TEST_KEY".into())
    }

    fn sentinel_secret() -> Result<Secret, SecretError> {
        reference().resolve_with(|_| Some(SENTINEL.into()))
    }

    #[test]
    fn resolves_value_from_lookup() -> Result<(), SecretError> {
        let secret = sentinel_secret()?;
        assert_eq!(secret.expose(), SENTINEL);
        Ok(())
    }

    #[test]
    fn debug_display_and_serde_never_contain_the_value() -> Result<(), Box<dyn std::error::Error>> {
        let secret = sentinel_secret()?;
        let outputs = [
            format!("{secret:?}"),
            format!("{secret:#?}"),
            format!("{secret}"),
            serde_json::to_string(&secret)?,
            serde_json::to_string(&vec![&secret])?,
        ];
        for output in outputs {
            assert!(!output.contains(SENTINEL), "leaked in {output}");
            assert!(output.contains(REDACTED), "not redacted: {output}");
        }
        Ok(())
    }

    #[test]
    fn resolution_errors_name_the_variable() {
        let cases = [
            (
                None,
                SecretError::Missing {
                    var: "IGOR_TEST_KEY".into(),
                },
            ),
            (
                Some(OsString::new()),
                SecretError::Empty {
                    var: "IGOR_TEST_KEY".into(),
                },
            ),
        ];
        for (value, expected) in cases {
            let result = reference().resolve_with(|_| value);
            assert_eq!(result.err(), Some(expected.clone()));
            assert!(expected.to_string().contains("IGOR_TEST_KEY"));
        }
    }

    #[test]
    fn env_name_rules() {
        for name in ["ANTHROPIC_API_KEY", "_X", "PATH", "A1"] {
            assert!(is_env_name(name), "{name} should be valid");
        }
        let too_long = "A".repeat(129);
        for name in [
            "",
            "path",
            "1A",
            "A-B",
            "A B",
            "sk-ant-abc",
            too_long.as_str(),
        ] {
            assert!(!is_env_name(name), "{name:?} should be invalid");
        }
    }
}
