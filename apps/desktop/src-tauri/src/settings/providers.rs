//! Provider editing and write-only Keychain entry; responses carry only references/status.
use super::*;
use igor_config::{ProviderConfig, ProviderEntry, Secret};

#[derive(Debug, Serialize)]
pub struct ProviderRow {
    pub entry: ProviderEntry,
    pub key_missing: Option<String>,
    pub key_saved: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct ProviderSettingsView {
    pub providers: Vec<ProviderRow>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveProviderRequest {
    pub replacing: Option<String>,
    pub entry: ProviderEntry,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequest {
    pub id: String,
}

// Deliberately no Debug or Serialize: a pasted credential is input only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveProviderKeyRequest {
    pub replacing: Option<String>,
    pub entry: ProviderEntry,
    pub key: String,
}

/// Inspect only presence/emptiness; do not resolve a Secret for a Settings response.
pub fn key_variable_missing(name: &str) -> bool {
    std::env::var_os(name).is_none_or(|value| value.is_empty())
}

/// Configuration identity only; never hash or return a resolved credential value.
pub fn provider_revision(config: &ProviderConfig) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        &config.id,
        format!("{:?}", config.kind),
        &config.model,
        &config.base_url,
        config
            .api_key
            .as_ref()
            .map(|key| (key.env_var(), key.keychain_name())),
    )
        .hash(&mut hash);
    format!("{:016x}", hash.finish())
}

fn provider_error(error: &ConfigError, index: usize) -> CommandError {
    let prefix = format!("providers[{index}].");
    let field = error
        .field()
        .and_then(|field| field.strip_prefix(&prefix))
        .map(str::to_owned);
    let message = match (error.problem(), &field) {
        (Some(problem), Some(_)) => problem.to_string(),
        _ => "Provider configuration could not be saved.".into(),
    };
    CommandError { field, message }
}

