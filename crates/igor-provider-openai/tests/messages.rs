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
use igor_core::provider::{ContentBlock, Provider, ProviderError, ProviderErrorKind, StopReason};
use igor_provider_http::stub::{
    Reply, SENTINEL_KEY, Stub, check_cancellation_aborts_an_in_flight_request,
    check_hanging_server_times_out, complete_once, ok, sample_request, status,
};
use igor_provider_openai::{OpenAiCompatibleProvider, OpenAiSettings};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

const DIR: &str = env!("CARGO_MANIFEST_DIR");

fn provider_with_key(
    base_url: &str,
    timeout: Duration,
    api_key: Option<&str>,
) -> Result<OpenAiCompatibleProvider, Box<dyn Error>> {
    let mut settings = OpenAiSettings::new(format!("{base_url}/v1"), "fixture-model");
    settings.api_key = api_key.map(Secret::new);
    settings.request_timeout = timeout;
    Ok(OpenAiCompatibleProvider::new(settings)?)
}

fn provider(base_url: &str, timeout: Duration) -> Result<OpenAiCompatibleProvider, Box<dyn Error>> {
    provider_with_key(base_url, timeout, None)
}

async fn malformed(name: &str) -> Result<ProviderError, Box<dyn Error>> {
    let err = complete_once(&provider, ok(DIR, name))
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
    let stub = Stub::start(vec![ok(DIR, "text_turn.json")]).await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    let response = provider
        .complete(sample_request(), CancellationToken::new())
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
        "sales__get_sales_history"
    );
    Ok(())
}

#[tokio::test]
async fn optional_key_is_sent_as_bearer_token() -> TestResult {
    let stub = Stub::start(vec![ok(DIR, "text_turn.json")]).await?;
    let provider = provider_with_key(&stub.base_url, Duration::from_secs(5), Some(SENTINEL_KEY))?;
    provider
        .complete(sample_request(), CancellationToken::new())
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
    let response = complete_once(&provider, ok(DIR, "tool_calls_turn.json")).await??;
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
    let response = complete_once(&provider, ok(DIR, "tool_call_in_text.json")).await??;
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
    let response = complete_once(&provider, ok(DIR, "unvalidated_call.json")).await??;
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
            status(400, DIR, "error_400_string.json"),
            ProviderErrorKind::InvalidRequest,
            false,
            "Model not loaded",
        ),
        (
            status(401, DIR, "error_401.json"),
            ProviderErrorKind::Authentication,
            false,
            "Invalid API key",
        ),
        (
            status(500, DIR, "error_500.json"),
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
        let err = complete_once(&provider, reply)
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
    check_hanging_server_times_out(&provider).await
}

#[tokio::test]
async fn cancellation_aborts_an_in_flight_request() -> TestResult {
    check_cancellation_aborts_an_in_flight_request(&provider).await
}

