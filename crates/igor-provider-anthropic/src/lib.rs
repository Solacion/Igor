//! Anthropic Messages API adapter for the IGOR provider interface.
//!
//! Sends non-streaming `POST /v1/messages` requests with tool definitions and maps the result
//! to provider-neutral types. Model id, base URL and timeout are configuration values; the API
//! key is held as a redacted [`Secret`] and sent only in the `x-api-key` header.
//!
//! The adapter does not retry. It marks errors [`ProviderError::retryable`] so the session loop
//! can retry visibly.

mod errors;
mod wire;

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use igor_config::{ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::provider::{Message, ModelRequest, ModelResponse, Provider, ProviderError};
use serde_json::Value;
use thiserror::Error;

/// Messages API version sent in the `anthropic-version` header.
pub const API_VERSION: &str = "2026-10-21";

/// Default bound on one request, including reading the response. Non-streaming responses with a
/// large `max_tokens` can take minutes.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Largest response body accepted, in bytes.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Number of assistant turns whose raw content is remembered for replay.
const REPLAY_CAPACITY: usize = 64;

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
            });
        }
        Ok(Self::new(&config.base_url, &config.model, api_key))
    }
}

/// The provider could not be created.
#[derive(Debug, Error)]
pub enum BuildError {
    /// The configuration entry is not an `anthropic` provider.
    #[error("provider {id:?} is not of kind `anthropic`")]
    WrongKind {
        /// Provider id.
        id: String,
    },
    /// The HTTP client could not be built.
    #[error("cannot build HTTP client: {0}")]
    Client(#[from] reqwest::Error),
}

/// Anthropic Messages API provider.
#[derive(Debug)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Secret,
    replay: Mutex<ReplayCache>,
}

impl AnthropicProvider {
    /// Creates a provider. No request is sent until [`Provider::complete`].
    pub fn new(settings: AnthropicSettings) -> Result<Self, BuildError> {
        let client = reqwest::Client::builder()
            .timeout(settings.request_timeout)
            .build()?;
        Ok(Self {
            client,
            endpoint: format!("{}/v1/messages", settings.base_url.trim_end_matches('/')),
            model: settings.model,
            api_key: settings.api_key,
            replay: Mutex::new(ReplayCache::default()),
        })
    }

    fn replay(&self) -> std::sync::MutexGuard<'_, ReplayCache> {
        self.replay.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn send(&self, request: &ModelRequest) -> Result<ModelResponse, ProviderError> {
        let secret = self.api_key.expose();
        let body = {
            let replay = self.replay();
            wire::request_body(&self.model, request, |message| replay.get(message))
        };
        let response = self
            .client
            .post(&self.endpoint)
            .header("x-api-key", secret)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .body(body.to_string())
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

        let parsed = wire::parse_response(&bytes)
            .map_err(|err| errors::error(err.kind, err.retryable, &err.message, secret))?;
        if let Some(raw) = parsed.raw_content_to_replay {
            self.replay().insert(&parsed.response.content, raw);
        }
        Ok(parsed.response)
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
                igor_core::provider::ProviderErrorKind::MalformedResponse,
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
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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

/// Raw content of recent assistant turns that held thinking blocks, keyed by their neutral
/// content.
///
/// The neutral [`igor_core::provider::ContentBlock`] has no thinking variant, but the API needs
/// thinking blocks sent back unchanged when a conversation continues. When the session sends an
/// assistant message back unchanged, the adapter substitutes the raw content it received.
#[derive(Debug, Default)]
struct ReplayCache {
    entries: VecDeque<(String, Vec<Value>)>,
}

impl ReplayCache {
    fn key(content: &[igor_core::provider::ContentBlock]) -> Option<String> {
        serde_json::to_string(content).ok()
    }

    fn get(&self, message: &Message) -> Option<Vec<Value>> {
        let key = Self::key(&message.content)?;
        self.entries
            .iter()
            .rev()
            .find(|(entry, _)| *entry == key)
            .map(|(_, raw)| raw.clone())
    }

    fn insert(&mut self, content: &[igor_core::provider::ContentBlock], raw: Vec<Value>) {
        let Some(key) = Self::key(content) else {
            return;
        };
        if self.entries.len() == REPLAY_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back((key, raw));
    }
}

#[cfg(test)]
mod tests {
    use igor_core::provider::{ContentBlock, Role};
    use serde_json::json;

    use super::*;

    #[test]
    fn replay_cache_is_bounded_and_matches_exact_content() {
        let mut cache = ReplayCache::default();
        let content = |n: usize| {
            vec![ContentBlock::Text {
                text: format!("turn {n}"),
            }]
        };
        for n in 0..=REPLAY_CAPACITY {
            cache.insert(&content(n), vec![json!(n)]);
        }
        let message = |n| Message {
            role: Role::Assistant,
            content: content(n),
        };
        assert_eq!(cache.entries.len(), REPLAY_CAPACITY);
        assert_eq!(cache.get(&message(0)), None, "oldest entry evicted");
        assert_eq!(
            cache.get(&message(REPLAY_CAPACITY)),
            Some(vec![json!(REPLAY_CAPACITY)])
        );
    }

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
