//! Editing the configuration file: model providers, MCP servers and tool permissions.
//!
//! A [`ConfigDocument`] keeps the file's formatting and comments, changes only the entries it is
//! asked to change, and validates the whole result with the same rules as [`Config::load`] before
//! saving. Saving replaces the file atomically, so a failed save leaves the old file intact.
//! Secrets are written only as references: [`ServerEntry::env`] and
//! [`ProviderEntry::api_key_env`] hold variable names, never values.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::{Config, ConfigError, Origin, Problem, ProviderKind, absolute_dir, validate};

/// A model provider entry as written in the configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEntry {
    /// Operator-chosen provider id.
    pub id: String,
    /// API flavour.
    pub kind: ProviderKind,
    /// Model name sent to the provider.
    pub model: String,
    /// API base URL. `None` means the kind's default.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Name of the environment variable that holds the API key. Required for
    /// [`ProviderKind::Anthropic`].
    #[serde(default)]
    pub api_key_env: Option<String>,
}

/// An MCP server entry as written in the configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerEntry {
    /// Host-assigned server id.
    pub id: String,
    /// Program to run, as written: relative paths resolve against the configuration directory.
    pub command: String,
    /// Program arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory, as written. `None` means the configuration directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Names of IGOR's environment variables the server may inherit.
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    /// Variables set in the server, each mapped to the name of the IGOR environment variable
    /// that holds the value.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Bound on `initialize` plus `tools/list`, in milliseconds. `None` means the default.
    #[serde(default)]
    pub startup_timeout_ms: Option<u64>,
    /// Bound on each tool call, in milliseconds. `None` means the configured default.
    #[serde(default)]
    pub call_timeout_ms: Option<u64>,
}

/// A tool permission class as written in the configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionClass {
    /// The model may call the tool without approval.
    Read,
    /// Each call needs operator approval.
    Write,
    /// Hidden from the model and refused.
    Denied,
}

impl PermissionClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Denied => "denied",
        }
    }
}

/// The entries as read back from the file. Secret references and provider kinds are read
/// leniently so a listing never fails on them; validation reports them.
#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    providers: Vec<ListedProvider>,
    #[serde(default)]
    servers: Vec<ListedServer>,
    #[serde(default)]
    permissions: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct ListedProvider {
    #[serde(default)]
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    model: String,
    base_url: Option<String>,
    api_key: Option<toml::Value>,
}

#[derive(Deserialize)]
struct ListedServer {
    #[serde(default)]
    id: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env_allowlist: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, toml::Value>,
    startup_timeout_ms: Option<u64>,
    call_timeout_ms: Option<u64>,
}

/// An editable configuration file.
#[derive(Debug, Clone)]
pub struct ConfigDocument {
    path: PathBuf,
    doc: DocumentMut,
}

