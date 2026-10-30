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
use igor_provider_anthropic::{
    API_VERSION, AnthropicProvider, AnthropicSettings, MAX_RESPONSE_BYTES,
};
use igor_provider_http::stub::{Reply, Stub};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

const SENTINEL_KEY: &str = "IGOR-TEST-SECRET-7f3a";

// A missing fixture is a broken test setup: fail loudly with its path.
#[allow(clippy::panic)]
fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read fixture {path}: {err}"))
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
    stub.abort();
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
async fn several_tool_calls_in_one_turn_drop_thinking_blocks() -> TestResult {
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
    let blocks = assistant["content"].as_array().ok_or("assistant content")?;
    assert_eq!(blocks.len(), 3, "the neutral turn is sent back as is");
    assert!(
        blocks.iter().all(|block| block["type"] != "thinking"),
        "thinking blocks are never replayed"
    );
    let results = &second.body["messages"][2]["content"];
    assert_eq!(results[1]["type"], "tool_result");
    assert_eq!(results[1]["is_error"], true);
    Ok(())
}

#[tokio::test]
async fn oversized_error_body_keeps_its_status() -> TestResult {
    let result = complete_once(Reply::Http {
        status: 429,
        headers: vec![("retry-after", "5")],
        body: "<html>".repeat(MAX_RESPONSE_BYTES / 6 + 1),
    })
    .await?;
    let err = result.err().ok_or("expected an error")?;
    assert_eq!(err.kind, ProviderErrorKind::RateLimited);
    assert!(
        err.retryable,
        "a rate limit stays retryable however large its body"
    );
    assert!(err.message.contains("retry after 5s"), "{}", err.message);
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
    stub.abort();
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
    stub.abort();
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
    stub.abort();
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
