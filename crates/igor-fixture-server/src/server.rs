//! The MCP server handler that answers from a [`Fixture`].

use std::borrow::Cow;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};

use crate::call_log::CallLog;
use crate::fixture::{Fixture, MatchKind, unrecorded_message};

/// Newest MCP protocol revision the fixture server negotiates. It is the newest revision that
/// still has an `initialize` handshake.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::LATEST_WITH_INITIALIZE;

/// Serves one fixture set over MCP.
#[derive(Debug)]
pub struct FixtureServer {
    fixture: Fixture,
    call_log: Option<CallLog>,
}

impl FixtureServer {
    /// Creates a server for `fixture`, optionally logging every call to `call_log`.
    pub fn new(fixture: Fixture, call_log: Option<CallLog>) -> Self {
        Self { fixture, call_log }
    }
}

impl ServerHandler for FixtureServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(PROTOCOL_VERSION)
            .with_server_info(Implementation::new(
                self.fixture.server_name.clone(),
                self.fixture.server_version.clone(),
            ))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::known_up_to(&PROTOCOL_VERSION))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.fixture.tools.clone()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let lookup = self
            .fixture
            .lookup(&request.name, request.arguments.as_ref());
        if let Some(log) = &self.call_log {
            log.record(&request.name, &lookup.canonical_arguments, lookup.kind)
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        }
        match (lookup.kind, lookup.recorded) {
            (MatchKind::UnknownTool, _) => Err(ErrorData::invalid_params(
                format!("unknown tool: {}", request.name),
                None,
            )),
            (_, Some(recorded)) => {
                if let Some(delay) = recorded.delay {
                    // Behave like a live server: stop waiting once the client cancels the request.
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = context.ct.cancelled() => {
                            return Err(ErrorData::internal_error("request cancelled", None));
                        }
                    }
                }
                Ok(recorded.result.clone().into())
            }
            (_, None) => Ok(
                CallToolResult::error(vec![ContentBlock::text(unrecorded_message(
                    &request.name,
                    &lookup.canonical_arguments,
                ))])
                .into(),
            ),
        }
    }
}
