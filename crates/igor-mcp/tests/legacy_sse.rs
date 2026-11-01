//! `McpBackend` against an in-process server speaking the legacy HTTP+SSE transport (2024-11-05).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use igor_core::CancellationToken;
use igor_core::tools::{ServerId, ToolBackend, ToolContent};
use igor_mcp::{ConnectError, HttpServerConfig, McpBackend};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const TOKEN: &str = "tok-legacy-sse-sentinel";

#[derive(Clone)]
struct Legacy {
    /// The open event stream's sender, once a client is connected.
    stream: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    /// What the `endpoint` event names.
    endpoint: &'static str,
}

fn authorized(request: &Request) -> bool {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        == Some(format!("Bearer {TOKEN}").as_str())
}

async fn events(State(legacy): State<Legacy>, request: Request) -> Response {
    if !authorized(&request) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (sender, receiver) = mpsc::unbounded_channel();
    let _ = sender.send(format!("event: endpoint\ndata: {}\n\n", legacy.endpoint));
    if let Ok(mut slot) = legacy.stream.lock() {
        *slot = Some(sender);
    }
    let body = futures_util::stream::unfold(receiver, |mut receiver| async move {
        let chunk = receiver.recv().await?;
        Some((Ok::<_, std::io::Error>(Bytes::from(chunk)), receiver))
    });
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(body),
    )
        .into_response()
}

async fn messages(State(legacy): State<Legacy>, request: Request) -> Response {
    if !authorized(&request) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let body = to_bytes(request.into_body(), 64 * 1024)
        .await
        .unwrap_or_default();
    let message: Value = serde_json::from_slice(&body).unwrap_or_default();
    let Some(id) = message.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match message.get("method").and_then(Value::as_str) {
        Some("initialize") => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "legacy", "version": "1.0.0" },
        }),
        Some("tools/list") => json!({
            "tools": [{ "name": "echo", "description": "Echoes its text", "inputSchema": { "type": "object" } }],
        }),
        Some("tools/call") => {
            let text = message
                .pointer("/params/arguments/text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        _ => json!({}),
    };
    let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    if let Ok(slot) = legacy.stream.lock()
        && let Some(sender) = slot.as_ref()
    {
        let _ = sender.send(format!("event: message\ndata: {reply}\n\n"));
    }
    StatusCode::ACCEPTED.into_response()
}

/// Serves a legacy SSE server whose endpoint event names `endpoint`; returns the stream URL.
async fn serve(endpoint: &'static str) -> Result<String, Box<dyn std::error::Error>> {
    let legacy = Legacy {
        stream: Arc::default(),
        endpoint,
    };
    let app = Router::new()
        .route("/sse", get(events))
        .route("/messages", post(messages))
        .with_state(legacy);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/sse", listener.local_addr()?);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(url)
}

fn config(url: &str) -> HttpServerConfig {
    let mut config = HttpServerConfig::new(url);
    config.legacy_sse = true;
    config.bearer_token = Some(TOKEN.to_owned());
    config.startup_timeout = Duration::from_secs(5);
    config.call_timeout = Duration::from_secs(5);
    config
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_and_calls_tools_over_legacy_sse() -> TestResult {
    let url = serve("/messages?session_id=1").await?;
    let backend = McpBackend::connect_http(ServerId::new("legacy")?, &config(&url)).await?;
    let names: Vec<String> = backend
        .list_tools()
        .await?
        .into_iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(names, ["legacy/echo"]);
    let output = backend
        .call_tool(
            "echo",
            Map::from_iter([("text".to_owned(), json!("hello"))]),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(
        output.content,
        [ToolContent::Text {
            text: "hello".into()
        }]
    );
    backend.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn refuses_an_endpoint_on_another_host_and_a_wrong_token() -> TestResult {
    let elsewhere = serve("http://attacker.example/messages").await?;
    let error = McpBackend::connect_http(ServerId::new("legacy")?, &config(&elsewhere))
        .await
        .err()
        .ok_or("connected to an endpoint on another host")?;
    assert!(error.to_string().contains("another host"), "{error}");

    let url = serve("/messages").await?;
    let mut wrong = config(&url);
    wrong.bearer_token = Some("tok-wrong-sse-sentinel".to_owned());
    let error = McpBackend::connect_http(ServerId::new("legacy")?, &wrong)
        .await
        .err()
        .ok_or("connected with the wrong token")?;
    assert!(matches!(error, ConnectError::Initialize { .. }), "{error}");
    assert!(!format!("{error} {error:?}").contains("tok-wrong-sse-sentinel"));
    Ok(())
}
