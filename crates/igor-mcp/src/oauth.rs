//! OAuth 2.1 for remote servers: signing in through the system browser, and keeping the tokens
//! in a host-provided [`TokenStore`].
//!
//! [`sign_in`] discovers the server's authorization server, registers IGOR as a client when the
//! server allows it, opens the authorization page with PKCE, and waits for the browser to come
//! back to a one-off loopback address. [`crate::McpBackend::connect_http`] then uses the stored
//! tokens and refreshes them when they expire. Tokens never appear in `Debug` output or errors.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState,
    StoredCredentials,
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// How long [`sign_in`] waits for the browser by default.
pub const DEFAULT_SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// Largest request IGOR reads from the browser on the loopback address.
const MAX_CALLBACK_BYTES: usize = 16 * 1024;

/// Where one server's OAuth credentials are kept, as an opaque text value. Hosts keep it in the
/// Keychain. The value holds tokens: never log or display it.
pub trait TokenStore: Send + Sync {
    /// The stored value, or `None` when the operator has not signed in.
    fn load(&self) -> Result<Option<String>, String>;
    /// Replaces the stored value.
    fn save(&self, value: &str) -> Result<(), String>;
    /// Removes the stored value.
    fn clear(&self) -> Result<(), String>;
}

/// Adapts a [`TokenStore`] to rmcp's credential store.
pub(crate) struct Credentials(pub(crate) Arc<dyn TokenStore>);

#[async_trait]
impl CredentialStore for Credentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        match self.0.load().map_err(AuthError::CredentialStoreError)? {
            // Unreadable credentials are treated as signed out, so the operator can sign in again.
            Some(text) => Ok(serde_json::from_str(&text).ok()),
            None => Ok(None),
        }
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let text = serde_json::to_string(&credentials)
            .map_err(|error| AuthError::CredentialStoreError(error.to_string()))?;
        self.0.save(&text).map_err(AuthError::CredentialStoreError)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.0.clear().map_err(AuthError::CredentialStoreError)
    }
}

/// An authorization manager for `url` that reads and refreshes tokens in `store`, or `None` when
/// the operator has not signed in.
pub(crate) async fn signed_in(
    url: &str,
    client: reqwest::Client,
    store: Arc<dyn TokenStore>,
) -> Result<Option<AuthorizationManager>, String> {
    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|error| error.to_string())?;
    manager
        .with_client(client)
        .map_err(|error| error.to_string())?;
    manager.set_credential_store(Credentials(store));
    match manager.initialize_from_store().await {
        Ok(true) => Ok(Some(manager)),
        Ok(false) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Signing in did not complete.
#[derive(Debug, Error)]
pub enum SignInError {
    /// The server or its authorization server refused or failed a step.
    #[error("sign-in failed: {0}")]
    Failed(String),
    /// The browser did not come back in time.
    #[error("sign-in was not completed within {0:?}")]
    TimedOut(Duration),
    /// The operator declined, or the authorization server reported an error.
    #[error("sign-in was declined: {0}")]
    Declined(String),
}

impl From<AuthError> for SignInError {
    fn from(error: AuthError) -> Self {
        Self::Failed(error.to_string())
    }
}

/// Signs in to the OAuth-protected server at `url` and saves the credentials in `store`.
///
/// `open` gets the authorization page to show in the operator's browser. Only `https` pages are
/// opened, or `http` on this machine. Redirects are not followed by IGOR's own requests.
pub async fn sign_in(
    url: &str,
    store: Arc<dyn TokenStore>,
    open: impl FnOnce(&str) -> Result<(), String>,
    timeout: Duration,
) -> Result<(), SignInError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| SignInError::Failed(format!("cannot listen for the browser: {error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| SignInError::Failed(error.to_string()))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| SignInError::Failed(error.to_string()))?;
    let mut state = OAuthState::new(url, Some(client)).await?;
    if let OAuthState::Unauthorized(manager) = &mut state {
        manager.set_credential_store(Credentials(store));
    }
    state
        .start_authorization(AuthorizationRequest::new(&redirect_uri).with_client_name("Igor"))
        .await?;
    let page = state.get_authorization_url().await?;
    if !opens_safely(&page) {
        return Err(SignInError::Failed(
            "the authorization page is not an https address".into(),
        ));
    }
    open(&page).map_err(SignInError::Failed)?;
    let callback = tokio::time::timeout(timeout, wait_for_callback(&listener))
        .await
        .map_err(|_| SignInError::TimedOut(timeout))??;
    match callback {
        Callback::Code { code, state: csrf } => {
            state.handle_callback(&code, &csrf).await?;
            Ok(())
        }
        Callback::Error(reason) => Err(SignInError::Declined(reason)),
    }
}

/// True for an `https` address, or `http` on this machine.
fn opens_safely(page: &str) -> bool {
    let Ok(url) = url::Url::parse(page) else {
        return false;
    };
    match url.scheme() {
        "https" => true,
        "http" => matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        _ => false,
    }
}

/// What the browser brought back.
enum Callback {
    Code { code: String, state: String },
    Error(String),
}

impl fmt::Debug for Callback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code { .. } => f.write_str("Code([REDACTED])"),
            Self::Error(reason) => f.debug_tuple("Error").field(reason).finish(),
        }
    }
}

/// Answers browser requests on `listener` until one reaches `/callback` with a code or an error.
async fn wait_for_callback(listener: &TcpListener) -> Result<Callback, SignInError> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| SignInError::Failed(error.to_string()))?;
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2048];
        while request.len() < MAX_CALLBACK_BYTES && !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }
        let target = String::from_utf8_lossy(&request)
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .map(str::to_owned);
        let callback = target.as_deref().and_then(parse_callback);
        let body = match &callback {
            Some(Callback::Code { .. }) => {
                "Signed in. You can close this window and return to Igor."
            }
            Some(Callback::Error(_)) => "Sign-in was not completed. You can close this window.",
            None => "Waiting for sign-in…",
        };
        let status = if callback.is_some() {
            "200 OK"
        } else {
            "404 Not Found"
        };
        let page = format!(
            "<!doctype html><meta charset=\"utf-8\"><title>Igor</title><p style=\"font-family:-apple-system,sans-serif\">{body}</p>"
        );
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let Some(callback) = callback {
            return Ok(callback);
        }
    }
}

/// The code and state, or the error, from a `/callback?…` request target.
fn parse_callback(target: &str) -> Option<Callback> {
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/callback" {
        return None;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(error) = param("error") {
        let description = param("error_description").unwrap_or_default();
        return Some(Callback::Error(
            format!("{error} {description}").trim().to_owned(),
        ));
    }
    Some(Callback::Code {
        code: param("code")?,
        state: param("state")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_carry_a_code_and_state_or_an_error() {
        assert!(matches!(
            parse_callback("/callback?code=abc&state=xyz"),
            Some(Callback::Code { code, state }) if code == "abc" && state == "xyz"
        ));
        assert!(matches!(
            parse_callback("/callback?error=access_denied&error_description=No+thanks"),
            Some(Callback::Error(reason)) if reason == "access_denied No thanks"
        ));
        assert!(parse_callback("/favicon.ico").is_none());
        assert!(parse_callback("/callback?code=abc").is_none());
    }

    #[test]
    fn only_https_or_local_pages_are_opened() {
        assert!(opens_safely("https://auth.example.com/authorize?x=1"));
        assert!(opens_safely("http://127.0.0.1:9000/authorize"));
        assert!(!opens_safely("http://auth.example.com/authorize"));
        assert!(!opens_safely("file:///etc/passwd"));
        assert!(!opens_safely("javascript:alert(1)"));
    }
}
