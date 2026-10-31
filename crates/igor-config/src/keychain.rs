//! API keys kept in the macOS Keychain.
//!
//! A provider's `api_key = { keychain = "NAME" }` names a generic password item with service
//! [`KEYCHAIN_SERVICE`] and account `NAME`. Hosts store a key the operator pastes with
//! [`KeyStore::set`]; the configuration file only ever holds the name. [`SystemKeychain`] is the
//! real store; [`MemoryKeyStore`] lets tests run without touching it.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::secret::{Secret, SecretError};

/// Keychain service under which IGOR's items are stored.
pub const KEYCHAIN_SERVICE: &str = "com.solacion.igor";

/// Longest Keychain item name.
const MAX_NAME_LEN: usize = 64;

/// True for Keychain item names: 1 to 64 ASCII letters, digits, `.`, `_` or `-`.
pub fn is_keychain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Where API keys named in the configuration are kept.
pub trait KeyStore: Send + Sync {
    /// The key stored under `name`, or `None` when there is none.
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretError>;
    /// Stores `secret` under `name`, replacing any earlier key.
    fn set(&self, name: &str, secret: &Secret) -> Result<(), SecretError>;
    /// Removes the key under `name`. Returns whether there was one.
    fn delete(&self, name: &str) -> Result<bool, SecretError>;
}

fn check_name(name: &str) -> Result<(), SecretError> {
    if is_keychain_name(name) {
        Ok(())
    } else {
        Err(SecretError::InvalidKeychainName)
    }
}

/// The login Keychain of the current macOS user. Unsupported on other systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemKeychain;

#[cfg(target_os = "macos")]
mod system {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    use super::{KEYCHAIN_SERVICE, KeyStore, SystemKeychain, check_name};
    use crate::secret::{Secret, SecretError};

    /// `errSecItemNotFound`.
    const ITEM_NOT_FOUND: i32 = -25300;

    fn failed(name: &str, err: &security_framework::base::Error) -> SecretError {
        SecretError::Keychain {
            name: name.to_owned(),
            reason: format!("Keychain error {}", err.code()),
        }
    }

    impl KeyStore for SystemKeychain {
        fn get(&self, name: &str) -> Result<Option<Secret>, SecretError> {
            check_name(name)?;
            match get_generic_password(KEYCHAIN_SERVICE, name) {
                Ok(bytes) => String::from_utf8(bytes)
                    .map(|value| Some(Secret::new(value)))
                    .map_err(|_| SecretError::Keychain {
                        name: name.to_owned(),
                        reason: "the stored key is not valid UTF-8".to_owned(),
                    }),
                Err(err) if err.code() == ITEM_NOT_FOUND => Ok(None),
                Err(err) => Err(failed(name, &err)),
            }
        }

        fn set(&self, name: &str, secret: &Secret) -> Result<(), SecretError> {
            check_name(name)?;
            set_generic_password(KEYCHAIN_SERVICE, name, secret.expose().as_bytes())
                .map_err(|err| failed(name, &err))
        }

        fn delete(&self, name: &str) -> Result<bool, SecretError> {
            check_name(name)?;
            match delete_generic_password(KEYCHAIN_SERVICE, name) {
                Ok(()) => Ok(true),
                Err(err) if err.code() == ITEM_NOT_FOUND => Ok(false),
                Err(err) => Err(failed(name, &err)),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl KeyStore for SystemKeychain {
    fn get(&self, _name: &str) -> Result<Option<Secret>, SecretError> {
        Err(SecretError::KeychainUnsupported)
    }

    fn set(&self, _name: &str, _secret: &Secret) -> Result<(), SecretError> {
        Err(SecretError::KeychainUnsupported)
    }

    fn delete(&self, _name: &str) -> Result<bool, SecretError> {
        Err(SecretError::KeychainUnsupported)
    }
}

/// An in-memory [`KeyStore`] for tests.
#[derive(Debug, Default)]
pub struct MemoryKeyStore {
    keys: Mutex<HashMap<String, String>>,
}

impl MemoryKeyStore {
    fn keys(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.keys.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretError> {
        check_name(name)?;
        Ok(self
            .keys()
            .get(name)
            .map(|value| Secret::new(value.clone())))
    }

    fn set(&self, name: &str, secret: &Secret) -> Result<(), SecretError> {
        check_name(name)?;
        self.keys()
            .insert(name.to_owned(), secret.expose().to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<bool, SecretError> {
        check_name(name)?;
        Ok(self.keys().remove(name).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_restricted() {
        for good in ["claude", "lm_studio", "provider.claude-2"] {
            assert!(is_keychain_name(good), "{good}");
        }
        let long = "a".repeat(MAX_NAME_LEN + 1);
        for bad in ["", "has space", "sk-ant/../x", long.as_str()] {
            assert!(!is_keychain_name(bad), "{bad}");
        }
    }

    #[test]
    fn memory_store_round_trips() -> Result<(), SecretError> {
        let store = MemoryKeyStore::default();
        assert!(store.get("claude")?.is_none());
        store.set("claude", &Secret::new("sk-test"))?;
        assert_eq!(
            store.get("claude")?.as_ref().map(Secret::expose),
            Some("sk-test")
        );
        assert!(store.delete("claude")?);
        assert!(!store.delete("claude")?);
        assert_eq!(
            store.set("bad name", &Secret::new("x")),
            Err(SecretError::InvalidKeychainName)
        );
        Ok(())
    }

    /// Writes, reads and deletes one temporary item in the real login Keychain.
    #[test]
    #[ignore = "touches the user's login Keychain; run by hand on macOS"]
    fn system_keychain_round_trips() -> Result<(), SecretError> {
        let name = format!("igor-test-{}", std::process::id());
        let store = SystemKeychain;
        store.set(&name, &Secret::new("IGOR-TEST-SECRET-kc"))?;
        let read = store.get(&name)?;
        let deleted = store.delete(&name)?;
        assert_eq!(
            read.as_ref().map(Secret::expose),
            Some("IGOR-TEST-SECRET-kc")
        );
        assert!(deleted);
        assert!(store.get(&name)?.is_none());
        Ok(())
    }
}
