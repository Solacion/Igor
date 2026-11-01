//! The legacy HTTP+SSE transport (MCP 2024-11-05), for servers that predate Streamable HTTP.
//!
//! IGOR opens a GET event stream at the server URL. The server's first `endpoint` event names
//! where IGOR POSTs its messages; replies and server requests arrive as `message` events on the
//! stream. The endpoint must be on the same origin as the URL, so headers are never sent to
//! another address.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap};
use rmcp::RoleClient;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use sse_stream::SseStream;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Messages buffered from the event stream before the session reads them.
const INCOMING_CAPACITY: usize = 64;

/// A failure of the legacy SSE transport. Never carries header values.
#[derive(Debug, Error)]
pub enum SseError {
    /// An HTTP request failed or was refused.
    #[error("{0}")]
    Http(String),
    /// The server's event stream did not follow the protocol.
    #[error("{0}")]
    Protocol(String),
}

/// A connected legacy SSE session: messages are POSTed to `endpoint` and read from the stream.
pub(crate) struct LegacySse {
    client: reqwest::Client,
    endpoint: url::Url,
    headers: HeaderMap,
    incoming: mpsc::Receiver<RxJsonRpcMessage<RoleClient>>,
    reader: JoinHandle<()>,
}

impl LegacySse {
    /// Opens the event stream at `url` with `headers` and waits, up to `timeout`, for the
    /// server to name its message endpoint.
    pub(crate) async fn connect(
        client: reqwest::Client,
        url: &str,
        headers: HeaderMap,
        timeout: Duration,
    ) -> Result<Self, SseError> {
        let base = url::Url::parse(url).map_err(|error| SseError::Protocol(error.to_string()))?;
        let response = client
            .get(base.clone())
            .headers(headers.clone())
            .header(ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(|error| SseError::Http(error.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(SseError::Http(format!(
                "the server answered {} to the event stream request",
                response.status()
            )));
        }
        let mut events = SseStream::from_bytes_stream(response.bytes_stream());
        let endpoint = tokio::time::timeout(timeout, async {
            while let Some(event) = events.next().await {
                let event =
                    event.map_err(|_| SseError::Protocol("unreadable event stream".to_owned()))?;
                if event.event.as_deref() == Some("endpoint") {
                    return Ok(event.data.unwrap_or_default());
                }
            }
            Err(SseError::Protocol(
                "the event stream ended before naming a message endpoint".to_owned(),
            ))
        })
        .await
        .map_err(|_| {
            SseError::Protocol("no message endpoint within the startup time".to_owned())
        })??;
        let endpoint = base
            .join(endpoint.trim())
            .map_err(|_| SseError::Protocol("the message endpoint is not a URL".to_owned()))?;
        if endpoint.origin() != base.origin() {
            return Err(SseError::Protocol(
                "the message endpoint is on another host; headers are only sent to the configured server"
                    .to_owned(),
            ));
        }
        let (sender, incoming) = mpsc::channel(INCOMING_CAPACITY);
        let reader = tokio::spawn(async move {
            while let Some(Ok(event)) = events.next().await {
                if !matches!(event.event.as_deref(), None | Some("message")) {
                    continue;
                }
                let Some(data) = event.data else { continue };
                // A malformed message is skipped; the request it answered times out.
                if let Ok(message) = serde_json::from_str(&data)
                    && sender.send(message).await.is_err()
                {
                    break;
                }
            }
        });
        Ok(Self {
            client,
            endpoint,
            headers,
            incoming,
            reader,
        })
    }
}

impl Transport<RoleClient> for LegacySse {
    type Error = SseError;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let request = self
            .client
            .post(self.endpoint.clone())
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .json(&item);
        async move {
            let response = request
                .send()
                .await
                .map_err(|error| SseError::Http(error.without_url().to_string()))?;
            if response.status().is_success() {
                Ok(())
            } else {
                Err(SseError::Http(format!(
                    "the server answered {} to a message",
                    response.status()
                )))
            }
        }
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
        self.incoming.recv()
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.reader.abort();
        self.incoming.close();
        Ok(())
    }
}

impl Drop for LegacySse {
    fn drop(&mut self) {
        self.reader.abort();
    }
}
