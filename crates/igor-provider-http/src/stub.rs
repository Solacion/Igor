//! Loopback HTTP stub for adapter tests. Enabled by the `test-stub` feature.
//!
//! Serves one scripted reply per accepted connection and records each request, so adapter tests
//! never reach the network.

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// What the stub does with each accepted connection, in order.
pub enum Reply {
    /// Read the request, then answer with this HTTP response.
    Http {
        /// Status code.
        status: u16,
        /// Extra response headers.
        headers: Vec<(&'static str, &'static str)>,
        /// Response body.
        body: String,
    },
    /// Read the request, then close the connection without answering.
    Close,
    /// Read the request, then never answer.
    Hang,
}

/// A received request: its head (request line and headers) and JSON body.
#[derive(Debug)]
pub struct Received {
    /// Request line and header lines.
    pub head: String,
    /// Parsed JSON body.
    pub body: Value,
}

impl Received {
    /// Value of the first header called `name`, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }
}

/// A loopback HTTP server that answers each connection with the next [`Reply`].
pub struct Stub {
    /// `http://127.0.0.1:<port>`.
    pub base_url: String,
    task: JoinHandle<Vec<Received>>,
}

impl Stub {
    /// Binds a free loopback port and starts serving `replies` in order.
    pub async fn start(replies: Vec<Reply>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let task = tokio::spawn(async move {
            let mut received = Vec::new();
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                if let Some(request) = read_request(&mut stream).await {
                    received.push(request);
                }
                match reply {
                    Reply::Http {
                        status,
                        headers,
                        body,
                    } => {
                        let mut response = format!(
                            "HTTP/1.1 {status} STUB\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                            body.len()
                        );
                        for (name, value) in headers {
                            response.push_str(&format!("{name}: {value}\r\n"));
                        }
                        response.push_str("\r\n");
                        response.push_str(&body);
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    }
                    Reply::Close => drop(stream),
                    Reply::Hang => {
                        std::future::pending::<()>().await;
                    }
                }
            }
            received
        });
        Ok(Self { base_url, task })
    }

    /// Waits for every reply to be served and returns the requests received.
    pub async fn received(self) -> Result<Vec<Received>, tokio::task::JoinError> {
        self.task.await
    }

    /// Stops serving, e.g. after a test that leaves a [`Reply::Hang`] connection open.
    pub fn abort(&self) {
        self.task.abort();
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(buffer.get(..read)?);
        let text = String::from_utf8_lossy(&data);
        let Some(header_end) = text.find("\r\n\r\n") else {
            continue;
        };
        let head = text.get(..header_end)?.to_owned();
        let length: usize = head
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().ok())?
            })
            .unwrap_or(0);
        let body_start = header_end + 4;
        if data.len() >= body_start + length {
            let body = serde_json::from_slice(data.get(body_start..body_start + length)?).ok()?;
            return Some(Received { head, body });
        }
    }
}
