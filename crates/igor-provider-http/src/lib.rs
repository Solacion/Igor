//! HTTP helpers shared by IGOR model-provider adapters.
//!
//! Each adapter keeps its own wire format and API error shape; this crate holds what they have in
//! common: mapping HTTP and transport failures to [`ProviderError`], reading bounded response
//! bodies and racing a request against its cancellation token. Every error message is scrubbed of
//! the API key and bounded in length before it leaves an adapter.

pub mod errors;
#[cfg(feature = "test-stub")]
pub mod stub;

use std::future::Future;
use std::time::Duration;

use igor_core::CancellationToken;
use igor_core::provider::{ProviderError, ProviderErrorKind};

/// Default bound on one request, including reading the response. Non-streaming responses with a
/// large `max_tokens`, and local models, can take minutes.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Largest response body accepted, in bytes.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Reads the response body, refusing bodies above [`MAX_RESPONSE_BYTES`].
pub async fn read_body(
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

/// Runs `request` unless `cancel` fires first. A request already cancelled is never started.
pub async fn cancellable<F>(
    cancel: CancellationToken,
    request: F,
) -> Result<F::Output, ProviderError>
where
    F: Future,
{
    if cancel.is_cancelled() {
        return Err(errors::cancelled());
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(errors::cancelled()),
        output = request => Ok(output),
    }
}

/// Reads the `retry-after` header as text, when present.
pub fn retry_after(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
