//! Settings: MCP servers and their tool permissions.
//!
//! The configuration file is the source of truth. Every change goes through
//! [`igor_config::ConfigDocument`], so it is validated with the same rules as loading and saved
//! atomically. The desktop keeps one live connection per configured server to report status
//! and list tools for the permission editor. Secret values never cross IPC: server entries
//! carry only the names of the environment variables that hold them.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use igor_config::{ConfigDocument, ConfigError, PermissionClass, ServerConfig, ServerEntry};
use igor_core::tools::{ServerId, ToolBackend, ToolDescriptor};
use igor_mcp::{McpBackend, StdioServerConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Connection state of one server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// No connection attempt yet.
    NotConnected,
    /// Starting the server and listing its tools.
    Starting,
    /// Connected; tools listed.
    Connected,
    /// The last attempt failed.
    Failed {
        /// Why. Diagnostic text without secret values.
        detail: String,
    },
}

/// A tool reported by a server, for the permission editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolView {
    /// Tool name.
    pub name: String,
    /// Server-supplied description. Untrusted: render as text.
    pub description: Option<String>,
    /// The server claims the tool does not modify anything. A hint only.
    pub read_only_hint: Option<bool>,
    /// The server claims the tool may destroy data. A hint only.
    pub destructive_hint: Option<bool>,
}

impl From<&ToolDescriptor> for ToolView {
    fn from(tool: &ToolDescriptor) -> Self {
        Self {
            name: tool.id.name.clone(),
            description: tool.description.clone(),
            read_only_hint: tool.hints.read_only,
            destructive_hint: tool.hints.destructive,
        }
    }
}

/// One configured server as the Settings view shows it.
#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    /// The entry as written in the configuration file.
    pub entry: ServerEntry,
    /// Live connection state.
    pub status: Status,
    /// Tools from the live connection; empty when not connected.
    pub tools: Vec<ToolView>,
    /// Permission classes by tool name, as configured. A tool without one is unclassified.
    pub permissions: BTreeMap<String, String>,
}

/// Everything the Settings view shows.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    /// The configuration file being edited.
    pub config_path: String,
    /// Configured servers, in file order.
    pub servers: Vec<ServerView>,
}

