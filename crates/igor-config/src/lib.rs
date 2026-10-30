//! IGOR host configuration.
//!
//! Loads and validates the local TOML configuration: model providers, MCP servers, tool
//! permissions and limits. The result is typed settings for the composition root; `igor-core`
//! never reads configuration files.
//!
//! Secrets are never stored in configuration, only referenced by environment variable name (see
//! [`SecretRef`]). Tool classification comes only from this configuration: a tool without an
//! entry is [`Classification::Unclassified`] and therefore denied.

mod error;
mod raw;
mod secret;
mod validate;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use igor_core::policy::Classification;
use igor_core::tools::{ServerId, ToolId};

pub use error::{ConfigError, Origin, Problem};
pub use secret::{REDACTED, Secret, SecretError, SecretRef};

/// Environment variable that overrides the default configuration path.
pub const CONFIG_PATH_ENV: &str = "IGOR_CONFIG";

/// Default configuration path relative to the home directory (macOS).
pub const DEFAULT_CONFIG_PATH: &str = "Library/Application Support/igor/config.toml";

/// Validated IGOR configuration.
#[derive(Debug, Clone)]
pub struct Config {
    providers: Vec<ProviderConfig>,
    servers: Vec<ServerConfig>,
    permissions: ToolPermissions,
    limits: Limits,
}

impl Config {
    /// Reads and validates the configuration file at `path`. Relative server paths resolve
    /// against the file's directory, made absolute, so they do not depend on how `path` was
    /// written.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let read_error = |source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        };
        let source = std::fs::read_to_string(path).map_err(read_error)?;
        let base_dir = absolute_dir(path.parent()).map_err(read_error)?;
        validate::parse(&source, &base_dir, Origin::File(path.to_path_buf()))
    }

    /// Parses and validates configuration text. Relative server paths resolve against
    /// `base_dir`, made absolute against the current directory.
    pub fn from_toml_str(source: &str, base_dir: &Path) -> Result<Self, ConfigError> {
        let absolute = absolute_dir(Some(base_dir)).map_err(|source| ConfigError::Read {
            path: base_dir.to_path_buf(),
            source,
        })?;
        validate::parse(source, &absolute, Origin::Inline)
    }

    /// Configured model providers, in file order.
    pub fn providers(&self) -> &[ProviderConfig] {
        &self.providers
    }

    /// The provider with this id.
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Configured MCP servers, in file order.
    pub fn servers(&self) -> &[ServerConfig] {
        &self.servers
    }

    /// The server with this id.
    pub fn server(&self, id: &ServerId) -> Option<&ServerConfig> {
        self.servers.iter().find(|s| &s.id == id)
    }

    /// Host tool classifications.
    pub fn permissions(&self) -> &ToolPermissions {
        &self.permissions
    }

    /// Agent execution limits.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
}

/// Makes `dir` absolute; an empty or missing directory means the current directory.
fn absolute_dir(dir: Option<&Path>) -> std::io::Result<PathBuf> {
    match dir {
        Some(dir) if !dir.as_os_str().is_empty() => std::path::absolute(dir),
        _ => std::env::current_dir(),
    }
}

/// Supported model provider kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Anthropic Messages API.
    Anthropic,
    /// OpenAI-compatible chat completions API, such as LM Studio's local server.
    OpenAiCompatible,
}

/// A configured model provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Operator-chosen identifier.
    pub id: String,
    /// API flavour.
    pub kind: ProviderKind,
    /// Model name sent to the provider.
    pub model: String,
    /// API base URL (`http` or `https`, without credentials).
    pub base_url: String,
    /// API key reference. Required for [`ProviderKind::Anthropic`].
    pub api_key: Option<SecretRef>,
}

/// A configured MCP server, started as a stdio child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Host-assigned server id.
    pub id: ServerId,
    /// Program to run. A relative path with a directory component is resolved against the
    /// configuration directory; a bare name is looked up on `PATH` at spawn time.
    pub command: PathBuf,
    /// Program arguments.
    pub args: Vec<String>,
    /// Working directory, resolved against the configuration directory.
    pub cwd: PathBuf,
    /// Names of IGOR's environment variables the child may inherit. Nothing else is inherited.
    pub env_allowlist: Vec<String>,
    /// Variables set in the child, each read from a secret reference at spawn time.
    pub env: BTreeMap<String, SecretRef>,
    /// Bound on `initialize` plus `tools/list`.
    pub startup_timeout: Duration,
    /// Bound on each tool call (the server's override or [`Limits::tool_call_timeout`]).
    pub call_timeout: Duration,
}

/// Host tool classifications, keyed by server and exact tool name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPermissions {
    by_server: BTreeMap<ServerId, BTreeMap<String, Classification>>,
}

impl ToolPermissions {
    /// Classification of `tool`; [`Classification::Unclassified`] when the host has none.
    pub fn classify(&self, tool: &ToolId) -> Classification {
        self.by_server
            .get(&tool.server)
            .and_then(|tools| tools.get(&tool.name))
            .copied()
            .unwrap_or(Classification::Unclassified)
    }
}

/// Bounds on agent execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximum model turns per request.
    pub max_iterations: u32,
    /// Maximum size of one tool result passed to the model, in bytes.
    pub max_result_bytes: usize,
    /// Default bound on each tool call.
    pub tool_call_timeout: Duration,
}

impl Limits {
    /// Default [`Limits::max_iterations`].
    pub const DEFAULT_MAX_ITERATIONS: u32 = 10;
    /// Default [`Limits::max_result_bytes`] (32 KiB).
    pub const DEFAULT_MAX_RESULT_BYTES: usize = 32 * 1024;
    /// Default [`Limits::tool_call_timeout`].
    pub const DEFAULT_TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(30);
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_iterations: Self::DEFAULT_MAX_ITERATIONS,
            max_result_bytes: Self::DEFAULT_MAX_RESULT_BYTES,
            tool_call_timeout: Self::DEFAULT_TOOL_CALL_TIMEOUT,
        }
    }
}

/// Default bound on server startup (`initialize` plus `tools/list`).
pub const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Chooses the configuration path: `explicit` (a command-line flag) wins, then
/// [`CONFIG_PATH_ENV`], then [`DEFAULT_CONFIG_PATH`] under `$HOME`. `None` when no source applies.
pub fn config_path(explicit: Option<&Path>) -> Option<PathBuf> {
    config_path_with(explicit, |name| std::env::var_os(name))
}

fn config_path_with(
    explicit: Option<&Path>,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let non_empty = |name: &str| lookup(name).filter(|value| !value.is_empty());
    explicit
        .map(Path::to_path_buf)
        .or_else(|| non_empty(CONFIG_PATH_ENV).map(PathBuf::from))
        .or_else(|| non_empty("HOME").map(|home| PathBuf::from(home).join(DEFAULT_CONFIG_PATH)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_precedence() {
        let env = |name: &str| match name {
            CONFIG_PATH_ENV => Some(OsString::from("/env/igor.toml")),
            "HOME" => Some(OsString::from("/home/op")),
            _ => None,
        };
        let home_only = |name: &str| (name == "HOME").then(|| OsString::from("/home/op"));

        assert_eq!(
            config_path_with(Some(Path::new("/flag.toml")), env),
            Some(PathBuf::from("/flag.toml"))
        );
        assert_eq!(
            config_path_with(None, env),
            Some(PathBuf::from("/env/igor.toml"))
        );
        assert_eq!(
            config_path_with(None, home_only),
            Some(PathBuf::from(
                "/home/op/Library/Application Support/igor/config.toml"
            ))
        );
        assert_eq!(config_path_with(None, |_| None), None);
    }
}
