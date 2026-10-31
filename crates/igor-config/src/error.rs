//! Configuration errors.
//!
//! Every error names where the problem is (file and line, or file and field path) and what is
//! wrong. No error contains a secret value: literal secrets are dropped during parsing and secret
//! reference values are never echoed.

use std::fmt;
use std::io;
use std::path::PathBuf;

use igor_core::tools::{InvalidServerId, ServerId};
use thiserror::Error;

/// A configuration file could not be read, parsed or validated.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("{}: cannot read configuration: {source}", path.display())]
    Read {
        /// Path that was read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// The file is not valid TOML or does not match the schema (missing or unknown keys, wrong
    /// value types).
    #[error("{origin}:{line}:{column}: {message}")]
    Parse {
        /// File the configuration came from.
        origin: Origin,
        /// 1-based line of the problem.
        line: usize,
        /// 1-based column of the problem.
        column: usize,
        /// Parser message.
        message: String,
    },
    /// The file parsed but a value is not acceptable.
    #[error("{origin}: {field}: {problem}")]
    Invalid {
        /// File the configuration came from.
        origin: Origin,
        /// Path of the offending field, such as `servers[1].id`.
        field: String,
        /// What is wrong.
        problem: Problem,
    },
}

impl ConfigError {
    /// The validation problem, when this is a validation error.
    pub fn problem(&self) -> Option<&Problem> {
        match self {
            Self::Invalid { problem, .. } => Some(problem),
            Self::Read { .. } | Self::Parse { .. } => None,
        }
    }

    /// The offending field path, when this is a validation error.
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::Invalid { field, .. } => Some(field),
            Self::Read { .. } | Self::Parse { .. } => None,
        }
    }
}

/// Where a configuration came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Loaded from this file.
    File(PathBuf),
    /// Parsed from a string supplied in code.
    Inline,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => write!(f, "{}", path.display()),
            Self::Inline => f.write_str("<inline configuration>"),
        }
    }
}

/// A validation problem with one configuration value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Problem {
    /// A required value is empty.
    #[error("must not be empty")]
    Empty,
    /// A value required by the provider kind is missing.
    #[error("required for provider kind `{0}`")]
    RequiredForKind(&'static str),
    /// The provider kind is not supported.
    #[error("unknown provider kind {0:?}: expected \"anthropic\" or \"openai_compatible\"")]
    UnknownProviderKind(String),
    /// A provider id is malformed.
    #[error(
        "invalid provider id {0:?}: use lowercase letters, digits, '-' or '_', starting with a letter, at most {max} characters",
        max = crate::validate::MAX_PROVIDER_ID_LEN
    )]
    InvalidProviderId(String),
    /// Two providers share an id.
    #[error("provider id {0:?} is declared more than once")]
    DuplicateProviderId(String),
    /// A base URL is not an `http` or `https` URL, or embeds credentials.
    #[error("invalid base URL: {0}")]
    InvalidUrl(&'static str),
    /// A server id is malformed.
    #[error(transparent)]
    InvalidServerId(#[from] InvalidServerId),
    /// Two servers share an id.
    #[error("server id {0:?} is declared more than once")]
    DuplicateServerId(ServerId),
    /// The server transport is not supported.
    #[error("unsupported transport {0:?}: only \"stdio\" is supported")]
    UnsupportedTransport(String),
    /// A secret field holds a literal value instead of a reference. The value is not shown.
    #[error(
        "literal secret values are not allowed (value not shown); use {{ env = \"VARIABLE_NAME\" }}"
    )]
    LiteralSecret,
    /// A secret reference does not name a valid environment variable. The value is not shown.
    #[error(
        "secret reference must name an environment variable matching [A-Z_][A-Z0-9_]* (value not shown)"
    )]
    InvalidSecretReference,
    /// An environment variable name is malformed.
    #[error("invalid environment variable name {0:?}: expected [A-Z_][A-Z0-9_]*")]
    InvalidEnvName(String),
    /// An environment variable name appears twice.
    #[error("environment variable {0} is listed more than once")]
    DuplicateEnvName(String),
    /// A variable is both inherited through the allowlist and set by reference.
    #[error("environment variable {0} is both allowlisted and set in `env`")]
    EnvNameConflict(String),
    /// A server would receive a provider's API key.
    #[error("environment variable {0} holds a provider API key and must not be passed to a server")]
    ExposesProviderKey(String),
    /// An edit names a provider that is not declared.
    #[error("undeclared provider {0:?}")]
    UnknownProvider(String),
    /// A permission refers to a server that is not declared.
    #[error("permission for undeclared server {0:?}")]
    UnknownServer(String),
    /// A tool name is malformed.
    #[error("invalid tool name {0:?}: use 1 to 128 ASCII letters, digits, '_', '-' or '.'")]
    InvalidToolName(String),
    /// A permission class is not one of `read`, `write` or `denied`.
    #[error("unknown permission class {0:?}: expected \"read\", \"write\" or \"denied\"")]
    UnknownPermissionClass(String),
    /// A number is outside its allowed range.
    #[error("{value} is out of range: expected {min} to {max}")]
    OutOfRange {
        /// Value given.
        value: u64,
        /// Smallest allowed value.
        min: u64,
        /// Largest allowed value.
        max: u64,
    },
}
