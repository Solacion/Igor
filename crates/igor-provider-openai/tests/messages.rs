//! Adapter tests against a loopback HTTP stub that replays chat completions responses.
//!
//! Fixtures in `tests/fixtures/` follow the OpenAI-compatible response shape served by LM Studio
//! and contain no key or account identifiers. No test reaches the network. The malformed cases
//! follow the testing review on #14; cases that the registry, approval gate or session own
//! (schema validation, unknown names, loops, write approval) are shown here only as
//! pass-through: the adapter returns what the model produced and fabricates nothing.

use std::error::Error;
use std::time::Duration;

use igor_config::Secret;
use igor_core::CancellationToken;
use igor_core::provider::{
    ContentBlock, Message, ModelRequest, ModelResponse, Provider, ProviderError, ProviderErrorKind,
    Role, StopReason, ToolSpec,
};
use igor_provider_openai::{OpenAiCompatibleProvider, OpenAiSettings};
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

fn provider(
    base_url: &str,
    timeout: Duration,
    api_key: Option<&str>,
) -> Result<OpenAiCompatibleProvider, Box<dyn Error>> {
    let mut settings = OpenAiSettings::new(format!("{base_url}/v1"), "fixture-model");
    settings.api_key = api_key.map(Secret::new);
    settings.request_timeout = timeout;
    Ok(OpenAiCompatibleProvider::new(settings)?)
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
            name: "sales__get_stock_levels".into(),
            description: "Stock levels".into(),
            input_schema: json!({ "type": "object" }),
        }],
        max_tokens: 2048,
    }
}

async fn complete_once(
    reply: Reply,
) -> Result<Result<ModelResponse, ProviderError>, Box<dyn Error>> {
    let stub = Stub::start(vec![reply]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5), None)?;
    let result = provider.complete(request(), CancellationToken::new()).await;
    stub.task.abort();
    Ok(result)
}

async fn malformed(name: &str) -> Result<ProviderError, Box<dyn Error>> {
    let err = complete_once(ok(name))
        .await?
        .err()
        .ok_or("expected an error")?;
    assert_eq!(
        (err.kind, err.retryable),
        (ProviderErrorKind::MalformedResponse, false),
        "{name}: {err}"
    );
    Ok(err)
}

#[tokio::test]
async fn text_turn_round_trip_without_a_key() -> TestResult {
    let stub = Stub::start(vec![ok("text_turn.json")]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5), None)?;
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
    assert!(sent.head.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert_eq!(sent.header("authorization"), None);
    assert_eq!(sent.body["model"], "fixture-model");
    assert_eq!(sent.body["messages"][0]["role"], "system");
    assert_eq!(
        sent.body["tools"][0]["function"]["name"],
        "sales__get_stock_levels"
    );
    Ok(())
}

#[tokio::test]
async fn optional_key_is_sent_as_bearer_token() -> TestResult {
    let stub = Stub::start(vec![ok("text_turn.json")]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5), Some(SENTINEL_KEY))?;
    provider
        .complete(request(), CancellationToken::new())
        .await?;
    let received = stub.received().await?;
    let expected = format!("Bearer {SENTINEL_KEY}");
    assert_eq!(
        received.first().and_then(|r| r.header("authorization")),
        Some(expected.as_str())
    );
    Ok(())
}

#[tokio::test]
async fn several_native_tool_calls_in_one_turn() -> TestResult {
    let response = complete_once(ok("tool_calls_turn.json")).await??;
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    let calls: Vec<(&str, &str, &Value)> = response
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => Some((id.as_str(), name.as_str(), input)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        [
            (
                "call_fixture_1",
                "sales__get_sales_history",
                &json!({ "supplier": "X", "from": "2026-10-01", "to": "2026-10-31" })
            ),
            (
                "call_fixture_2",
                "weather__get_forecast",
                &json!({ "location": "Cape Town", "date": "2026-11-03" })
            ),
        ]
    );
    Ok(())
}

/// Review case 1: `arguments` is not valid JSON.
#[tokio::test]
async fn invalid_json_arguments_are_malformed() -> TestResult {
    let err = malformed("invalid_json_arguments.json").await?;
    assert!(err.message.contains("not valid JSON"), "{err}");
    Ok(())
}

/// Review case 1: `arguments` is JSON but not an object.
#[tokio::test]
async fn non_object_arguments_are_malformed() -> TestResult {
    let err = malformed("non_object_arguments.json").await?;
    assert!(err.message.contains("not a JSON object"), "{err}");
    Ok(())
}

#[tokio::test]
async fn tool_call_without_id_is_malformed() -> TestResult {
    malformed("missing_call_id.json").await?;
    Ok(())
}

/// Review case 4: a tool call written in the message text is not a tool call.
#[tokio::test]
async fn tool_call_written_in_text_stays_text() -> TestResult {
    let response = complete_once(ok("tool_call_in_text.json")).await??;
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(
        response
            .content
            .iter()
            .all(|block| matches!(block, ContentBlock::Text { .. })),
        "no ToolUse may be parsed out of text"
    );
    Ok(())
}

/// Review case 5: an empty response.
#[tokio::test]
async fn empty_responses_are_malformed() -> TestResult {
    malformed("empty_choices.json").await?;
    malformed("empty_message.json").await?;
    Ok(())
}

/// Review case 5: output cut at the token limit in the middle of a tool call.
#[tokio::test]
async fn truncated_tool_call_is_malformed() -> TestResult {
    malformed("truncated_call.json").await?;
    Ok(())
}

/// Review cases 2, 3 and 7: an unqualified name and schema-invalid arguments are returned
/// unchanged, so the registry rejects them before dispatch; the adapter neither validates,
/// repairs nor drops them, and approval stays the gate's job.
#[tokio::test]
async fn unvalidated_calls_pass_through_unchanged() -> TestResult {
    let response = complete_once(ok("unvalidated_call.json")).await??;
    let calls: Vec<(&str, &Value)> = response
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
            ("sales/create_draft_order", &json!({ "supplier": "X" })),
            ("sales__get_stock_levels", &json!({ "sku": 7 })),
        ]
    );
    Ok(())
}

#[tokio::test]
async fn http_and_transport_errors_map_to_core_errors() -> TestResult {
    let cases = [
        (
            status(400, "error_400_string.json"),
            ProviderErrorKind::InvalidRequest,
            false,
            "Model not loaded",
        ),
        (
            status(401, "error_401.json"),
            ProviderErrorKind::Authentication,
            false,
            "Invalid API key",
        ),
        (
            status(500, "error_500.json"),
            ProviderErrorKind::Unavailable,
            true,
            "server_error",
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
    let provider = provider(&stub.base_url, Duration::from_millis(200), None)?;
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
    let provider = provider(&stub.base_url, Duration::from_secs(30), None)?;
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
async fn api_key_never_appears_in_errors() -> TestResult {
    let stub = Stub::start(vec![Reply::Http {
        status: 401,
        headers: Vec::new(),
        body: format!(r#"{{"error":{{"message":"bad key {SENTINEL_KEY}"}}}}"#),
    }])
    .await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5), Some(SENTINEL_KEY))?;
    let err = provider
        .complete(request(), CancellationToken::new())
        .await
        .err()
        .ok_or("expected an error")?;
    for output in [err.to_string(), format!("{err:?}")] {
        assert!(!output.contains(SENTINEL_KEY), "{output}");
    }
    Ok(())
}
