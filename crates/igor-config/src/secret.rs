//! Secret references and resolved secrets.
//!
//! Configuration never holds a secret value. It holds a [`SecretRef`] naming an environment
//! variable or a Keychain item (see [`crate::keychain`]), resolved into a [`Secret`] only when
//! the value is needed. A [`Secret`] never prints or serialises its value.

use std::ffi::OsString;
use std::fmt;

use serde::{Serialize, Serializer};
use thiserror::Error;

use crate::keychain::{KeyStore, SystemKeychain};

/// Text shown in place of a secret value.
pub const REDACTED: &str = "[REDACTED]";

/// Reference to a secret held outside the configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef {
    source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Env(String),
    Keychain(String),
}

impl SecretRef {
    /// Creates a reference to an already validated environment variable name.
    pub(crate) fn from_validated_env(env: String) -> Self {
        Self {
            source: Source::Env(env),
        }
    }

    /// Creates a reference to an already validated Keychain item name.
    pub(crate) fn from_validated_keychain(name: String) -> Self {
        Self {
            source: Source::Keychain(name),
        }
    }

    /// Name of the environment variable holding the secret, for an `env` reference.
    pub fn env_var(&self) -> Option<&str> {
        match &self.source {
            Source::Env(var) => Some(var),
            Source::Keychain(_) => None,
        }
    }

    /// Name of the Keychain item holding the secret, for a `keychain` reference.
    pub fn keychain_name(&self) -> Option<&str> {
        match &self.source {
            Source::Keychain(name) => Some(name),
            Source::Env(_) => None,
        }
    }

    /// Where the secret is, for messages: `environment variable NAME` or `Keychain item NAME`.
    pub fn describe(&self) -> String {
        match &self.source {
            Source::Env(var) => format!("environment variable {var}"),
            Source::Keychain(name) => format!("Keychain item {name}"),
        }
    }

    /// Reads the secret from IGOR's process environment or the system Keychain.
    pub fn resolve(&self) -> Result<Secret, SecretError> {
        self.resolve_in(|name| std::env::var_os(name), &SystemKeychain)
    }

    /// Reads an `env` secret through `lookup`, which maps a variable name to its value, and a
    /// `keychain` secret from the system Keychain. Lets callers and tests supply an environment
    /// without modifying the process environment.
    pub fn resolve_with(
        &self,
        lookup: impl FnOnce(&str) -> Option<OsString>,
    ) -> Result<Secret, SecretError> {
        self.resolve_in(lookup, &SystemKeychain)
    }

    /// Reads the secret through `lookup` for an `env` reference, or from `store` for a
    /// `keychain` reference.
    pub fn resolve_in(
        &self,
        lookup: impl FnOnce(&str) -> Option<OsString>,
        store: &dyn KeyStore,
    ) -> Result<Secret, SecretError> {
        let env = match &self.source {
            Source::Env(env) => env,
            Source::Keychain(name) => {
                let secret = store
                    .get(name)?
                    .ok_or_else(|| SecretError::KeychainMissing { name: name.clone() })?;
                if secret.expose().is_empty() {
                    return Err(SecretError::KeychainMissing { name: name.clone() });
                }
                return Ok(secret);
            }
        };
        let var = || env.clone();
        let value = lookup(env).ok_or_else(|| SecretError::Missing { var: var() })?;
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
    /// No key is stored in the Keychain under this name.
    #[error("no API key is stored in the Keychain as {name}")]
    KeychainMissing {
        /// Keychain item name.
        name: String,
    },
    /// The Keychain could not be read or written.
    #[error("Keychain item {name}: {reason}")]
    Keychain {
        /// Keychain item name.
        name: String,
        /// What went wrong, without the value.
        reason: String,
    },
    /// The name is not a valid Keychain item name.
    #[error("invalid Keychain item name: use 1 to 64 letters, digits, '.', '_' or '-'")]
    InvalidKeychainName,
    /// This system has no supported Keychain.
    #[error("the Keychain is only supported on macOS")]
    KeychainUnsupported,
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

    #[test]
    fn keychain_references_resolve_from_the_store_only() -> Result<(), SecretError> {
        use crate::keychain::MemoryKeyStore;
        let store = MemoryKeyStore::default();
        let reference = SecretRef::from_validated_keychain("claude".into());
        assert_eq!(reference.env_var(), None);
        assert_eq!(reference.describe(), "Keychain item claude");
        let missing = reference.resolve_in(|_| Some(SENTINEL.into()), &store);
        assert_eq!(
            missing.err(),
            Some(SecretError::KeychainMissing {
                name: "claude".into()
            })
        );
        store.set("claude", &Secret::new(SENTINEL))?;
        let secret = reference.resolve_in(|_| None, &store)?;
        assert_eq!(secret.expose(), SENTINEL);
        assert!(!format!("{reference:?} {secret:?}").contains(SENTINEL));
        Ok(())
    }
}