/// A failed settings command. `field` names the offending entry field, such as `command` or
/// `env.TOKEN`, when the problem is in the submitted server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    /// Field of the submitted server entry, when known.
    pub field: Option<String>,
    /// What is wrong. Never contains a secret value.
    pub message: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.field {
            Some(field) => write!(f, "{field}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for CommandError {}

impl CommandError {
    fn message(message: impl Into<String>) -> Self {
        Self {
            field: None,
            message: message.into(),
        }
    }

    /// Maps a configuration error, reducing `servers[i].field` to `field` when `i` is the
    /// submitted entry.
    fn config(error: &ConfigError, index: Option<usize>) -> Self {
        let field = error.field().and_then(|field| {
            let prefix = format!("servers[{}].", index?);
            field.strip_prefix(&prefix).map(str::to_owned)
        });
        let message = match (error.problem(), &field) {
            (Some(problem), Some(_)) => problem.to_string(),
            _ => error.to_string(),
        };
        Self { field, message }
    }
}

struct Connection {
    backend: Option<Arc<SharedBackend>>,
    status: Status,
    tools: Vec<ToolView>,
    descriptors: Vec<ToolDescriptor>,
}

impl Connection {
    fn new(status: Status) -> Self {
        Self {
            backend: None,
            status,
            tools: Vec::new(),
            descriptors: Vec::new(),
        }
    }
}

impl Connection {
    /// The recorded status, or failed when a connected server has since stopped.
    fn current_status(&self) -> Status {
        match (&self.status, &self.backend) {
            (Status::Connected, Some(backend)) if !backend.is_connected() => Status::Failed {
                detail: "the server stopped".to_owned(),
            },
            (status, _) => status.clone(),
        }
    }
}

/// The final owner disconnects a server; an in-flight turn keeps its lease after a Settings edit.
pub struct SharedBackend(pub McpBackend);
impl std::ops::Deref for SharedBackend {
    type Target = McpBackend;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for SharedBackend {
    fn drop(&mut self) {
        self.0.disconnect();
    }
}
#[async_trait::async_trait]
impl ToolBackend for SharedBackend {
    fn server_id(&self) -> &ServerId {
        self.0.server_id()
    }
    fn is_connected(&self) -> bool {
        self.0.is_connected()
    }
    async fn shutdown(&self) {
        self.0.shutdown().await;
    }
    async fn list_tools(&self) -> Result<Vec<ToolDescriptor>, igor_core::tools::BackendError> {
        self.0.list_tools().await
    }
    async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Map<String, serde_json::Value>,
        cancel: igor_core::CancellationToken,
    ) -> Result<igor_core::tools::ToolOutput, igor_core::tools::BackendError> {
        self.0.call_tool(name, arguments, cancel).await
    }
}
pub struct Snapshot {
    pub config: igor_config::Config,
    pub descriptors: Vec<ToolDescriptor>,
    pub backends: HashMap<ServerId, Arc<dyn ToolBackend>>,
    pub servers: Vec<Arc<SharedBackend>>,
    pub revision: u64,
}

/// Settings state: the configuration file and live server connections.
pub struct Settings {
    path: Option<PathBuf>,
    connections: Mutex<BTreeMap<String, Connection>>,
    /// Serialises edits, so two saves cannot interleave read and write.
    edits: Mutex<()>,
}

impl Settings {
    /// Settings for the configuration file at `path`; `None` when no location applies.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            connections: Mutex::new(BTreeMap::new()),
            edits: Mutex::new(()),
        }
    }

    fn document(&self) -> Result<ConfigDocument, CommandError> {
        let path = self.path.as_deref().ok_or_else(|| {
            CommandError::message("no configuration location: set IGOR_CONFIG or HOME")
        })?;
        ConfigDocument::open(path).map_err(|error| CommandError::config(&error, None))
    }

    pub async fn snapshot(&self) -> Result<Snapshot, CommandError> {
        let _edit = self.edits.lock().await;
        let doc = self.document()?;
        let config = doc
            .validate()
            .map_err(|_| CommandError::message("Configuration could not be loaded"))?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        std::fs::read(doc.path())
            .unwrap_or_default()
            .hash(&mut hash);
        let connections = self.connections.lock().await;
        let mut descriptors = Vec::new();
        let mut backends = HashMap::new();
        let mut servers = Vec::new();
        for server in config.servers() {
            if let Some(connection) = connections.get(server.id.as_str())
                && let Some(backend) = &connection.backend
            {
                (Arc::as_ptr(backend) as usize).hash(&mut hash);
                backend.is_connected().hash(&mut hash);
                if backend.is_connected() {
                    descriptors.extend(connection.descriptors.clone());
                    backends.insert(server.id.clone(), backend.clone() as Arc<dyn ToolBackend>);
                    servers.push(backend.clone());
                }
            }
        }
        Ok(Snapshot {
            config,
            descriptors,
            backends,
            servers,
            revision: hash.finish(),
        })
    }

    /// The current settings.
    pub async fn view(&self) -> Result<SettingsView, CommandError> {
        let doc = self.document()?;
        let entries = doc
            .servers()
            .map_err(|error| CommandError::config(&error, None))?;
        let connections = self.connections.lock().await;
        let mut servers = Vec::with_capacity(entries.len());
        for entry in entries {
            let permissions = doc
                .permissions(&entry.id)
                .map_err(|error| CommandError::config(&error, None))?;
            let (status, tools) = connections.get(&entry.id).map_or_else(
                || (Status::NotConnected, Vec::new()),
                |c| (c.current_status(), c.tools.clone()),
            );
            servers.push(ServerView {
                entry,
                status,
                tools,
                permissions,
            });
        }
        Ok(SettingsView {
            config_path: doc.path().display().to_string(),
            servers,
        })
    }

    /// Validates and saves `entry`, adding it or replacing the server `replacing`, then
    /// (re)connects it.
    pub async fn save_server(
        &self,
        replacing: Option<&str>,
        entry: ServerEntry,
    ) -> Result<SettingsView, CommandError> {
        let config = {
            let _edit = self.edits.lock().await;
            let mut doc = self.document()?;
            let index = position(&doc, replacing)?;
            doc.put_server(replacing, &entry)
                .map_err(|problem| CommandError::message(problem.to_string()))?;
            doc.save()
                .map_err(|error| CommandError::config(&error, Some(index)))?
        };
        if let Some(old) = replacing
            && old != entry.id
        {
            self.connections.lock().await.remove(old);
        }
        if let Some(server) = config.servers().iter().find(|s| s.id.as_str() == entry.id) {
            self.connect(server).await;
        }
        self.view().await
    }

    /// Removes the server `id` and its permissions, and disconnects it.
    pub async fn remove_server(&self, id: &str) -> Result<SettingsView, CommandError> {
        {
            let _edit = self.edits.lock().await;
            let mut doc = self.document()?;
            if !doc.remove_server(id) {
                return Err(CommandError::message(format!("no server {id:?}")));
            }
            doc.save()
                .map_err(|error| CommandError::config(&error, None))?;
        }
        self.connections.lock().await.remove(id);
        self.view().await
    }

    /// Sets or clears the class of one tool.
    pub async fn set_permission(
        &self,
        server: &str,
        tool: &str,
        class: Option<PermissionClass>,
    ) -> Result<SettingsView, CommandError> {
        {
            let _edit = self.edits.lock().await;
            let mut doc = self.document()?;
            doc.set_permission(server, tool, class);
            doc.save()
                .map_err(|error| CommandError::config(&error, None))?;
        }
        self.view().await
    }

    /// Starts `entry` without saving it, lists its tools and stops it.
    pub async fn test_server(
        &self,
        replacing: Option<&str>,
        entry: ServerEntry,
    ) -> Result<Vec<ToolView>, CommandError> {
        let mut doc = self.document()?;
        let index = position(&doc, replacing)?;
        doc.put_server(replacing, &entry)
            .map_err(|problem| CommandError::message(problem.to_string()))?;
        let config = doc
            .validate()
            .map_err(|error| CommandError::config(&error, Some(index)))?;
        let server = config
            .servers()
            .get(index)
            .ok_or_else(|| CommandError::message("the server was not added"))?;
        let (backend, tools) = start(server).await.map_err(CommandError::message)?;
        backend.disconnect();
        Ok(tools.iter().map(ToolView::from).collect())
    }

    /// Reconnects the server `id`.
    pub async fn reconnect(&self, id: &str) -> Result<SettingsView, CommandError> {
        let config = self
            .document()?
            .validate()
            .map_err(|error| CommandError::config(&error, None))?;
        let server = config
            .servers()
            .iter()
            .find(|s| s.id.as_str() == id)
            .ok_or_else(|| CommandError::message(format!("no server {id:?}")))?;
        self.connect(server).await;
        self.view().await
    }

    /// Connects every configured server. Problems are recorded as statuses.
    pub async fn connect_all(&self) {
        let Ok(doc) = self.document() else {
            return;
        };
        let Ok(config) = doc.validate() else {
            return;
        };
        for server in config.servers() {
            self.connect(server).await;
        }
    }

    async fn connect(&self, server: &ServerConfig) {
        let id = server.id.as_str().to_owned();
        self.connections
            .lock()
            .await
            .insert(id.clone(), Connection::new(Status::Starting));
        let connection = match start(server).await {
            Ok((backend, tools)) => Connection {
                backend: Some(Arc::new(SharedBackend(backend))),
                status: Status::Connected,
                tools: tools.iter().map(ToolView::from).collect(),
                descriptors: tools,
            },
            Err(detail) => Connection::new(Status::Failed { detail }),
        };
        let mut connections = self.connections.lock().await;
        // A server removed or renamed while starting is not re-added.
        if let Some(slot) = connections.get_mut(&id) {
            *slot = connection;
        }
    }
}

