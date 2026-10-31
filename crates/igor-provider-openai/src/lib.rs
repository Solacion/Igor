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
//! The same details give the context window the model is loaded with. LM Studio cuts a
//! conversation that does not fit, so the model loses earlier tool results and repeats its calls.
//! The adapter refuses a request that would not fit, with a message saying how to fix it, instead
//! of sending it.
//!
//! The adapter does not retry. It marks errors [`ProviderError::retryable`] so the session loop
//! can retry visibly.

mod wire;

use std::sync::atomic::{AtomicU32, Ordering};
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

/// Request bytes per prompt token, for estimating whether a request fits the loaded context.
/// LM Studio with Qwen3 counts about 4.2 bytes of request JSON per prompt token; rounding down
/// errs towards a larger estimate.
const BYTES_PER_TOKEN: usize = 4;

/// Most context kept free for the reply when checking that a request fits.
const REPLY_RESERVE_TOKENS: u32 = 1024;

/// OpenAI-compatible chat completions provider.
#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    endpoint: String,
    /// LM Studio's model details URL, when the base URL ends in `/v1`.
    details: Option<reqwest::Url>,
    model: String,
    api_key: Option<Secret>,
    /// Context window LM Studio last reported for the model, in tokens; 0 when unknown.
    loaded_context: AtomicU32,
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
            loaded_context: AtomicU32::new(0),
        })
    }

    async fn send(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
        let secret = self.api_key.as_ref().map_or("", Secret::expose);
        let body = wire::request_body(&self.model, request).to_string();
        self.check_fits(body.len(), request.max_tokens).await?;
        let mut http = self.client.post(&self.endpoint);
        if let Some(key) = &self.api_key {
            http = http.bearer_auth(key.expose());
        }
        let bytes = send_json(http, body, secret).await?;
        wire::parse_response(&bytes).map_err(|err| errors::scrub(err, secret))
    }

    /// LM Studio's details for the model. Any failure to get them gives `None`: unknown.
    async fn details(&self) -> Option<wire::ModelDetails> {
        let url = self.details.as_ref()?;
        let mut http = self.client.get(url.clone()).timeout(PREFLIGHT_TIMEOUT);
        if let Some(key) = &self.api_key {
            http = http.bearer_auth(key.expose());
        }
        let response = http.send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = response.bytes().await.ok()?;
        if body.len() > MAX_DETAILS_BYTES {
            return None;
        }
        let details = wire::parse_details(&body, &self.model)?;
        self.loaded_context
            .store(details.loaded_context.unwrap_or(0), Ordering::Relaxed);
        Some(details)
    }

    /// Refuses a request of `body_bytes` that, with room for the reply, would not fit the
    /// context LM Studio loaded the model with. The details are asked again before refusing, in
    /// case the operator has reloaded the model with a larger context.
    async fn check_fits(&self, body_bytes: usize, max_tokens: u32) -> Result<(), ProviderError> {
        let needed = u32::try_from(body_bytes / BYTES_PER_TOKEN)
            .unwrap_or(u32::MAX)
            .saturating_add(max_tokens.min(REPLY_RESERVE_TOKENS));
        let known = self.loaded_context.load(Ordering::Relaxed);
        if known == 0 || needed <= known {
            return Ok(());
        }
        match self
            .details()
            .await
            .and_then(|details| details.loaded_context)
        {
            Some(context) if needed > context => Err(ProviderError {
                kind: ProviderErrorKind::InvalidRequest,
                message: format!(
                    "the conversation needs about {needed} tokens of context, but LM Studio has \
                     model {:?} loaded with {context}, so the model would lose earlier tool \
                     results. Reload the model in LM Studio with a larger context length, such \
                     as 32768, or connect fewer tools",
                    self.model
                ),
                retryable: false,
            }),
            _ => Ok(()),
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
        let details = cancellable(cancel, self.details()).await?;
        if details.is_some_and(|details| !details.tool_use) {
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
