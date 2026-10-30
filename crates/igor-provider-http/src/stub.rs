//! Loopback HTTP stub for adapter tests. Enabled by the `test-stub` feature.
//!
//! Serves one scripted reply per accepted connection and records each request, so adapter tests
//! never reach the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    accepted: Arc<AtomicUsize>,
    task: JoinHandle<Vec<Received>>,
}

impl Stub {
    /// Binds a free loopback port and starts serving `replies` in order.
    pub async fn start(replies: Vec<Reply>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        let task = tokio::spawn(async move {
            let mut received = Vec::new();
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                counter.fetch_add(1, Ordering::SeqCst);
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
        Ok(Self {
            base_url,
            accepted,
            task,
        })
    }

    /// Number of connections accepted so far, without waiting.
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
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

/// Reads `tests/fixtures/<name>` under a crate's manifest directory.
// A missing fixture is a broken test setup: fail loudly with its path. Test-only code.
#[allow(clippy::panic)]
pub fn fixture(manifest_dir: &str, name: &str) -> String {
    let path = format!("{manifest_dir}/tests/fixtures/{name}");
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read fixture {path}: {err}"))
}

/// A `200 OK` reply with a fixture body.
pub fn ok(manifest_dir: &str, name: &str) -> Reply {
    status(200, manifest_dir, name)
}

/// A reply with `status` and a fixture body.
pub fn status(status: u16, manifest_dir: &str, name: &str) -> Reply {
    Reply::Http {
        status,
        headers: Vec::new(),
        body: fixture(manifest_dir, name),
    }
}