/// Index the submitted entry will have in the document's server list.
fn position(doc: &ConfigDocument, replacing: Option<&str>) -> Result<usize, CommandError> {
    let servers = doc
        .servers()
        .map_err(|error| CommandError::config(&error, None))?;
    Ok(match replacing {
        Some(id) => servers
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| CommandError::message(format!("no server {id:?}")))?,
        None => servers.len(),
    })
}

/// Starts a server and lists its tools.
async fn start(server: &ServerConfig) -> Result<(McpBackend, Vec<ToolDescriptor>), String> {
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
    let id: ServerId = server.id.clone();
    let backend = McpBackend::connect(id, &stdio)
        .await
        .map_err(|e| e.to_string())?;
    let tools = backend.list_tools().await.map_err(|e| e.to_string())?;
    Ok((backend, tools))
}

/// Request to save a server.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveServerRequest {
    /// Id of the server being edited; `None` adds a new server.
    pub replacing: Option<String>,
    /// The entry to save.
    pub entry: ServerEntry,
}

/// Request naming one server.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerRequest {
    /// Server id.
    pub id: String,
}

/// Request to set or clear one tool's class.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    /// Server id.
    pub server: String,
    /// Tool name.
    pub tool: String,
    /// New class; `None` leaves the tool unclassified.
    pub class: Option<PermissionClass>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    /// Builds the fixture MCP server from the core workspace and returns its path.
    fn fixture_server() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let root = repo_root();
        let status = Command::new(env!("CARGO"))
            .args(["build", "--locked", "-q", "-p", "igor-fixture-server"])
            .current_dir(&root)
            .status()?;
        if !status.success() {
            return Err("cannot build igor-fixture-server".into());
        }
        Ok(root.join("target/debug/igor-fixture-server"))
    }

    fn temp_config(name: &str) -> std::io::Result<PathBuf> {
        let dir = std::env::temp_dir()
            .join(format!("igor-desktop-settings-{}", std::process::id()))
            .join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join("config.toml"))
    }

    fn sales_entry(command: &Path) -> ServerEntry {
        let fixture = repo_root().join("crates/igor-fixture-server/fixtures/sales.json");
        ServerEntry {
            id: "pos".to_owned(),
            command: command.display().to_string(),
            args: vec!["--fixture".to_owned(), fixture.display().to_string()],
            ..ServerEntry::default()
        }
    }

    #[tokio::test]
    async fn add_test_save_permission_and_remove_round_trip() -> TestResult {
        let server = fixture_server()?;
        let path = temp_config("round-trip")?;
        let settings = Settings::new(Some(path.clone()));
        assert!(settings.view().await?.servers.is_empty());

        let tools = settings.test_server(None, sales_entry(&server)).await?;
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "get_stock_levels",
                "get_sales_history",
                "create_draft_order"
            ]
        );
        assert!(!path.exists(), "testing must not save");

        let view = settings.save_server(None, sales_entry(&server)).await?;
        let pos = &view.servers[0];
        assert_eq!(pos.status, Status::Connected);
        assert_eq!(pos.tools.len(), 3);
        assert!(pos.permissions.is_empty(), "new tools start unclassified");

        let view = settings
            .set_permission("pos", "create_draft_order", Some(PermissionClass::Write))
            .await?;
        assert_eq!(view.servers[0].permissions["create_draft_order"], "write");
        let config = igor_config::Config::load(&path)?;
        assert_eq!(config.servers()[0].id.as_str(), "pos");

        let connected_revision = settings.snapshot().await?.revision;
        if let Some(backend) = settings
            .connections
            .lock()
            .await
            .get("pos")
            .and_then(|c| c.backend.as_ref())
        {
            backend.disconnect();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let view = settings.view().await?;
        assert_ne!(settings.snapshot().await?.revision, connected_revision);
        assert!(settings.snapshot().await?.descriptors.is_empty());
        assert_eq!(
            view.servers[0].status,
            Status::Failed {
                detail: "the server stopped".to_owned()
            }
        );

        let view = settings.remove_server("pos").await?;
        assert!(view.servers.is_empty());
        assert!(!std::fs::read_to_string(&path)?.contains("create_draft_order"));
        Ok(())
    }

    #[tokio::test]
    async fn invalid_entry_reports_its_field_and_is_not_saved() -> TestResult {
        let path = temp_config("invalid")?;
        let settings = Settings::new(Some(path.clone()));
        let entry = ServerEntry {
            id: "Bad Id".to_owned(),
            command: "./x".to_owned(),
            ..ServerEntry::default()
        };
        let error = settings
            .save_server(None, entry)
            .await
            .err()
            .ok_or("saved an invalid entry")?;
        assert_eq!(error.field.as_deref(), Some("id"));
        assert!(!path.exists());

        let entry = ServerEntry {
            id: "pos".to_owned(),
            command: "./x".to_owned(),
            env: BTreeMap::from([("TOKEN".to_owned(), "lower case".to_owned())]),
            ..ServerEntry::default()
        };
        let error = settings
            .test_server(None, entry)
            .await
            .err()
            .ok_or("accepted a bad reference")?;
        assert_eq!(error.field.as_deref(), Some("env.TOKEN"));
        assert!(!error.message.contains("lower case"));
        Ok(())
    }

    #[tokio::test]
    async fn failed_start_is_a_status_not_an_error() -> TestResult {
        let path = temp_config("failed")?;
        let settings = Settings::new(Some(path));
        let entry = ServerEntry {
            id: "missing".to_owned(),
            command: "/nonexistent/igor-server".to_owned(),
            ..ServerEntry::default()
        };
        let view = settings.save_server(None, entry).await?;
        assert!(matches!(view.servers[0].status, Status::Failed { .. }));
        Ok(())
    }
}
