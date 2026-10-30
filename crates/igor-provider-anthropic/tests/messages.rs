//! Adapter tests against a loopback HTTP stub that replays Messages API responses.
//!
//! Fixtures in `tests/fixtures/` follow the documented response shapes and contain no key or
//! account identifiers. No test reaches the network.

use std::error::Error;
use std::time::Duration;

use igor_config::Secret;
use igor_core::CancellationToken;
use igor_core::provider::{
    ContentBlock, Message, ModelRequest, Provider, ProviderError, ProviderErrorKind, Role,
    StopReason, ToolSpec,
};
use igor_provider_anthropic::{API_VERSION, AnthropicProvider, AnthropicSettings};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

type TestResult = Result<(), Box<dyn Error>>;

const SENTINEL_KEY: &str = "IGOR-TEST-SECRET-7f3a";

/// What the stub does with each accepted connection, in order.
enum Reply {
    Http {
        status: u16,
        headers: Vec<(&'static str, &'static str)>,
        body: String,
    },
    /// Read the request, then close the connection without answering.
    Close,
    /// Read the request, then never answer.
    Hang,
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap_or_else(|err| format!("missing fixture {name}: {err}"))
}

fn ok(name: &str) -> Reply {
    Reply::Http {
        status: 200,
        headers: Vec::new(),
        body: fixture(name),
    }
}

fn status(status: u16, name: &str) -> Reply {
    Reply::Http {
        status,
        headers: Vec::new(),
        body: fixture(name),
    }
}

/// A received request: header lines (lowercased names) and the JSON body.
#[derive(Debug)]
struct Received {
    head: String,
    body: Value,
}

impl Received {
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }
}

struct Stub {
    base_url: String,
    task: JoinHandle<Vec<Received>>,
}

impl Stub {
    async fn start(replies: Vec<Reply>) -> Result<Self, Box<dyn Error>> {
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

    async fn received(self) -> Result<Vec<Received>, Box<dyn Error>> {
        Ok(self.task.await?)
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

fn provider(base_url: &str, timeout: Duration) -> Result<AnthropicProvider, Box<dyn Error>> {
    let mut settings = AnthropicSettings::new(base_url, "fixture-model", Secret::new(SENTINEL_KEY));
    settings.request_timeout = timeout;
    Ok(AnthropicProvider::new(settings)?)
}

fn request() -> ModelRequest {
    ModelRequest {
        system: "Ask before assuming dates.".into(),
        messages: vec![Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "Create an order for supplier X".into(),
            }],
        }],
        tools: vec![ToolSpec {
            name: "sales__get_sales_history".into(),
            description: "Sales history".into(),
            input_schema: json!({ "type": "object" }),
        }],
        max_tokens: 2048,
    }
}

