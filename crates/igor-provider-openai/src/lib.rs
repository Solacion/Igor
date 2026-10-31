//! OpenAI-compatible chat completions adapter for the IGOR provider interface.
//!
//! Targets local servers such as LM Studio through `POST {base_url}/chat/completions`, without
//! streaming. Only native `tool_calls` are read as tool calls; the model must support native tool
//! calling. Base URL, model and timeout are configuration values; an API key is optional and is
//! held as a redacted [`Secret`].
//!
//! Before the first request that offers tools, [`Provider::preflight`] asks LM Studio's model
//! details endpoint, `GET {root}/api/v0/models/{model}`, whether the model lists the `tool_use`
//! capability. Other servers, and any answer that is not LM Studio's model details, leave tool
//! support unknown and the session goes ahead.
//!
//! The adapter does not retry. It marks errors [`ProviderError::retryable`] so the session loop
//! can retry visibly.

mod wire;

use std::time::Duration;

use async_trait::async_trait;
use igor_config::{ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::provider::{
    ModelRequest, ModelResponse, Provider, ProviderError, ProviderErrorKind,
};
use igor_provider_http::{cancellable, errors, send_json};

pub use igor_provider_http::BuildError;

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
                expected: "openai_compatible",
            });
        }
        let mut settings = Self::new(&config.base_url, &config.model);
        settings.api_key = api_key;
        Ok(settings)
    }
}

/// Bound on the tool-support check, which LM Studio answers from its model list.
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(5);

/// Largest model details body read by the tool-support check.
const MAX_DETAILS_BYTES: usize = 64 * 1024;

/// OpenAI-compatible chat completions provider.
#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    endpoint: String,
    /// LM Studio's model details URL, when the base URL ends in `/v1`.
    details: Option<reqwest::Url>,
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
            details: details_url(&settings.base_url, &settings.model),
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
        wire::parse_response(&bytes).map_err(|err| errors::scrub(err, secret))
    }

    /// Whether LM Studio's model details say the model cannot call tools. Any failure to get
    /// them means unknown, which is `false`.
    async fn lacks_tool_use(&self) -> bool {
        let Some(url) = &self.details else {
            return false;
        };
        let mut http = self.client.get(url.clone()).timeout(PREFLIGHT_TIMEOUT);
        if let Some(key) = &self.api_key {
            http = http.bearer_auth(key.expose());
        }
        let Ok(response) = http.send().await else {
            return false;
        };
        if !response.status().is_success() {
            return false;
        }
        match response.bytes().await {
            Ok(body) if body.len() <= MAX_DETAILS_BYTES => {
                wire::details_lack_tool_use(&body, &self.model)
            }
            _ => false,
        }
    }
}

/// `{root}/api/v0/models/{model}` for a base URL `{root}/v1`, with `model` as one encoded path
/// segment.
fn details_url(base_url: &str, model: &str) -> Option<reqwest::Url> {
    let root = base_url.trim_end_matches('/').strip_suffix("/v1")?;
    let mut url = reqwest::Url::parse(root).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(["api", "v0", "models", model]);
    Some(url)
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

    async fn preflight(&self, cancel: CancellationToken) -> Result<(), ProviderError> {
        if cancellable(cancel, self.lacks_tool_use()).await? {
            return Err(ProviderError {
                kind: ProviderErrorKind::ToolsUnsupported,
                message: format!(
                    "model {:?} cannot call tools: LM Studio does not list tool use for it",
                    self.model
                ),
                retryable: false,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_url_is_derived_from_a_v1_base_url() {
        let url = |base: &str, model: &str| details_url(base, model).map(String::from);
        assert_eq!(
            url("http://127.0.0.1:1234/v1", "qwen/qwen3-vl-8b").as_deref(),
            Some("http://127.0.0.1:1234/api/v0/models/qwen%2Fqwen3-vl-8b")
        );
        assert_eq!(
            url("http://127.0.0.1:1234/v1/", "m").as_deref(),
            Some("http://127.0.0.1:1234/api/v0/models/m")
        );
        assert_eq!(url("https://api.example.com/openai", "m"), None);
    }

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
