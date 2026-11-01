//! OAuth sign-in and connection against an in-process authorization server and MCP server.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use igor_core::tools::{ServerId, ToolBackend};
use igor_mcp::{ConnectError, HttpServerConfig, McpBackend, SignInError, TokenStore, sign_in};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const ACCESS_TOKEN: &str = "at-oauth-sentinel-41f0";
const CODE: &str = "code-oauth-sentinel-77aa";

#[derive(Default)]
struct MemoryTokens(Mutex<Option<String>>);

impl TokenStore for MemoryTokens {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.clone())
    }
    fn save(&self, value: &str) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = Some(value.to_owned());
        Ok(())
    }
    fn clear(&self) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = None;
        Ok(())
    }
}

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
        Ok(ListToolsResult::with_all_items(vec![Tool::new(
            "whoami",
            "Says who is signed in",
            schema,
        )]))
    }

    async fn call_tool(
        &self,
        _request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(CallToolResponse::Complete(CallToolResult::success(vec![
            ContentBlock::text("operator"),
        ])))
    }
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

fn query(request: &Request, name: &str) -> Option<String> {
    url::form_urlencoded::parse(request.uri().query()?.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// Serves the authorization server and the MCP endpoint on one loopback port; returns the MCP
/// URL.
async fn serve() -> Result<String, Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let resource = {
        let base = base.clone();
        move || {
            let base = base.clone();
            async move {
                json_response(
                    StatusCode::OK,
                    json!({ "resource": format!("{base}/mcp"), "authorization_servers": [base] }),
                )
            }
        }
    };
    let metadata = {
        let base = base.clone();
        move || {
            let base = base.clone();
            async move {
                json_response(
                    StatusCode::OK,
                    json!({
                        "issuer": base,
                        "authorization_endpoint": format!("{base}/authorize"),
                        "token_endpoint": format!("{base}/token"),
                        "registration_endpoint": format!("{base}/register"),
                        "response_types_supported": ["code"],
                        "grant_types_supported": ["authorization_code", "refresh_token"],
                        "code_challenge_methods_supported": ["S256"],
                        "token_endpoint_auth_methods_supported": ["none"],
                    }),
                )
            }
        }
    };
    let register = |request: Request| async move {
        let body = to_bytes(request.into_body(), 64 * 1024)
            .await
            .unwrap_or_default();
        let asked: Value = serde_json::from_slice(&body).unwrap_or_default();
        json_response(
            StatusCode::CREATED,
            json!({
                "client_id": "igor-test-client",
                "redirect_uris": asked.get("redirect_uris").cloned().unwrap_or_default(),
                "token_endpoint_auth_method": "none",
            }),
        )
    };
    let authorize = |request: Request| async move {
        let (Some(redirect), Some(state)) =
            (query(&request, "redirect_uri"), query(&request, "state"))
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if query(&request, "code_challenge_method").as_deref() != Some("S256") {
            return StatusCode::BAD_REQUEST.into_response();
        }
        Response::builder()
            .status(StatusCode::FOUND)
            .header(
                header::LOCATION,
                format!("{redirect}?code={CODE}&state={state}"),
            )
            .body(Body::empty())
            .unwrap_or_default()
    };
    let token = |request: Request| async move {
        let body = to_bytes(request.into_body(), 64 * 1024)
            .await
            .unwrap_or_default();
        let form: Vec<(String, String)> = url::form_urlencoded::parse(&body)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let field = |name: &str| {
            form.iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        if field("code") != Some(CODE) || field("code_verifier").is_none_or(str::is_empty) {
            return json_response(StatusCode::BAD_REQUEST, json!({ "error": "invalid_grant" }));
        }
        json_response(
            StatusCode::OK,
            json!({
                "access_token": ACCESS_TOKEN,
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": "rt-oauth-sentinel",
            }),
        )
    };
    let challenge =
        format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\"");
    let require_token = move |request: Request, next: Next| {
        let challenge = challenge.clone();
        async move {
            let authorized = request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                == Some(format!("Bearer {ACCESS_TOKEN}").as_str());
            if !authorized {
                return (
                    StatusCode::UNAUTHORIZED,
                    [(header::WWW_AUTHENTICATE, challenge)],
                )
                    .into_response();
            }
            next.run(request).await
        }
    };
    let mcp = StreamableHttpService::new(
        || Ok(Echo),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(resource.clone()),
        )
        .route("/.well-known/oauth-protected-resource/mcp", get(resource))
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/register", post(register))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .nest_service(
            "/mcp",
            Router::new()
                .fallback_service(mcp)
                .layer(middleware::from_fn(require_token)),
        );
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(format!("{base}/mcp"))
}

fn config(url: &str, store: &Arc<MemoryTokens>) -> HttpServerConfig {
    let mut config = HttpServerConfig::new(url);
    config.oauth = Some(Arc::clone(store) as Arc<dyn TokenStore>);
    config.startup_timeout = Duration::from_secs(10);
    config
}

/// Plays the browser: follows the authorization page's redirect back to IGOR's loopback address.
fn browser(page: &str) -> Result<(), String> {
    let page = page.to_owned();
    tokio::spawn(async move {
        let _ = reqwest::get(page).await;
    });
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn signs_in_through_the_browser_then_connects_with_the_stored_token() -> TestResult {
    let url = serve().await?;
    let store = Arc::new(MemoryTokens::default());
    let id = ServerId::new("remote")?;

    let before = McpBackend::connect_http(id.clone(), &config(&url, &store)).await;
    assert!(
        matches!(before, Err(ConnectError::SignInNeeded { .. })),
        "{before:?}"
    );

    sign_in(
        &url,
        Arc::clone(&store) as Arc<dyn TokenStore>,
        browser,
        Duration::from_secs(10),
    )
    .await?;
    let saved = store.load()?.ok_or("nothing saved")?;
    assert!(saved.contains("igor-test-client"));

    let config = config(&url, &store);
    assert!(!format!("{config:?}").contains(ACCESS_TOKEN));
    let backend = McpBackend::connect_http(id, &config).await?;
    let names: Vec<String> = backend
        .list_tools()
        .await?
        .into_iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(names, ["remote/whoami"]);
    backend.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_or_abandoned_sign_in_saves_nothing() -> TestResult {
    let url = serve().await?;
    let store = Arc::new(MemoryTokens::default());
    let declined = sign_in(
        &url,
        Arc::clone(&store) as Arc<dyn TokenStore>,
        |page| {
            // The operator says no: the authorization server sends an error back.
            let redirect = url::Url::parse(page)
                .ok()
                .and_then(|page| {
                    page.query_pairs()
                        .find(|(key, _)| key == "redirect_uri")
                        .map(|(_, value)| value.into_owned())
                })
                .ok_or("no redirect")?;
            browser(&format!("{redirect}?error=access_denied"))
        },
        Duration::from_secs(10),
    )
    .await;
    assert!(
        matches!(declined, Err(SignInError::Declined(_))),
        "{declined:?}"
    );
    let abandoned = sign_in(
        &url,
        Arc::clone(&store) as Arc<dyn TokenStore>,
        |_| Ok(()),
        Duration::from_millis(300),
    )
    .await;
    assert!(
        matches!(abandoned, Err(SignInError::TimedOut(_))),
        "{abandoned:?}"
    );
    assert!(
        store
            .load()?
            .is_none_or(|saved| !saved.contains(ACCESS_TOKEN))
    );
    Ok(())
}
