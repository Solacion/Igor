//! Connects configured servers and builds the tool registry.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::sync::Arc;

use igor_config::{
    Config, HttpServer, KeyStore, Secret, SecretError, ServerConfig, ServerTransport,
    SystemKeychain,
};
use igor_core::registry::ToolRegistry;
use igor_core::session::UnavailableServer;
use igor_core::tools::{ServerId, ToolBackend, ToolDescriptor};
use igor_mcp::{HttpServerConfig, McpBackend, StdioServerConfig, TokenStore};

use crate::render::sanitize;

/// Connected servers and the registry built from their tools.
pub struct Connected {
    /// Tools of every connected server.
    pub registry: ToolRegistry,
    /// Backends by server.
    pub backends: HashMap<ServerId, Arc<dyn ToolBackend>>,
    /// Configured servers that could not be connected, with the reason.
    pub unavailable: Vec<UnavailableServer>,
}

/// Connects every configured server, reporting each result on `out`. A server that fails to
/// start or list its tools is reported and left out; the session continues without it.
pub async fn connect(config: &Config, out: &mut impl Write) -> io::Result<Connected> {
    let mut backends: HashMap<ServerId, Arc<dyn ToolBackend>> = HashMap::new();
    let mut descriptors: Vec<ToolDescriptor> = Vec::new();
    let mut unavailable = Vec::new();
    for server in config.servers() {
        match connect_one(server).await {
            Ok((backend, tools)) => {
                writeln!(
                    out,
                    "[server {}: connected, {} tools]",
                    server.id,
                    tools.len()
                )?;
                descriptors.extend(tools);
                backends.insert(server.id.clone(), backend);
            }
            Err(Failed { message, stderr }) => {
                writeln!(
                    out,
                    "[server {}: unavailable] {}",
                    server.id,
                    sanitize(&message)
                )?;
                // Server output, shown to the operator only: the model gets just the reason.
                if !stderr.is_empty() {
                    writeln!(out, "  last server output:")?;
                }
                for line in &stderr {
                    writeln!(out, "  | {}", sanitize(line))?;
                }
                unavailable.push(UnavailableServer {
                    id: server.id.clone(),
                    reason: message,
                });
            }
        }
    }
    let registry = build_registry(config, descriptors, out)?;
    Ok(Connected {
        registry,
        backends,
        unavailable,
    })
}

/// Builds the registry from `descriptors` and the configured permissions, reporting excluded,
/// hidden and approval-gated tools on `out`.
pub fn build_registry(
    config: &Config,
    descriptors: Vec<ToolDescriptor>,
    out: &mut impl Write,
) -> io::Result<ToolRegistry> {
    let classes = descriptors
        .iter()
        .map(|d| (d.id.clone(), config.permissions().classify(&d.id)))
        .collect();
    let registry = ToolRegistry::build(descriptors, &classes);
    for excluded in registry.excluded() {
        writeln!(
            out,
            "[tool {} excluded] {}",
            sanitize(&excluded.id.to_string()),
            excluded.reason
        )?;
    }
    for tool in registry.tools() {
        if !tool.classification().is_offered() {
            writeln!(
                out,
                "[tool {} hidden: {:?}]",
                sanitize(&tool.id().to_string()),
                tool.classification()
            )?;
        } else if tool.tightened_by_hints() {
            writeln!(
                out,
                "[tool {} needs approval: the server reports it may modify data]",
                sanitize(&tool.id().to_string())
            )?;
        }
    }
    Ok(registry)
}

/// Lists every connected server's tools again and builds a new registry. `stale` names the
/// servers that reported a change; each is reported on `out`. A server whose listing fails is
/// reported and its tools are left out.
pub async fn refresh(
    config: &Config,
    backends: &HashMap<ServerId, Arc<dyn ToolBackend>>,
    stale: &[ServerId],
    out: &mut impl Write,
) -> io::Result<ToolRegistry> {
    let mut servers: Vec<_> = backends.iter().collect();
    servers.sort_by(|a, b| a.0.cmp(b.0));
    let mut descriptors = Vec::new();
    for (id, backend) in servers {
        match backend.list_tools().await {
            Ok(tools) => {
                if stale.contains(id) {
                    writeln!(out, "[server {id}: tools refreshed, {} tools]", tools.len())?;
                }
                descriptors.extend(tools);
            }
            Err(error) => {
                writeln!(
                    out,
                    "[server {id}: cannot list tools] {}",
                    sanitize(&error.to_string())
                )?;
            }
        }
    }
    build_registry(config, descriptors, out)
}

/// Why a server could not be used, with its last stderr lines when it started.
struct Failed {
    message: String,
    stderr: Vec<String>,
}

impl From<String> for Failed {
    fn from(message: String) -> Self {
        Self {
            message,
            stderr: Vec::new(),
        }
    }
}

async fn connect_one(
    server: &ServerConfig,
) -> Result<(Arc<dyn ToolBackend>, Vec<ToolDescriptor>), Failed> {
    let backend = match &server.transport {
        ServerTransport::Stdio(local) => {
            let mut stdio = StdioServerConfig::new(&local.command);
            stdio.args = local.args.iter().map(OsString::from).collect();
            stdio.cwd = Some(local.cwd.clone());
            stdio.env_allowlist = local.env_allowlist.clone();
            stdio.startup_timeout = server.startup_timeout;
            stdio.call_timeout = server.call_timeout;
            for (name, secret) in &local.env {
                let value = secret.resolve().map_err(|e| Failed::from(e.to_string()))?;
                stdio
                    .env
                    .push((name.clone(), OsString::from(value.expose())));
            }
            McpBackend::connect(server.id.clone(), &stdio).await
        }
        ServerTransport::Http(remote) => {
            let http = http_config(server, remote).map_err(|e| Failed::from(e.to_string()))?;
            McpBackend::connect_http(server.id.clone(), &http).await
        }
    }
    .map_err(|e| Failed {
        message: e.to_string(),
        stderr: e.stderr().to_vec(),
    })?;
    let tools = match backend.list_tools().await {
        Ok(tools) => tools,
        Err(e) => {
            return Err(Failed {
                message: e.to_string(),
                stderr: backend.recent_stderr(),
            });
        }
    };
    Ok((Arc::new(backend), tools))
}

/// The connection settings for a remote server, with its header secrets read now.
fn http_config(
    server: &ServerConfig,
    remote: &HttpServer,
) -> Result<HttpServerConfig, SecretError> {
    let mut http = HttpServerConfig::new(remote.url.as_str());
    http.startup_timeout = server.startup_timeout;
    http.call_timeout = server.call_timeout;
    for (name, secret) in &remote.headers {
        http.headers
            .push((name.clone(), secret.resolve()?.expose().to_owned()));
    }
    if let Some(token) = &remote.bearer_token {
        http.bearer_token = Some(token.resolve()?.expose().to_owned());
    }
    if remote.oauth {
        http.oauth = Some(Arc::new(KeychainTokens(HttpServer::oauth_item(&server.id))));
    }
    Ok(http)
}

/// A server's OAuth tokens, kept in a login Keychain item.
pub(crate) struct KeychainTokens(pub(crate) String);

impl TokenStore for KeychainTokens {
    fn load(&self) -> Result<Option<String>, String> {
        SystemKeychain
            .get(&self.0)
            .map(|found| found.map(|secret| secret.expose().to_owned()))
            .map_err(|error| error.to_string())
    }

    fn save(&self, value: &str) -> Result<(), String> {
        SystemKeychain
            .set(&self.0, &Secret::new(value.to_owned()))
            .map_err(|error| error.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        SystemKeychain
            .delete(&self.0)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}
