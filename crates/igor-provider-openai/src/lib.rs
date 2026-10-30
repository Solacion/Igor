//! OpenAI-compatible chat completions adapter for the IGOR provider interface.
//!
//! Targets local servers such as LM Studio through `POST {base_url}/chat/completions`, without
//! streaming. Only native `tool_calls` are read as tool calls; the model must support native tool
//! calling. Base URL, model and timeout are configuration values; an API key is optional and is
//! held as a redacted [`Secret`].
//!
//! The adapter does not retry. It marks errors [`ProviderError::retryable`] so the session loop
//! can retry visibly.

mod errors;
mod wire;

use std::time::Duration;

use async_trait::async_trait;
use igor_config::{ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::provider::{
    ModelRequest, ModelResponse, Provider, ProviderError, ProviderErrorKind,
};
use thiserror::Error;

pub use wire::TOOL_ERROR_PREFIX;

/// Default bound on one request, including reading the response. Local models can be slow.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Largest response body accepted, in bytes.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

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
        let client = reqwest::Client::builder()
            .timeout(settings.request_timeout)
            .build()?;
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
        let mut http = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json")
            .body(body.to_string());
        if let Some(key) = &self.api_key {
            http = http.bearer_auth(key.expose());
        }
        let response = http
            .send()
            .await
            .map_err(|err| errors::transport_error(&err, secret))?;

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = read_body(response, secret).await?;
        if !status.is_success() {
            return Err(errors::status_error(
                status.as_u16(),
                retry_after.as_deref(),
                &bytes,
                secret,
            ));
        }
        wire::parse_response(&bytes)
            .map_err(|err| errors::error(err.kind, err.retryable, &err.message, secret))
    }
}

/// Reads the response body, refusing bodies above [`MAX_RESPONSE_BYTES`].
async fn read_body(
    mut response: reqwest::Response,
    secret: &str,
) -> Result<Vec<u8>, ProviderError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| errors::transport_error(&err, secret))?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(errors::error(
                ProviderErrorKind::MalformedResponse,
                false,
                &format!("response body exceeds {MAX_RESPONSE_BYTES} bytes"),
                secret,
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
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
        if cancel.is_cancelled() {
            return Err(errors::cancelled());
        }
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(errors::cancelled()),
            result = self.send(&request) => result,
        }
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
