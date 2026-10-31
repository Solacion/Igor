//! Connects configured servers and builds the tool registry.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::sync::Arc;

use igor_config::{Config, ServerConfig};
use igor_core::registry::ToolRegistry;
use igor_core::session::UnavailableServer;
use igor_core::tools::{ServerId, ToolBackend, ToolDescriptor};
use igor_mcp::{McpBackend, StdioServerConfig};

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
            Err(message) => {
                writeln!(
                    out,
                    "[server {}: unavailable] {}",
                    server.id,
                    sanitize(&message)
                )?;
                unavailable.push(UnavailableServer {
                    id: server.id.clone(),
                    reason: message,
                });
            }
        }
    }
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
    Ok(Connected {
        registry,
        backends,
        unavailable,
    })
}

async fn connect_one(
    server: &ServerConfig,
) -> Result<(Arc<dyn ToolBackend>, Vec<ToolDescriptor>), String> {
    let mut stdio = StdioServerConfig::new(&server.command);
    stdio.args = server.args.iter().map(OsString::from).collect();
    stdio.cwd = Some(server.cwd.clone());
    stdio.env_allowlist = server.env_allowlist.clone();
    stdio.startup_timeout = server.startup_timeout;
    stdio.call_timeout = server.call_timeout;
    for (name, secret) in &server.env {
        let value = secret.resolve().map_err(|e| e.to_string())?;
        stdio
            .env
            .push((name.clone(), OsString::from(value.expose())));
    }
    let backend = McpBackend::connect(server.id.clone(), &stdio)
        .await
        .map_err(|e| e.to_string())?;
    let tools = backend.list_tools().await.map_err(|e| e.to_string())?;
    Ok((Arc::new(backend), tools))
}