#[tokio::test]
async fn api_key_never_appears_in_errors() -> TestResult {
    let stub = Stub::start(vec![Reply::Http {
        status: 401,
        headers: Vec::new(),
        body: format!(r#"{{"error":{{"message":"bad key {SENTINEL_KEY}"}}}}"#),
    }])
    .await?;
    let provider = provider_with_key(&stub.base_url, Duration::from_secs(5), Some(SENTINEL_KEY))?;
    let err = provider
        .complete(sample_request(), CancellationToken::new())
        .await
        .err()
        .ok_or("expected an error")?;
    for output in [err.to_string(), format!("{err:?}")] {
        assert!(!output.contains(SENTINEL_KEY), "{output}");
    }
    Ok(())
}

// Tool-support preflight against LM Studio's model details endpoint.

async fn preflight_once(
    reply: Reply,
) -> Result<(Result<(), ProviderError>, String), Box<dyn Error>> {
    let stub = Stub::start(vec![reply]).await?;
    let provider = provider_with_key(&stub.base_url, Duration::from_secs(5), Some(SENTINEL_KEY))?;
    let result = provider.preflight(CancellationToken::new()).await;
    let received = stub.received().await?;
    let head = received.first().map(|r| r.head.clone()).unwrap_or_default();
    Ok((result, head))
}

#[tokio::test]
async fn preflight_asks_lm_studio_for_the_models_capabilities() -> TestResult {
    let (result, head) = preflight_once(ok(DIR, "details_tool_use.json")).await?;
    assert_eq!(result, Ok(()));
    assert!(
        head.starts_with("GET /api/v0/models/fixture-model "),
        "{head}"
    );
    Ok(())
}

#[tokio::test]
async fn a_model_without_tool_use_is_reported_as_unable_to_call_tools() -> TestResult {
    let (result, _) = preflight_once(ok(DIR, "details_no_tool_use.json")).await?;
    let err = result.err().ok_or("expected ToolsUnsupported")?;
    assert_eq!(
        (err.kind, err.retryable),
        (ProviderErrorKind::ToolsUnsupported, false)
    );
    assert!(err.message.contains("\"fixture-model\""), "{err}");
    assert!(!err.message.contains(SENTINEL_KEY));
    Ok(())
}

#[tokio::test]
async fn preflight_treats_anything_but_lm_studio_details_as_unknown() -> TestResult {
    let other_model = json!({ "id": "other", "object": "model", "type": "llm" }).to_string();
    for reply in [
        status(400, DIR, "details_not_found.json"),
        status(404, DIR, "error_401.json"),
        Reply::Http {
            status: 200,
            headers: Vec::new(),
            body: other_model,
        },
        Reply::Http {
            status: 200,
            headers: Vec::new(),
            body: "not json".to_owned(),
        },
        Reply::Close,
    ] {
        let (result, _) = preflight_once(reply).await?;
        assert_eq!(result, Ok(()));
    }
    Ok(())
}

#[tokio::test]
async fn preflight_is_skipped_when_the_base_url_does_not_end_in_v1() -> TestResult {
    let stub = Stub::start(Vec::new()).await?;
    let settings = OpenAiSettings::new(format!("{}/openai", stub.base_url), "fixture-model");
    let provider = OpenAiCompatibleProvider::new(settings)?;
    assert_eq!(provider.preflight(CancellationToken::new()).await, Ok(()));
    assert_eq!(stub.accepted(), 0);
    stub.abort();
    Ok(())
}

// Context check against the window LM Studio loaded the model with.

/// A request of about 10,000 prompt tokens: more than the 8,192 of `details_tool_use.json`.
fn long_request() -> igor_core::provider::ModelRequest {
    let mut request = sample_request();
    request.system = "x".repeat(40_000);
    request
}

#[tokio::test]
async fn a_conversation_too_long_for_the_loaded_context_is_refused() -> TestResult {
    let stub = Stub::start(vec![
        ok(DIR, "details_tool_use.json"),
        ok(DIR, "details_tool_use.json"),
    ])
    .await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    provider.preflight(CancellationToken::new()).await?;
    let err = provider
        .complete(long_request(), CancellationToken::new())
        .await
        .err()
        .ok_or("expected the request to be refused")?;
    assert_eq!(
        (err.kind, err.retryable),
        (ProviderErrorKind::InvalidRequest, false)
    );
    assert!(err.message.contains("loaded with 8192"), "{err}");
    assert!(err.message.contains("larger context length"), "{err}");
    let received = stub.received().await?;
    assert!(
        received
            .iter()
            .all(|request| !request.head.starts_with("POST")),
        "the request must not be sent"
    );
    Ok(())
}

#[tokio::test]
async fn a_model_reloaded_with_a_larger_context_is_asked_again() -> TestResult {
    let stub = Stub::start(vec![
        ok(DIR, "details_tool_use.json"),
        ok(DIR, "details_large_context.json"),
        ok(DIR, "text_turn.json"),
    ])
    .await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    provider.preflight(CancellationToken::new()).await?;
    let response = provider
        .complete(long_request(), CancellationToken::new())
        .await?;
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    Ok(())
}

#[tokio::test]
async fn a_conversation_that_fits_is_sent_without_asking_again() -> TestResult {
    let stub = Stub::start(vec![
        ok(DIR, "details_tool_use.json"),
        ok(DIR, "text_turn.json"),
    ])
    .await?;
    let provider = provider(&stub.base_url, Duration::from_secs(5))?;
    provider.preflight(CancellationToken::new()).await?;
    provider
        .complete(sample_request(), CancellationToken::new())
        .await?;
    assert_eq!(stub.accepted(), 2);
    Ok(())
}
