//! Mapping of HTTP and transport failures to [`ProviderError`].
//!
//! Every message is scrubbed of the API key, when one is configured, and bounded in length before it leaves the adapter.

use igor_config::REDACTED;
use igor_core::provider::{ProviderError, ProviderErrorKind};
use serde_json::Value;

/// Maximum length, in characters, of a provider error message.
const MAX_MESSAGE_CHARS: usize = 300;

/// Builds an error, scrubbing `secret` from the message and bounding its length.
pub(crate) fn error(
    kind: ProviderErrorKind,
    retryable: bool,
    message: &str,
    secret: &str,
) -> ProviderError {
    let scrubbed = if secret.is_empty() {
        message.to_owned()
    } else {
        message.replace(secret, REDACTED)
    };
    let mut bounded: String = scrubbed.chars().take(MAX_MESSAGE_CHARS).collect();
    if bounded.len() < scrubbed.len() {
        bounded.push('…');
    }
    ProviderError {
        kind,
        message: bounded,
        retryable,
    }
}

/// Maps a non-success HTTP response. 429, 408, 409 and every 5xx (including 529 overloaded) are
/// retryable; authentication, permission and request errors are not.
pub(crate) fn status_error(
    status: u16,
    retry_after: Option<&str>,
    body: &[u8],
    secret: &str,
) -> ProviderError {
    let (kind, retryable) = match status {
        401 | 403 => (ProviderErrorKind::Authentication, false),
        408 => (ProviderErrorKind::Timeout, true),
        409 => (ProviderErrorKind::Unavailable, true),
        429 => (ProviderErrorKind::RateLimited, true),
        500..=599 => (ProviderErrorKind::Unavailable, true),
        _ => (ProviderErrorKind::InvalidRequest, false),
    };
    let detail = serde_json::from_slice::<Value>(body).ok();
    let api_error = detail.as_ref().and_then(|value| value.get("error"));
    let error_type = api_error
        .and_then(|error| error.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("unknown_error");
    // OpenAI-compatible servers send `{"error": {"message": ...}}`; some, such as LM Studio
    // for some failures, send `{"error": "..."}`.
    let api_message = api_error
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or(error.as_str())
        })
        .unwrap_or("no error message");
    let mut message = format!("HTTP {status} {error_type}: {api_message}");
    if let Some(seconds) = retry_after.filter(|value| !value.is_empty()) {
        message.push_str(&format!(" (retry after {seconds}s)"));
    }
    error(kind, retryable, &message, secret)
}

/// Maps a transport failure. Timeouts and connection failures are retryable: a model call has
/// no external effect.
pub(crate) fn transport_error(err: &reqwest::Error, secret: &str) -> ProviderError {
    if err.is_timeout() {
        error(
            ProviderErrorKind::Timeout,
            true,
            "request timed out",
            secret,
        )
    } else {
        let message = format!("request failed: {err}");
        error(ProviderErrorKind::Unavailable, true, &message, secret)
    }
}

/// The request was cancelled through its cancellation token.
pub(crate) fn cancelled() -> ProviderError {
    error(ProviderErrorKind::Cancelled, false, "request cancelled", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_kinds_and_retryability() {
        let cases = [
            (400, ProviderErrorKind::InvalidRequest, false),
            (401, ProviderErrorKind::Authentication, false),
            (402, ProviderErrorKind::InvalidRequest, false),
            (403, ProviderErrorKind::Authentication, false),
            (404, ProviderErrorKind::InvalidRequest, false),
            (408, ProviderErrorKind::Timeout, true),
            (413, ProviderErrorKind::InvalidRequest, false),
            (429, ProviderErrorKind::RateLimited, true),
            (500, ProviderErrorKind::Unavailable, true),
            (503, ProviderErrorKind::Unavailable, true),
            (529, ProviderErrorKind::Unavailable, true),
        ];
        for (status, kind, retryable) in cases {
            let err = status_error(status, None, b"", "");
            assert_eq!((err.kind, err.retryable), (kind, retryable), "{status}");
        }
    }

    #[test]
    fn message_names_status_type_and_retry_after() {
        let body = br#"{"error":{"type":"rate_limit_error","message":"Slow down"}}"#;
        let err = status_error(429, Some("12"), body, "");
        assert_eq!(
            err.message,
            "HTTP 429 rate_limit_error: Slow down (retry after 12s)"
        );
    }

    #[test]
    fn plain_string_errors_are_read() {
        let err = status_error(400, None, br#"{"error":"Model not loaded"}"#, "");
        assert_eq!(err.message, "HTTP 400 unknown_error: Model not loaded");
    }

    #[test]
    fn messages_are_scrubbed_and_bounded() {
        let secret = "IGOR-TEST-SECRET-7f3a";
        let long = format!("{secret} {}", "x".repeat(1000));
        let err = error(ProviderErrorKind::Unavailable, true, &long, secret);
        assert!(!err.message.contains(secret));
        assert!(err.message.starts_with(REDACTED));
        assert_eq!(err.message.chars().count(), MAX_MESSAGE_CHARS + 1);
    }
}
