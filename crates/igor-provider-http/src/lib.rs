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

/// Largest successful response body accepted, in bytes.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Builds the HTTP client an adapter uses. Redirects are never followed: an adapter's custom
/// key header (such as `x-api-key`) would otherwise be forwarded to whatever host a base URL
/// redirects to.
pub fn client(request_timeout: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(request_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// How much of an error response body is read to describe the error, in bytes.
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Sends a JSON request and returns the body of a successful response.
///
/// The status is classified before the body is accepted: an error response keeps its status
/// classification (and retryability) even when its body is large, because only the first
/// [`MAX_ERROR_BODY_BYTES`] are read to describe it. A successful body above
/// [`MAX_RESPONSE_BYTES`] is refused as malformed. `secret` is scrubbed from every message.
pub async fn send_json(
    request: reqwest::RequestBuilder,
    body: String,
    secret: &str,
) -> Result<Vec<u8>, ProviderError> {
    let response = request
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| errors::transport_error(&err, secret))?;
    let status = response.status();
    if status.is_success() {
        return match read_capped(response, MAX_RESPONSE_BYTES, secret).await? {
            (bytes, false) => Ok(bytes),
            (_, true) => Err(errors::error(
                ProviderErrorKind::MalformedResponse,
                false,
                &format!("response body exceeds {MAX_RESPONSE_BYTES} bytes"),
                secret,
            )),
        };
    }
    let retry_after = retry_after(&response);
    // The status decides kind and retryability. A truncated body is not valid JSON, and a body
    // that fails to arrive adds nothing: describe either by the status alone.
    let bytes = match read_capped(response, MAX_ERROR_BODY_BYTES, secret).await {
        Ok((bytes, false)) => bytes,
        Ok((_, true)) | Err(_) => Vec::new(),
    };
    Err(errors::status_error(
        status.as_u16(),
        retry_after.as_deref(),
        &bytes,
        secret,
    ))
}

/// Reads at most `cap` bytes of the body; the flag is true when the body was longer.
async fn read_capped(
    mut response: reqwest::Response,
    cap: usize,
    secret: &str,
) -> Result<(Vec<u8>, bool), ProviderError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| errors::transport_error(&err, secret))?
    {
        if bytes.len() + chunk.len() > cap {
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, false))
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
fn retry_after(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
