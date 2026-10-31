//! Desktop composition only; permissions, orchestration and approval stay in igor-core.

use std::sync::Arc;
use std::time::Duration;

use igor_config::{ProviderConfig, ProviderKind, Secret};
use igor_core::CancellationToken;
use igor_core::ids::SequentialIds;
use igor_core::limits::ResultSizeLimit;
use igor_core::provider::Provider;
use igor_core::registry::ToolRegistry;
use igor_core::session::{SessionConfig, SessionParts};
use igor_provider_anthropic::{AnthropicProvider, AnthropicSettings};
use igor_provider_openai::{OpenAiCompatibleProvider, OpenAiSettings};

use crate::sessions::{BridgeError, ServerStatus, StartRequest};

pub struct Prepared {
    pub parts: SessionParts,
    pub config: SessionConfig,
    pub servers: Vec<Arc<crate::settings::SharedBackend>>,
    pub statuses: Vec<ServerStatus>,
    pub provider_id: String,
    pub model: String,
    pub secrets: Vec<Secret>,
    pub revision: u64,
}

pub async fn prepare(
    request: StartRequest,
    cancel: &CancellationToken,
    settings: &crate::settings::Settings,
) -> Result<Prepared, BridgeError> {
    if cancel.is_cancelled() {
        return Err(BridgeError::Cancelled);
    }
    let snapshot = settings
        .snapshot()
        .await
        .map_err(|_| BridgeError::Configuration)?;
    let config = snapshot.config;
    let configured = config
        .provider(&request.provider_id)
        .ok_or(BridgeError::Provider)?;
    let mut secrets = Vec::new();
    let provider = provider(configured, &mut secrets)?;
    for server in config.servers() {
        for reference in server.env.values() {
            secrets.push(
                reference
                    .resolve()
                    .map_err(|_| BridgeError::Configuration)?,
            );
        }
    }
    let descriptors = snapshot.descriptors;
    let backends = snapshot.backends;
    let servers = snapshot.servers;
    let statuses = config
        .servers()
        .iter()
        .map(|server| ServerStatus {
            server_id: server.id.to_string(),
            connected: backends.contains_key(&server.id),
            tool_count: descriptors
                .iter()
                .filter(|tool| tool.id.server == server.id)
                .count(),
        })
        .collect();
    let classes = descriptors
        .iter()
        .map(|tool| (tool.id.clone(), config.permissions().classify(&tool.id)))
        .collect();
    let limits = config.limits();
    let (events, _unused) = tokio::sync::mpsc::unbounded_channel();
    Ok(Prepared {
        parts: SessionParts {
            provider,
            registry: ToolRegistry::build(descriptors, &classes),
            backends,
            ids: Arc::new(SequentialIds::new("call")),
            events: Arc::new(events),
        },
        config: SessionConfig {
            max_model_requests: limits.max_iterations,
            tool_timeout: limits.tool_call_timeout,
            model_timeout: Duration::from_secs(120),
            result_limit: ResultSizeLimit::new(limits.max_result_bytes)
                .map_err(|_| BridgeError::Configuration)?,
            ..SessionConfig::default()
        },
        provider_id: configured.id.clone(),
        model: configured.model.clone(),
        servers,
        statuses,
        secrets,
        revision: snapshot.revision,
    })
}

fn provider(
    config: &ProviderConfig,
    secrets: &mut Vec<Secret>,
) -> Result<Arc<dyn Provider>, BridgeError> {
    let key = config
        .api_key
        .as_ref()
        .map(|reference| reference.resolve())
        .transpose()
        .map_err(|_| BridgeError::Provider)?;
    if let Some(key) = &key {
        secrets.push(Secret::new(key.expose()));
    }
    match config.kind {
        ProviderKind::Anthropic => {
            AnthropicSettings::from_config(config, key.ok_or(BridgeError::Provider)?)
                .and_then(AnthropicProvider::new)
                .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
                .map_err(|_| BridgeError::Provider)
        }
        ProviderKind::OpenAiCompatible => OpenAiSettings::from_config(config, key)
            .and_then(OpenAiCompatibleProvider::new)
            .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
            .map_err(|_| BridgeError::Provider),
    }
}
