//! Provider editing stays in ConfigDocument; IPC carries only environment variable names.
use super::*;
use igor_config::{ProviderConfig, ProviderEntry};

#[derive(Debug, Serialize)]
pub struct ProviderRow {
    pub entry: ProviderEntry,
    pub key_missing: Option<String>,
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

/// Configuration identity only; never hash or return a resolved credential value.
pub fn provider_revision(config: &ProviderConfig) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        &config.id,
        format!("{:?}", config.kind),
        &config.model,
        &config.base_url,
        config.api_key.as_ref().map(|key| key.env_var()),
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
    pub fn provider_choices(&self) -> Result<Vec<ProviderConfig>, CommandError> {
        let config = self
            .document()?
            .validate()
            .map_err(|_| CommandError::message("Provider configuration could not be loaded."))?;
        Ok(config.providers().to_vec())
    }

    pub fn provider_view(&self) -> Result<ProviderSettingsView, CommandError> {
        let doc = self.document()?;
        let config = doc
            .validate()
            .map_err(|_| CommandError::message("Provider configuration could not be loaded."))?;
        let entries = doc
            .providers()
            .map_err(|_| CommandError::message("Provider entries could not be loaded."))?;
        let providers = entries
            .into_iter()
            .map(|entry| {
                let key_missing = config
                    .provider(&entry.id)
                    .and_then(|provider| provider.api_key.as_ref())
                    .filter(|key| key.resolve().is_err())
                    .map(|key| key.env_var().to_owned());
                ProviderRow { entry, key_missing }
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
    use igor_config::{Config, ProviderKind};
    type TestResult = Result<(), Box<dyn std::error::Error>>;

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
        };
        settings.save_provider(None, local.clone()).await?;
        let before = provider_revision(&settings.provider_choices()?[0]);
        let cloud = ProviderEntry {
            id: "cloud".into(),
            kind: ProviderKind::Anthropic,
            model: "model".into(),
            base_url: None,
            api_key_env: Some("IGOR_PROVIDER_TEST_UNSET_20261031".into()),
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
            "IGOR_PROVIDER_TEST_UNSET_20261031"
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
