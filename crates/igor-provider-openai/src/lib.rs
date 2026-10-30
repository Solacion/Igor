//! OpenAI-compatible chat completions adapter for the IGOR provider interface.
//!
//! Targets local servers such as LM Studio through `POST {base_url}/chat/completions`, without
//! streaming. Only native `tool_calls` are read as tool calls; the model must support native tool
//! calling. Base URL, model and timeout are configuration values; an API key is optional and is
//! held as a redacted [`Secret`].
//!
//! The adapter does not retry. It marks errors [`ProviderError::retryable`] so the session loop
//! can retry visibly.

mod wire;

use std::time::Duration;

use async_trait::async_trait;
use igor_config::{ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::provider::{ModelRequest, ModelResponse, Provider, ProviderError};
use igor_provider_http::{cancellable, errors, send_json};
use thiserror::Error;

pub use igor_provider_http::{DEFAULT_REQUEST_TIMEOUT, MAX_RESPONSE_BYTES};
pub use wire::TOOL_ERROR_PREFIX;

/// Settings for [`OpenAiCompatibleProvider`].
#[derive(Debug)]
pub struct OpenAiSettings {
    /// API base URL including the version path, such as `http://127.0.0.1:1234/v1`.
    pub base_url: String,
    /// Model id sent with every request.
    pub model: String,
    /// Optional API key, sent as a bearer token.
    pub api_key: Option<Secret>,
    /// Bound on one request, including reading the response.
    pub request_timeout: Duration,
}

impl OpenAiSettings {
    /// Settings with no API key and [`DEFAULT_REQUEST_TIMEOUT`].
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key: None,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Settings from a validated `openai_compatible` provider entry and its resolved API key,
    /// if the entry has one.
    pub fn from_config(
        config: &ProviderConfig,
        api_key: Option<Secret>,
    ) -> Result<Self, BuildError> {
        if config.kind != ProviderKind::OpenAiCompatible {
            return Err(BuildError::WrongKind {
                id: config.id.clone(),
            });
        }
        let mut settings = Self::new(&config.base_url, &config.model);
        settings.api_key = api_key;
        Ok(settings)
    }
}

/// The provider could not be created.
#[derive(Debug, Error)]
pub enum BuildError {
    /// The configuration entry is not an `openai_compatible` provider.
    #[error("provider {id:?} is not of kind `openai_compatible`")]
    WrongKind {
        /// Provider id.
        id: String,
    },
    /// The HTTP client could not be built.
    #[error("cannot build HTTP client: {0}")]
    Client(#[from] reqwest::Error),
}

/// OpenAI-compatible chat completions provider.
#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<Secret>,
}

impl OpenAiCompatibleProvider {
    /// Creates a provider. No request is sent until [`Provider::complete`].
    pub fn new(settings: OpenAiSettings) -> Result<Self, BuildError> {
        let client = igor_provider_http::client(settings.request_timeout)?;
        Ok(Self {
            client,
            endpoint: format!(
                "{}/chat/completions",
                settings.base_url.trim_end_matches('/')
            ),
            model: settings.model,
            api_key: settings.api_key,
        })
    }

    async fn send(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
        let secret = self.api_key.as_ref().map_or("", Secret::expose);
        let body = wire::request_body(&self.model, request);
        let mut http = self.client.post(&self.endpoint);
        if let Some(key) = &self.api_key {
            http = http.bearer_auth(key.expose());
        }
        let bytes = send_json(http, body.to_string(), secret).await?;
        wire::parse_response(&bytes)
            .map_err(|err| errors::error(err.kind, err.retryable, &err.message, secret))
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai_compatible"
    }

    async fn complete(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
    ) -> Result<ModelResponse, ProviderError> {
        cancellable(cancel, self.send(&request)).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_key() -> Result<(), BuildError> {
        let secret = "IGOR-TEST-SECRET-7f3a";
        let mut settings = OpenAiSettings::new("http://127.0.0.1:9/v1", "m");
        settings.api_key = Some(Secret::new(secret));
        assert!(!format!("{settings:?}").contains(secret));
        let provider = OpenAiCompatibleProvider::new(settings)?;
        assert!(!format!("{provider:?}").contains(secret));
        Ok(())
    }
}
