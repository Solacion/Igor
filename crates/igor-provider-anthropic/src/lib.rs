//! Anthropic Messages API adapter for the IGOR provider interface.
//!
//! Sends non-streaming `POST /v1/messages` requests with tool definitions and maps the result
//! to provider-neutral types. Model id, base URL and timeout are configuration values; the API
//! key is held as a redacted [`Secret`] and sent only in the `x-api-key` header.
//!
//! The adapter does not retry. It marks errors [`ProviderError::retryable`] so the session loop
//! can retry visibly.
//!
//! Extended thinking is not requested. Thinking blocks in a response, if any, are dropped: the
//! neutral types cannot carry them and they are never replayed.

mod wire;

use std::time::Duration;

use async_trait::async_trait;
use igor_config::{ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::provider::{ModelRequest, ModelResponse, Provider, ProviderError};
use igor_provider_http::{cancellable, errors, send_json};

pub use igor_provider_http::BuildError;

pub use igor_provider_http::{DEFAULT_REQUEST_TIMEOUT, MAX_RESPONSE_BYTES};

/// Messages API version sent in the `anthropic-version` header.
pub const API_VERSION: &str = "2026-10-21";

/// Settings for [`AnthropicProvider`].
#[derive(Debug)]
pub struct AnthropicSettings {
    /// API base URL, such as `https://api.anthropic.com`.
    pub base_url: String,
    /// Model id sent with every request.
    pub model: String,
    /// API key.
    pub api_key: Secret,
    /// Bound on one request, including reading the response.
    pub request_timeout: Duration,
}

impl AnthropicSettings {
    /// Settings with [`DEFAULT_REQUEST_TIMEOUT`].
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, api_key: Secret) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            api_key,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Settings from a validated `anthropic` provider entry and its resolved API key.
    pub fn from_config(config: &ProviderConfig, api_key: Secret) -> Result<Self, BuildError> {
        if config.kind != ProviderKind::Anthropic {
            return Err(BuildError::WrongKind {
                id: config.id.clone(),
                expected: "anthropic",
            });
        }
        Ok(Self::new(&config.base_url, &config.model, api_key))
    }
}

/// Anthropic Messages API provider.
#[derive(Debug)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Secret,
}

impl AnthropicProvider {
    /// Creates a provider. No request is sent until [`Provider::complete`].
    pub fn new(settings: AnthropicSettings) -> Result<Self, BuildError> {
        let client = igor_provider_http::client(settings.request_timeout)?;
        Ok(Self {
            client,
            endpoint: format!("{}/v1/messages", settings.base_url.trim_end_matches('/')),
            model: settings.model,
            api_key: settings.api_key,
        })
    }

    async fn send(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
        let secret = self.api_key.expose();
        let body = wire::request_body(&self.model, request);
        let http = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", secret)
            .header("anthropic-version", API_VERSION);
        let bytes = send_json(http, body.to_string(), secret).await?;
        wire::parse_response(&bytes).map_err(|err| errors::scrub(err, secret))
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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
        let settings = AnthropicSettings::new("http://127.0.0.1:9", "m", Secret::new(secret));
        assert!(!format!("{settings:?}").contains(secret));
        let provider = AnthropicProvider::new(settings)?;
        assert!(!format!("{provider:?}").contains(secret));
        Ok(())
    }
}