impl ConfigDocument {
    /// Reads the file at `path`. A missing file is an empty configuration, created on
    /// [`ConfigDocument::save`].
    pub fn open(path: &Path) -> Result<Self, ConfigError> {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let doc = source.parse::<DocumentMut>().map_err(|err| {
            let (line, column) = err
                .span()
                .map_or((1, 1), |span| line_column(&source, span.start));
            ConfigError::Parse {
                origin: Origin::File(path.to_path_buf()),
                line,
                column,
                message: err.message().to_owned(),
            }
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    /// The file this document is saved to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The configured servers, in file order. Secret references that are not
    /// `{ env = "NAME" }` are left out; [`ConfigDocument::validate`] reports them.
    pub fn servers(&self) -> Result<Vec<ServerEntry>, ConfigError> {
        Ok(self
            .listing()?
            .servers
            .into_iter()
            .map(|server| ServerEntry {
                id: server.id,
                command: server.command,
                args: server.args,
                cwd: server.cwd,
                env_allowlist: server.env_allowlist,
                env: server
                    .env
                    .into_iter()
                    .filter_map(|(name, value)| {
                        let var = value.get("env")?.as_str()?.to_owned();
                        Some((name, var))
                    })
                    .collect(),
                startup_timeout_ms: server.startup_timeout_ms,
                call_timeout_ms: server.call_timeout_ms,
            })
            .collect())
    }

    /// The permission classes written for `server`'s tools, as written.
    pub fn permissions(&self, server: &str) -> Result<BTreeMap<String, String>, ConfigError> {
        Ok(self
            .listing()?
            .permissions
            .remove(server)
            .unwrap_or_default())
    }

    /// The configured providers, in file order. Providers of an unknown kind, and API keys that
    /// are not `{ env = "NAME" }`, are left out; [`ConfigDocument::validate`] reports them.
    pub fn providers(&self) -> Result<Vec<ProviderEntry>, ConfigError> {
        Ok(self
            .listing()?
            .providers
            .into_iter()
            .filter_map(|provider| {
                let kind = match provider.kind.as_str() {
                    "anthropic" => ProviderKind::Anthropic,
                    "openai_compatible" => ProviderKind::OpenAiCompatible,
                    _ => return None,
                };
                Some(ProviderEntry {
                    id: provider.id,
                    kind,
                    model: provider.model,
                    base_url: provider.base_url,
                    api_key_env: provider
                        .api_key
                        .and_then(|key| Some(key.get("env")?.as_str()?.to_owned())),
                })
            })
            .collect())
    }

    fn listing(&self) -> Result<Listing, ConfigError> {
        let source = self.doc.to_string();
        toml::from_str(&source).map_err(|err| {
            let (line, column) = err
                .span()
                .map_or((1, 1), |span| line_column(&source, span.start));
            ConfigError::Parse {
                origin: Origin::File(self.path.clone()),
                line,
                column,
                message: validate::hide_quoted_values(err.message()),
            }
        })
    }

    /// Adds `entry`, or replaces the provider with id `replacing`, keeping its place and
    /// comments. Fails with [`Problem::UnknownProvider`] when `replacing` is not configured.
    pub fn put_provider(
        &mut self,
        replacing: Option<&str>,
        entry: &ProviderEntry,
    ) -> Result<(), Problem> {
        if self.put_entry("providers", replacing, |table| write_provider(table, entry)) {
            Ok(())
        } else {
            Err(Problem::UnknownProvider(
                replacing.unwrap_or_default().to_owned(),
            ))
        }
    }

    /// Removes the provider with `id`. Returns whether it was configured.
    pub fn remove_provider(&mut self, id: &str) -> bool {
        self.remove_entry("providers", id)
    }

    /// Adds `entry`, or replaces the server with id `replacing`, keeping its place and comments.
    /// When the id changes, the server's permissions move to the new id. Fails with
    /// [`Problem::UnknownServer`] when `replacing` is not configured.
    pub fn put_server(
        &mut self,
        replacing: Option<&str>,
        entry: &ServerEntry,
    ) -> Result<(), Problem> {
        if !self.put_entry("servers", replacing, |table| write_server(table, entry)) {
            return Err(Problem::UnknownServer(
                replacing.unwrap_or_default().to_owned(),
            ));
        }
        if let Some(old) = replacing
            && old != entry.id
        {
            self.rename_permissions(old, &entry.id);
        }
        Ok(())
    }

    /// Removes the server with `id` and its permissions. Returns whether it was configured.
    pub fn remove_server(&mut self, id: &str) -> bool {
        let removed = self.remove_entry("servers", id);
        if let Some(permissions) = self.doc.get_mut("permissions").and_then(Item::as_table_mut) {
            permissions.remove(id);
        }
        removed
    }

    /// Sets the class of `server`'s `tool`, or removes it with `None`, which leaves the tool
    /// unclassified: hidden from the model and refused.
    pub fn set_permission(&mut self, server: &str, tool: &str, class: Option<PermissionClass>) {
        let permissions = self.doc.entry("permissions").or_insert_with(|| {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        });
        let Some(permissions) = permissions.as_table_mut() else {
            return;
        };
        match class {
            Some(class) => {
                let tools = permissions
                    .entry(server)
                    .or_insert_with(|| Item::Table(Table::new()));
                if let Some(tools) = tools.as_table_mut() {
                    tools.insert(tool, toml_edit::value(class.as_str()));
                }
            }
            None => {
                let now_empty = permissions
                    .get_mut(server)
                    .and_then(Item::as_table_mut)
                    .map(|tools| {
                        tools.remove(tool);
                        tools.is_empty()
                    });
                if now_empty == Some(true) {
                    permissions.remove(server);
                }
            }
        }
    }

    /// Validates the edited configuration with the same rules as [`Config::load`]. Field paths
    /// in errors, such as `servers[1].command`, refer to the edited document.
    pub fn validate(&self) -> Result<Config, ConfigError> {
        let base_dir = absolute_dir(self.path.parent()).map_err(|source| ConfigError::Read {
            path: self.path.clone(),
            source,
        })?;
        validate::parse(
            &self.doc.to_string(),
            &base_dir,
            Origin::File(self.path.clone()),
        )
    }

    /// Validates, then replaces the file atomically: the text is written to a temporary file in
    /// the same directory, flushed to disk and renamed over the original. The directory is
    /// created when missing.
    pub fn save(&self) -> Result<Config, ConfigError> {
        let config = self.validate()?;
        self.write().map_err(|source| ConfigError::Read {
            path: self.path.clone(),
            source,
        })?;
        Ok(config)
    }

    fn write(&self) -> io::Result<()> {
        let dir = absolute_dir(self.path.parent())?;
        std::fs::create_dir_all(&dir)?;
        let name = self
            .path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".{}.tmp", std::process::id()));
        let temp = dir.join(temp_name);
        let written = (|| {
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(self.doc.to_string().as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written
    }

    /// Writes a new `[[key]]` table, or the one with id `replacing`. Returns whether that
    /// table was found.
    fn put_entry(
        &mut self,
        key: &str,
        replacing: Option<&str>,
        write: impl FnOnce(&mut Table),
    ) -> bool {
        let mut tables = self.take_tables(key);
        let index = match replacing {
            Some(id) => tables
                .iter()
                .position(|table| table.get("id").and_then(Item::as_str) == Some(id)),
            None => {
                tables.push(Table::new());
                Some(tables.len() - 1)
            }
        };
        let table = index.and_then(|index| tables.get_mut(index));
        let found = table.is_some();
        if let Some(table) = table {
            write(table);
        }
        self.put_tables(key, tables);
        found
    }

    /// Removes the `[[key]]` table with `id`. Returns whether it was there.
    fn remove_entry(&mut self, key: &str, id: &str) -> bool {
        let mut tables = self.take_tables(key);
        let before = tables.len();
        tables.retain(|table| table.get("id").and_then(Item::as_str) != Some(id));
        let removed = tables.len() != before;
        self.put_tables(key, tables);
        removed
    }

    /// Takes the `[[key]]` tables out of the document; anything else under `key` is dropped,
    /// and validation would have rejected it.
    fn take_tables(&mut self, key: &str) -> ArrayOfTables {
        match self.doc.remove(key) {
            Some(Item::ArrayOfTables(tables)) => tables,
            _ => ArrayOfTables::new(),
        }
    }

    /// Puts the tables back. Each keeps its document position, so the file order is unchanged.
    fn put_tables(&mut self, key: &str, tables: ArrayOfTables) {
        if !tables.is_empty() {
            self.doc.insert(key, Item::ArrayOfTables(tables));
        }
    }

    fn rename_permissions(&mut self, old: &str, new: &str) {
        let Some(permissions) = self.doc.get_mut("permissions").and_then(Item::as_table_mut) else {
            return;
        };
        if let Some(tools) = permissions.remove(old) {
            permissions.insert(new, tools);
        }
    }
}

/// Writes `entry` into `table`, changing only keys whose value differs, so comments on
/// unchanged keys survive.
fn write_provider(table: &mut Table, entry: &ProviderEntry) {
    let kind = match entry.kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenAiCompatible => "openai_compatible",
    };
    set(table, "id", Some(Value::from(entry.id.as_str())));
    set(table, "kind", Some(Value::from(kind)));
    set(table, "model", Some(Value::from(entry.model.as_str())));
    set(
        table,
        "base_url",
        entry.base_url.as_deref().map(Value::from),
    );
    let api_key = entry.api_key_env.as_deref().map(|var| {
        let mut reference = InlineTable::new();
        reference.insert("env", Value::from(var));
        Value::InlineTable(reference)
    });
    set(table, "api_key", api_key);
}

/// Writes `entry` into `table`, changing only keys whose value differs, so comments on
/// unchanged keys survive.
fn write_server(table: &mut Table, entry: &ServerEntry) {
    set(table, "id", Some(Value::from(entry.id.as_str())));
    set(table, "transport", Some(Value::from("stdio")));
    set(table, "command", Some(Value::from(entry.command.as_str())));
    set(table, "args", non_empty(&entry.args).map(string_array));
    set(table, "cwd", entry.cwd.as_deref().map(Value::from));
    set(
        table,
        "env_allowlist",
        non_empty(&entry.env_allowlist).map(string_array),
    );
    let env = (!entry.env.is_empty()).then(|| {
        let mut env = InlineTable::new();
        for (name, var) in &entry.env {
            let mut reference = InlineTable::new();
            reference.insert("env", Value::from(var.as_str()));
            env.insert(name, Value::InlineTable(reference));
        }
        Value::InlineTable(env)
    });
    set(table, "env", env);
    set(
        table,
        "startup_timeout_ms",
        entry.startup_timeout_ms.map(ms_value),
    );
    set(
        table,
        "call_timeout_ms",
        entry.call_timeout_ms.map(ms_value),
    );
}

fn set(table: &mut Table, key: &str, value: Option<Value>) {
    match value {
        Some(value) => {
            let unchanged = table
                .get(key)
                .and_then(Item::as_value)
                .is_some_and(|old| old.to_string().trim() == value.to_string().trim());
            if !unchanged {
                table.insert(key, Item::Value(value));
            }
        }
        None => {
            table.remove(key);
        }
    }
}

fn non_empty(values: &[String]) -> Option<&[String]> {
    (!values.is_empty()).then_some(values)
}

fn string_array(values: &[String]) -> Value {
    Value::Array(values.iter().map(String::as_str).collect::<Array>())
}

/// Milliseconds as a TOML integer, saturating at `i64::MAX`; validation rejects such values.
fn ms_value(ms: u64) -> Value {
    Value::from(i64::try_from(ms).unwrap_or(i64::MAX))
}

fn line_column(source: &str, offset: usize) -> (usize, usize) {
    validate::line_column(source, offset)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use igor_core::policy::Classification;
    use igor_core::tools::{ServerId, ToolId};

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const COMMENTED: &str = r#"# Operator notes stay.
[[servers]]
id = "sales" # the shop
transport = "stdio"
command = "./sales-server"

# Second server.
[[servers]]
id = "notes"
transport = "stdio"
command = "./notes-server"

[permissions.sales]
get_stock_levels = "read" # safe
"#;

    fn temp_dir(name: &str) -> io::Result<PathBuf> {
        let dir = std::env::temp_dir()
            .join(format!("igor-config-edit-{}", std::process::id()))
            .join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn document(name: &str, source: &str) -> Result<ConfigDocument, Box<dyn std::error::Error>> {
        let path = temp_dir(name)?.join("config.toml");
        std::fs::write(&path, source)?;
        Ok(ConfigDocument::open(&path)?)
    }

    fn entry(id: &str) -> ServerEntry {
        ServerEntry {
            id: id.to_owned(),
            command: "./server".to_owned(),
            ..ServerEntry::default()
        }
    }

    fn tool(server: &str, name: &str) -> Result<ToolId, Box<dyn std::error::Error>> {
        Ok(ToolId {
            server: ServerId::new(server)?,
            name: name.to_owned(),
        })
    }

    #[test]
    fn missing_file_is_empty_and_save_creates_it() -> TestResult {
        let path = temp_dir("missing")?.join("nested/config.toml");
        let mut doc = ConfigDocument::open(&path)?;
        assert!(doc.servers()?.is_empty());
        doc.put_server(None, &entry("pos"))?;
        let config = doc.save()?;
        assert_eq!(config.servers().len(), 1);
        assert_eq!(Config::load(&path)?.servers()[0].id.as_str(), "pos");
        Ok(())
    }

    #[test]
    fn lists_servers_and_permissions() -> TestResult {
        let doc = document("list", COMMENTED)?;
        let ids: Vec<_> = doc.servers()?.into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["sales", "notes"]);
        assert_eq!(
            doc.permissions("sales")?,
            BTreeMap::from([("get_stock_levels".to_owned(), "read".to_owned())])
        );
        assert!(doc.permissions("notes")?.is_empty());
        Ok(())
    }

    #[test]
    fn edits_keep_comments_and_untouched_entries() -> TestResult {
        let mut doc = document("comments", COMMENTED)?;
        let mut notes = entry("notes");
        notes.command = "./notes-v2".to_owned();
        notes.args = vec!["--verbose".to_owned()];
        doc.put_server(Some("notes"), &notes)?;
        doc.put_server(None, &entry("pos"))?;
        doc.save()?;
        let text = std::fs::read_to_string(doc.path())?;
        for kept in [
            "# Operator notes stay.",
            "id = \"sales\" # the shop",
            "# Second server.",
            "get_stock_levels = \"read\" # safe",
        ] {
            assert!(text.contains(kept), "lost {kept:?}:\n{text}");
        }
        let servers = ConfigDocument::open(doc.path())?.servers()?;
        assert_eq!(servers[1], notes);
        assert_eq!(servers[2], entry("pos"));
        Ok(())
    }

    #[test]
    fn writes_every_field_and_secrets_only_as_references() -> TestResult {
        let mut doc = document("fields", "")?;
        let full = ServerEntry {
            id: "pos".to_owned(),
            command: "/usr/local/bin/pos-mcp".to_owned(),
            args: vec!["--store".to_owned(), "main".to_owned()],
            cwd: Some("data".to_owned()),
            env_allowlist: vec!["PATH".to_owned()],
            env: BTreeMap::from([("POS_TOKEN".to_owned(), "IGOR_POS_TOKEN".to_owned())]),
            startup_timeout_ms: Some(5_000),
            call_timeout_ms: Some(20_000),
        };
        doc.put_server(None, &full)?;
        let config = doc.save()?;
        let text = std::fs::read_to_string(doc.path())?;
        assert!(
            text.contains(r#"POS_TOKEN = { env = "IGOR_POS_TOKEN" }"#),
            "{text}"
        );
        assert_eq!(ConfigDocument::open(doc.path())?.servers()?, [full]);
        let server = &config.servers()[0];
        assert_eq!(server.startup_timeout, Duration::from_millis(5_000));
        assert_eq!(server.env["POS_TOKEN"].env_var(), "IGOR_POS_TOKEN");
        Ok(())
    }

    #[test]
    fn rename_moves_permissions_and_remove_drops_them() -> TestResult {
        let mut doc = document("rename", COMMENTED)?;
        doc.put_server(Some("sales"), &entry("shop"))?;
        let config = doc.validate()?;
        assert_eq!(
            config
                .permissions()
                .classify(&tool("shop", "get_stock_levels")?),
            Classification::Read
        );
        assert!(doc.remove_server("shop"));
        assert!(!doc.remove_server("shop"));
        assert!(doc.permissions("shop")?.is_empty());
        assert_eq!(doc.validate()?.servers().len(), 1);
        Ok(())
    }

    #[test]
    fn replacing_an_unknown_server_fails() -> TestResult {
        let mut doc = document("unknown", COMMENTED)?;
        assert_eq!(
            doc.put_server(Some("nope"), &entry("x")),
            Err(Problem::UnknownServer("nope".to_owned()))
        );
        Ok(())
    }

    #[test]
    fn set_and_clear_permissions() -> TestResult {
        let mut doc = document("permissions", COMMENTED)?;
        doc.set_permission("notes", "append", Some(PermissionClass::Write));
        doc.set_permission("sales", "get_stock_levels", None);
        let permissions = doc.validate()?.permissions().clone();
        assert_eq!(
            permissions.classify(&tool("notes", "append")?),
            Classification::Write
        );
        assert_eq!(
            permissions.classify(&tool("sales", "get_stock_levels")?),
            Classification::Unclassified
        );
        assert!(!doc.doc.to_string().contains("[permissions.sales]"));
        Ok(())
    }

    #[test]
    fn invalid_edits_are_reported_with_their_field_and_not_saved() -> TestResult {
        let mut doc = document("invalid", COMMENTED)?;
        let before = std::fs::read_to_string(doc.path())?;
        let mut bad = entry("sales");
        bad.command = String::new();
        doc.put_server(Some("notes"), &bad)?;
        let err = doc.save().err().ok_or("saved an invalid configuration")?;
        assert_eq!(err.field(), Some("servers[1].id"));
        assert!(matches!(err.problem(), Some(Problem::DuplicateServerId(_))));
        assert_eq!(std::fs::read_to_string(doc.path())?, before);

        let mut doc = document("invalid-env", "")?;
        let mut bad = entry("pos");
        bad.env = BTreeMap::from([("TOKEN".to_owned(), "not a name".to_owned())]);
        doc.put_server(None, &bad)?;
        let err = doc.validate().err().ok_or("accepted a bad reference")?;
        assert_eq!(err.field(), Some("servers[0].env.TOKEN"));
        assert!(!err.to_string().contains("not a name"));
        Ok(())
    }

    const PROVIDERS: &str = r#"# Providers.
[[providers]]
id = "lm_studio" # local
kind = "openai_compatible"
base_url = "http://127.0.0.1:1234/v1"
model = "qwen/qwen3-vl-8b"

[[providers]]
id = "claude"
kind = "anthropic"
model = "claude-haiku-5-5"
api_key = { env = "ANTHROPIC_API_KEY" }

[[servers]]
id = "sales"
transport = "stdio"
command = "./sales-server"
"#;

    fn claude(id: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.to_owned(),
            kind: ProviderKind::Anthropic,
            model: "claude-sonnet-5-5".to_owned(),
            base_url: None,
            api_key_env: Some("IGOR_CLAUDE_KEY".to_owned()),
        }
    }

    #[test]
    fn lists_providers_with_key_references_only() -> TestResult {
        let doc = document("providers-list", PROVIDERS)?;
        let providers = doc.providers()?;
        assert_eq!(
            providers,
            [
                ProviderEntry {
                    id: "lm_studio".to_owned(),
                    kind: ProviderKind::OpenAiCompatible,
                    model: "qwen/qwen3-vl-8b".to_owned(),
                    base_url: Some("http://127.0.0.1:1234/v1".to_owned()),
                    api_key_env: None,
                },
                ProviderEntry {
                    model: "claude-haiku-5-5".to_owned(),
                    api_key_env: Some("ANTHROPIC_API_KEY".to_owned()),
                    ..claude("claude")
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn provider_edits_keep_comments_and_write_keys_as_references() -> TestResult {
        let mut doc = document("providers-edit", PROVIDERS)?;
        doc.put_provider(Some("claude"), &claude("claude"))?;
        doc.put_provider(None, &claude("claude_big"))?;
        let config = doc.save()?;
        let text = std::fs::read_to_string(doc.path())?;
        assert!(text.contains("# Providers."), "{text}");
        assert!(text.contains(r#"id = "lm_studio" # local"#), "{text}");
        assert!(
            text.contains(r#"api_key = { env = "IGOR_CLAUDE_KEY" }"#),
            "{text}"
        );
        assert_eq!(config.providers().len(), 3);
        assert_eq!(
            config.provider("claude").map(|p| p.model.as_str()),
            Some("claude-sonnet-5-5")
        );
        let reread = ConfigDocument::open(doc.path())?.providers()?;
        assert_eq!(reread[1], claude("claude"));
        assert_eq!(reread[2], claude("claude_big"));
        assert_eq!(ConfigDocument::open(doc.path())?.servers()?.len(), 1);
        Ok(())
    }

    #[test]
    fn remove_and_replace_unknown_provider() -> TestResult {
        let mut doc = document("providers-remove", PROVIDERS)?;
        assert_eq!(
            doc.put_provider(Some("nope"), &claude("x")),
            Err(Problem::UnknownProvider("nope".to_owned()))
        );
        assert!(doc.remove_provider("lm_studio"));
        assert!(!doc.remove_provider("lm_studio"));
        let ids: Vec<_> = doc.providers()?.into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["claude"]);
        assert_eq!(doc.validate()?.providers().len(), 1);
        Ok(())
    }

    #[test]
    fn invalid_provider_edits_are_reported_with_their_field() -> TestResult {
        let mut doc = document("providers-invalid", PROVIDERS)?;
        let mut keyless = claude("claude");
        keyless.api_key_env = None;
        doc.put_provider(Some("claude"), &keyless)?;
        let err = doc
            .validate()
            .err()
            .ok_or("accepted anthropic without a key")?;
        assert_eq!(err.field(), Some("providers[1].api_key"));

        let mut doc = document("providers-bad-key", PROVIDERS)?;
        let mut bad = claude("claude");
        bad.api_key_env = Some("sk-ant-secret value".to_owned());
        doc.put_provider(Some("claude"), &bad)?;
        let err = doc.validate().err().ok_or("accepted a bad key reference")?;
        assert_eq!(err.field(), Some("providers[1].api_key"));
        assert!(!err.to_string().contains("sk-ant-secret"));

        let mut doc = document("providers-bad-url", PROVIDERS)?;
        let mut remote_http = claude("claude");
        remote_http.base_url = Some("http://example.com/v1".to_owned());
        doc.put_provider(Some("claude"), &remote_http)?;
        let err = doc
            .validate()
            .err()
            .ok_or("accepted plain http to a remote host")?;
        assert_eq!(err.field(), Some("providers[1].base_url"));
        Ok(())
    }
}