impl Settings {
    fn remember_key(&self, name: &str, saved: bool) {
        self.key_status
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.into(), saved);
    }

    pub fn credential_version(&self, id: &str) -> u64 {
        *self
            .credential_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .unwrap_or(&0)
    }

    fn changed_key(&self, id: &str) {
        let mut versions = self
            .credential_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let version = versions.entry(id.into()).or_default();
        *version = version.wrapping_add(1);
    }

    /// Called when a session starts, never by the Settings/picker refresh.
    pub fn resolve_provider_key(
        &self,
        config: &ProviderConfig,
    ) -> Result<Option<Secret>, CommandError> {
        let Some(reference) = config.api_key.as_ref() else {
            return Ok(None);
        };
        let result = reference.resolve_in(|name| std::env::var_os(name), self.key_store.as_ref());
        if let Some(name) = reference.keychain_name() {
            self.remember_key(name, result.is_ok());
        }
        result.map(Some).map_err(|_| {
            CommandError::message(
                "Provider key is unavailable. Replace it in Settings → Model providers.",
            )
        })
    }

    /// Validate the complete config before touching Keychain. Restore the previous key if
    /// the subsequent atomic config save fails. No response/error contains the pasted value.
    pub async fn save_provider_key(
        &self,
        replacing: Option<&str>,
        mut entry: ProviderEntry,
        key: String,
    ) -> Result<ProviderSettingsView, CommandError> {
        let secret = Secret::new(key);
        if secret.expose().trim().is_empty() {
            return Err(CommandError {
                field: Some("api_key".into()),
                message: "Enter an API key.".into(),
            });
        }
        {
            let _edit = self.edits.lock().await;
            let mut doc = self.document()?;
            let entries = doc
                .providers()
                .map_err(|_| CommandError::message("Provider entries could not be loaded."))?;
            let index = match replacing {
                Some(id) => entries.iter().position(|p| p.id == id).ok_or_else(|| {
                    CommandError::message("The provider to edit no longer exists.")
                })?,
                None => entries.len(),
            };
            entry.api_key_keychain = Some(entry.id.clone());
            entry.api_key_env = None;
            doc.put_provider(replacing, &entry)
                .map_err(|_| CommandError::message("The provider to edit no longer exists."))?;
            doc.validate()
                .map_err(|error| provider_error(&error, index))?;
            let previous = self
                .key_store
                .get(&entry.id)
                .map_err(|_| CommandError::message("Could not access Keychain. Try again."))?;
            self.key_store.set(&entry.id, &secret).map_err(|_| {
                CommandError::message("Could not save the key in Keychain. Try again.")
            })?;
            if let Err(error) = doc.save() {
                let restored = match previous {
                    Some(previous) => self.key_store.set(&entry.id, &previous),
                    None => self.key_store.delete(&entry.id).map(|_| ()),
                };
                if restored.is_err() {
                    return Err(CommandError::message(
                        "Configuration save failed and the Keychain key could not be restored. Replace the key before starting a session.",
                    ));
                }
                return Err(provider_error(&error, index));
            }
            self.remember_key(&entry.id, true);
            self.changed_key(&entry.id);
        }
        self.provider_view()
    }

    pub async fn remove_provider_key(
        &self,
        id: &str,
    ) -> Result<ProviderSettingsView, CommandError> {
        {
            let _edit = self.edits.lock().await;
            let entries = self
                .document()?
                .providers()
                .map_err(|_| CommandError::message("Provider entries could not be loaded."))?;
            let name = entries
                .iter()
                .find(|entry| entry.id == id)
                .and_then(|entry| entry.api_key_keychain.as_deref())
                .ok_or_else(|| CommandError::message("This provider has no Keychain reference."))?;
            self.key_store.delete(name).map_err(|_| {
                CommandError::message("Could not remove the key from Keychain. Try again.")
            })?;
            self.remember_key(name, false);
            self.changed_key(id);
        }
        // Retain the reference so Anthropic config remains valid and can accept a replacement.
        self.provider_view()
    }

    pub fn provider_choices(&self) -> Result<Vec<ProviderConfig>, CommandError> {
        let config = self
            .document()?
            .validate()
            .map_err(|_| CommandError::message("Provider configuration could not be loaded."))?;
        Ok(config.providers().to_vec())
    }

    pub fn provider_view(&self) -> Result<ProviderSettingsView, CommandError> {
        let doc = self.document()?;
        doc.validate()
            .map_err(|_| CommandError::message("Provider configuration could not be loaded."))?;
        let entries = doc
            .providers()
            .map_err(|_| CommandError::message("Provider entries could not be loaded."))?;
        let providers = entries
            .into_iter()
            .map(|entry| {
                let key_missing = entry
                    .api_key_env
                    .as_deref()
                    .filter(|name| key_variable_missing(name))
                    .map(str::to_owned);
                let key_saved = entry.api_key_keychain.as_ref().and_then(|name| {
                    self.key_status
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(name)
                        .copied()
                });
                ProviderRow {
                    entry,
                    key_missing,
                    key_saved,
                }
            })
            .collect();
        Ok(ProviderSettingsView { providers })
    }

    pub async fn save_provider(
        &self,
        replacing: Option<&str>,
        entry: ProviderEntry,
    ) -> Result<ProviderSettingsView, CommandError> {
        {
            let _edit = self.edits.lock().await;
            let mut doc = self.document()?;
            let entries = doc
                .providers()
                .map_err(|_| CommandError::message("Provider entries could not be loaded."))?;
            let index = match replacing {
                Some(id) => entries
                    .iter()
                    .position(|provider| provider.id == id)
                    .ok_or_else(|| {
                        CommandError::message("The provider to edit no longer exists.")
                    })?,
                None => entries.len(),
            };
            doc.put_provider(replacing, &entry)
                .map_err(|_| CommandError::message("The provider to edit no longer exists."))?;
            doc.validate()
                .map_err(|error| provider_error(&error, index))?;
            doc.save().map_err(|error| provider_error(&error, index))?;
        }
        self.provider_view()
    }

    pub async fn remove_provider(&self, id: &str) -> Result<ProviderSettingsView, CommandError> {
        {
            let _edit = self.edits.lock().await;
            let mut doc = self.document()?;
            if !doc.remove_provider(id) {
                return Err(CommandError::message("The provider no longer exists."));
            }
            doc.save()
                .map_err(|_| CommandError::message("Provider configuration could not be saved."))?;
        }
        self.provider_view()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use igor_config::{Config, KeyStore, ProviderKind};
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct CountingStore {
        inner: igor_config::MemoryKeyStore,
        reads: std::sync::atomic::AtomicUsize,
    }
    impl igor_config::KeyStore for CountingStore {
        fn get(&self, name: &str) -> Result<Option<Secret>, igor_config::SecretError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.inner.get(name)
        }
        fn set(&self, name: &str, value: &Secret) -> Result<(), igor_config::SecretError> {
            self.inner.set(name, value)
        }
        fn delete(&self, name: &str) -> Result<bool, igor_config::SecretError> {
            self.inner.delete(name)
        }
    }

    #[tokio::test]
    async fn pasted_key_stays_in_store_and_refresh_never_reads_it() -> TestResult {
        let dir = std::env::temp_dir().join(format!("igor-keychain-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        let store = Arc::new(CountingStore {
            inner: igor_config::MemoryKeyStore::default(),
            reads: std::sync::atomic::AtomicUsize::new(0),
        });
        let settings = Settings::with_key_store(Some(path.clone()), store.clone());
        let entry = ProviderEntry {
            id: "cloud".into(),
            kind: ProviderKind::Anthropic,
            model: "demo".into(),
            base_url: None,
            api_key_env: Some("IGOR_OLD_KEY".into()),
            api_key_keychain: None,
        };
        let sentinel = "sk-sentinel-keychain-never-return-20261101";
        let saved = settings
            .save_provider_key(None, entry.clone(), sentinel.into())
            .await?;
        assert_eq!(saved.providers[0].key_saved, Some(true));
        assert_eq!(saved.providers[0].entry.api_key_env, None);
        assert_eq!(
            saved.providers[0].entry.api_key_keychain.as_deref(),
            Some("cloud")
        );
        let reads = store.reads.load(Ordering::Relaxed);
        let mut payloads = vec![serde_json::to_string(&saved)?];
        for _ in 0..4 {
            payloads.push(serde_json::to_string(&settings.provider_view()?)?);
            payloads.push(serde_json::to_string(&crate::provider_options(&settings)?)?);
        }
        assert_eq!(
            store.reads.load(Ordering::Relaxed),
            reads,
            "refresh must not read Keychain"
        );
        let configured = settings.provider_choices()?.remove(0);
        let secret = settings
            .resolve_provider_key(&configured)?
            .ok_or("missing secret")?;
        assert_eq!(secret.expose(), sentinel);
        assert_eq!(
            store.reads.load(Ordering::Relaxed),
            reads + 1,
            "session start checks Keychain"
        );
        let version = settings.credential_version("cloud");
        let replaced = settings
            .save_provider_key(Some("cloud"), entry, "replacement-sentinel".into())
            .await?;
        assert!(settings.credential_version("cloud") > version);
        payloads.push(serde_json::to_string(&replaced)?);
        let removed = settings.remove_provider_key("cloud").await?;
        assert_eq!(removed.providers[0].key_saved, Some(false));
        payloads.push(serde_json::to_string(&removed)?);
        let error = settings
            .resolve_provider_key(&configured)
            .err()
            .ok_or("accepted removed key")?;
        payloads.push(serde_json::to_string(&error)?);
        payloads.push(std::fs::read_to_string(&path)?);
        for payload in payloads {
            assert!(!payload.contains(sentinel));
            assert!(!payload.contains("replacement-sentinel"));
        }
        assert!(store.inner.get("cloud")?.is_none());
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn invalid_config_never_writes_a_key() -> TestResult {
        let store = Arc::new(igor_config::MemoryKeyStore::default());
        let settings = Settings::with_key_store(
            Some(
                std::env::temp_dir().join(format!("igor-invalid-key-{}.toml", std::process::id())),
            ),
            store.clone(),
        );
        let entry = ProviderEntry {
            id: "cloud".into(),
            kind: ProviderKind::Anthropic,
            model: "".into(),
            base_url: None,
            api_key_env: None,
            api_key_keychain: None,
        };
        assert!(
            settings
                .save_provider_key(None, entry, "sentinel".into())
                .await
                .is_err()
        );
        assert!(igor_config::KeyStore::get(store.as_ref(), "cloud")?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn save_reload_edit_remove_and_key_name_only_round_trip() -> TestResult {
        let dir =
            std::env::temp_dir().join(format!("igor-provider-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        std::fs::write(&path, "# Keep this operator comment.\n")?;
        let settings = Settings::new(Some(path.clone()));
        let local = ProviderEntry {
            id: "local".into(),
            kind: ProviderKind::OpenAiCompatible,
            model: "qwen/qwen3-vl-8b".into(),
            base_url: Some("http://127.0.0.1:1234/v1".into()),
            api_key_env: None,
            api_key_keychain: None,
        };
        settings.save_provider(None, local.clone()).await?;
        let before = provider_revision(&settings.provider_choices()?[0]);
        let cloud = ProviderEntry {
            id: "cloud".into(),
            kind: ProviderKind::Anthropic,
            model: "model".into(),
            base_url: None,
            api_key_env: Some("IGOR_PROVIDER_TEST_UNSET_20261031".into()),
            api_key_keychain: None,
        };
        let view = settings.save_provider(None, cloud.clone()).await?;
        assert_eq!(
            view.providers[1].key_missing.as_deref(),
            Some("IGOR_PROVIDER_TEST_UNSET_20261031")
        );
        let loaded = Config::load(&path)?;
        assert_eq!(
            loaded
                .provider("cloud")
                .ok_or("missing provider")?
                .api_key
                .as_ref()
                .ok_or("missing reference")?
                .env_var(),
            Some("IGOR_PROVIDER_TEST_UNSET_20261031")
        );
        let source = std::fs::read_to_string(&path)?;
        assert!(source.contains("# Keep this operator comment."));
        assert!(source.contains("IGOR_PROVIDER_TEST_UNSET_20261031"));
        assert!(
            !source.contains("api_key_env"),
            "persist core's env reference syntax, not IPC fields"
        );
        let mut edited = local.clone();
        edited.base_url = Some("http://localhost:1235/v1".into());
        settings.save_provider(Some("local"), edited).await?;
        assert_ne!(before, provider_revision(&settings.provider_choices()?[0]));
        assert_eq!(
            Config::load(&path)?
                .provider("local")
                .ok_or("missing local")?
                .base_url,
            "http://localhost:1235/v1"
        );
        let saved = std::fs::read(&path)?;
        let mut invalid = cloud;
        invalid.api_key_env = Some("sk-private-key-must-never-be-saved".into());
        let error = settings
            .save_provider(Some("cloud"), invalid)
            .await
            .err()
            .ok_or("accepted a key value")?;
        assert_eq!(error.field.as_deref(), Some("api_key"));
        assert!(!serde_json::to_string(&error)?.contains("sk-private-key"));
        assert_eq!(std::fs::read(&path)?, saved);
        let view = settings.remove_provider("cloud").await?;
        assert_eq!(view.providers.len(), 1);
        assert!(Config::load(&path)?.provider("cloud").is_none());
        assert!(settings.save_provider(Some("absent"), local).await.is_err());
        assert!(serde_json::from_value::<SaveProviderRequest>(serde_json::json!({"replacing":null,"entry":{"id":"x","kind":"anthropic","model":"m","api_key":"raw-value"}})).is_err());
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }
}
