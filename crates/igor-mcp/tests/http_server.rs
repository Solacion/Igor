//! `McpBackend` against an in-process Streamable HTTP server that checks its headers.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use igor_core::CancellationToken;
use igor_core::tools::{BackendError, ServerId, ToolBackend, ToolContent};
use igor_mcp::{ConnectError, HttpServerConfig, McpBackend};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Map, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const API_KEY: &str = "key-7b1d-http-sentinel";
const TOKEN: &str = "tok-91c3-http-sentinel";

#[derive(Clone)]
struct Echo;

impl ServerHandler for Echo {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let schema = Arc::new(Map::from_iter([("type".to_owned(), json!("object"))]));
        Ok(ListToolsResult::with_all_items(vec![
            Tool::new("echo", "Echoes its text", Arc::clone(&schema)),
            Tool::new("slow", "Answers after five seconds", schema),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name == "slow" {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        let text = request
            .arguments
            .and_then(|args| args.get("text").and_then(|t| t.as_str()).map(str::to_owned))
            .unwrap_or_default();
        Ok(CallToolResponse::Complete(CallToolResult::success(vec![
            ContentBlock::text(text),
        ])))
    }
}

/// Refuses requests without the expected API key and bearer token, echoing the key it got so
/// tests can check that IGOR does not repeat it.
async fn require_auth(request: Request, next: Next) -> Response {
    // Read the headers before awaiting: the request body is not `Sync`.
    let (key, authorization) = {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        (header("x-api-key"), header("authorization"))
    };
    if key != API_KEY || authorization != format!("Bearer {TOKEN}") {
        return (StatusCode::UNAUTHORIZED, format!("bad key {key}")).into_response();
    }
    next.run(request).await
}

/// Serves `Echo` at `/mcp` on a free loopback port and returns its URL.
async fn serve() -> Result<String, Box<dyn std::error::Error>> {
    let service = StreamableHttpService::new(
        || Ok(Echo),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(require_auth));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/mcp", listener.local_addr()?);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(url)
}

fn config(url: &str) -> HttpServerConfig {
    let mut config = HttpServerConfig::new(url);
    config.headers = vec![("X-Api-Key".to_owned(), API_KEY.to_owned())];
    config.bearer_token = Some(TOKEN.to_owned());
    config.startup_timeout = Duration::from_secs(5);
    config.call_timeout = Duration::from_secs(5);
    config
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_and_calls_tools_with_header_auth() -> TestResult {
    let url = serve().await?;
    let backend = McpBackend::connect_http(ServerId::new("remote")?, &config(&url)).await?;
    assert!(backend.is_connected());
    let names: Vec<String> = backend
        .list_tools()
        .await?
        .into_iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(names, ["remote/echo", "remote/slow"]);
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
    assert!(!backend.is_connected());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_credentials_fail_without_revealing_secrets() -> TestResult {
    let url = serve().await?;
    let mut wrong = config(&url);
    wrong.headers = vec![("X-Api-Key".to_owned(), "key-wrong-http-sentinel".to_owned())];
    let error = McpBackend::connect_http(ServerId::new("remote")?, &wrong)
        .await
        .err()
        .ok_or("connected with the wrong key")?;
    assert!(matches!(error, ConnectError::Initialize { .. }), "{error}");
    let text = format!("{error} {error:?} {wrong:?}");
    for secret in ["key-wrong-http-sentinel", TOKEN] {
        assert!(!text.contains(secret), "secret in {text}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unreachable_server_reports_a_connect_error() -> TestResult {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/mcp", listener.local_addr()?);
    drop(listener);
    let error = McpBackend::connect_http(ServerId::new("remote")?, &config(&url))
        .await
        .err()
        .ok_or("connected to a closed port")?;
    assert!(
        matches!(
            error,
            ConnectError::Initialize { .. } | ConnectError::Timeout { .. }
        ),
        "{error}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn timeouts_and_cancellation_match_stdio() -> TestResult {
    let url = serve().await?;
    let mut short = config(&url);
    short.call_timeout = Duration::from_millis(300);
    let backend = McpBackend::connect_http(ServerId::new("remote")?, &short).await?;
    let timed_out = backend
        .call_tool("slow", Map::new(), CancellationToken::new())
        .await;
    assert!(
        matches!(timed_out, Err(BackendError::OutcomeUnknown(_))),
        "{timed_out:?}"
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    let before = backend.call_tool("echo", Map::new(), cancel).await;
    assert_eq!(before, Err(BackendError::CancelledBeforeSend));
    backend.shutdown().await;
    Ok(())
}