async fn complete_once(reply: Reply) -> Result<Result<StopReason, ProviderError>, Box<dyn Error>> {
    let stub = Stub::start(vec![reply]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    let result = provider
        .complete(request(), CancellationToken::new())
        .await
        .map(|response| response.stop_reason);
    stub.task.abort();
    Ok(result)
}

#[tokio::test]
async fn text_turn_round_trip_sends_headers_and_body() -> TestResult {
    let stub = Stub::start(vec![ok("text_turn.json")]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    let response = provider
        .complete(request(), CancellationToken::new())
        .await?;
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(
        response.content,
        [ContentBlock::Text {
            text: "Which order date should I use?".into()
        }]
    );

    let received = stub.received().await?;
    let sent = received.first().ok_or("no request received")?;
    assert!(sent.head.starts_with("POST /v1/messages HTTP/1.1"));
    assert_eq!(sent.header("x-api-key"), Some(SENTINEL_KEY));
    assert_eq!(sent.header("anthropic-version"), Some(API_VERSION));
    assert_eq!(sent.body["model"], "fixture-model");
    assert_eq!(sent.body["max_tokens"], 2048);
    assert_eq!(sent.body["system"], "Ask before assuming dates.");
    assert_eq!(sent.body["tools"][0]["name"], "sales__get_sales_history");
    Ok(())
}

#[tokio::test]
async fn several_tool_calls_in_one_turn_and_thinking_replay() -> TestResult {
    let stub = Stub::start(vec![ok("tool_use_turn.json"), ok("text_turn.json")]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;

    let first = provider
        .complete(request(), CancellationToken::new())
        .await?;
    assert_eq!(first.stop_reason, StopReason::ToolUse);
    let calls: Vec<(&str, &Value)> = first
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        [
            (
                "sales__get_sales_history",
                &json!({ "supplier": "X", "from": "2026-10-01", "to": "2026-10-31" })
            ),
            (
                "weather__get_forecast",
                &json!({ "location": "Cape Town", "date": "2026-11-03" })
            ),
        ]
    );
    assert_eq!(first.content.len(), 3, "thinking block is not exposed");

    // The session sends the assistant turn back unchanged, plus the tool results.
    let mut next = request();
    next.messages.push(Message {
        role: Role::Assistant,
        content: first.content.clone(),
    });
    next.messages.push(Message {
        role: Role::User,
        content: vec![
            ContentBlock::ToolResult {
                tool_use_id: "toolu_fixture_1".into(),
                content: "{\"units\": 120}".into(),
                is_error: false,
            },
            ContentBlock::ToolResult {
                tool_use_id: "toolu_fixture_2".into(),
                content: "no_forecast".into(),
                is_error: true,
            },
        ],
    });
    provider.complete(next, CancellationToken::new()).await?;

    let received = stub.received().await?;
    let second = received.get(1).ok_or("second request missing")?;
    let assistant = &second.body["messages"][1];
    assert_eq!(assistant["role"], "assistant");
    assert_eq!(
        assistant["content"][0],
        json!({ "type": "thinking", "thinking": "", "signature": "fixture-signature" }),
        "thinking block is sent back unchanged"
    );
    assert_eq!(assistant["content"].as_array().map(Vec::len), Some(4));
    let results = &second.body["messages"][2]["content"];
    assert_eq!(results[1]["type"], "tool_result");
    assert_eq!(results[1]["is_error"], true);
    Ok(())
}

#[tokio::test]
async fn http_errors_map_to_core_errors() -> TestResult {
    let cases = [
        (
            status(401, "error_401.json"),
            ProviderErrorKind::Authentication,
            false,
            "authentication_error",
        ),
        (
            Reply::Http {
                status: 429,
                headers: vec![("retry-after", "17")],
                body: fixture("error_429.json"),
            },
            ProviderErrorKind::RateLimited,
            true,
            "retry after 17s",
        ),
        (
            status(529, "error_529.json"),
            ProviderErrorKind::Unavailable,
            true,
            "overloaded_error",
        ),
        (
            status(500, "error_500.json"),
            ProviderErrorKind::Unavailable,
            true,
            "api_error",
        ),
        (
            ok("malformed_tool_input.json"),
            ProviderErrorKind::MalformedResponse,
            false,
            "not a JSON object",
        ),
        (
            Reply::Close,
            ProviderErrorKind::Unavailable,
            true,
            "request failed",
        ),
    ];
    for (reply, kind, retryable, text) in cases {
        let err = complete_once(reply)
            .await?
            .err()
            .ok_or("expected an error")?;
        assert_eq!((err.kind, err.retryable), (kind, retryable), "{err}");
        assert!(err.message.contains(text), "{err}");
    }
    Ok(())
}

#[tokio::test]
async fn hanging_server_times_out() -> TestResult {
    let stub = Stub::start(vec![Reply::Hang]).await?;
    let provider = provider(&stub.base_url, Duration::from_millis(200))?;
    let err = provider
        .complete(request(), CancellationToken::new())
        .await
        .err()
        .ok_or("expected a timeout")?;
    stub.task.abort();
    assert_eq!(
        (err.kind, err.retryable),
        (ProviderErrorKind::Timeout, true)
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_aborts_an_in_flight_request() -> TestResult {
    let stub = Stub::start(vec![Reply::Hang]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(30))?;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        trigger.cancel();
    });
    let err = provider
        .complete(request(), cancel)
        .await
        .err()
        .ok_or("expected cancellation")?;
    stub.task.abort();
    assert_eq!(
        (err.kind, err.retryable),
        (ProviderErrorKind::Cancelled, false)
    );
    Ok(())
}

#[tokio::test]
async fn already_cancelled_request_is_not_sent() -> TestResult {
    let stub = Stub::start(vec![ok("text_turn.json")]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = provider.complete(request(), cancel).await.err();
    assert_eq!(err.map(|e| e.kind), Some(ProviderErrorKind::Cancelled));
    stub.task.abort();
    Ok(())
}

#[tokio::test]
async fn api_key_never_appears_in_errors() -> TestResult {
    let echo = format!(
        r#"{{"type":"error","error":{{"type":"authentication_error","message":"bad key {SENTINEL_KEY}"}}}}"#
    );
    let err = complete_once(Reply::Http {
        status: 401,
        headers: Vec::new(),
        body: echo,
    })
    .await?
    .err()
    .ok_or("expected an error")?;
    for output in [err.to_string(), format!("{err:?}"), err.message.clone()] {
        assert!(!output.contains(SENTINEL_KEY), "{output}");
    }
    assert!(err.message.contains(igor_config::REDACTED));
    Ok(())
}
