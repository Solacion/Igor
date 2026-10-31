//! Turns raw configuration into validated settings.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use igor_core::policy::Classification;
use igor_core::tools::ServerId;

use crate::raw::{RawConfig, RawLimits, RawProvider, RawSecret, RawServer};
use crate::secret::{SecretRef, is_env_name};
use crate::{
    Config, ConfigError, DEFAULT_STARTUP_TIMEOUT, Limits, Origin, Problem, ProviderConfig,
    ProviderKind, ServerConfig, ToolPermissions,
};

const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const MAX_ITERATIONS: RangeInclusive<u64> = 1..=100;
const RESULT_BYTES: RangeInclusive<u64> = 1024..=16 * 1024 * 1024;
const CALL_TIMEOUT_MS: RangeInclusive<u64> = 100..=600_000;
const STARTUP_TIMEOUT_MS: RangeInclusive<u64> = 100..=120_000;
const MAX_TOOL_NAME_LEN: usize = 128;

/// A problem at a field path; the origin is attached by [`parse`].
type Invalid = (String, Problem);

pub(crate) fn parse(source: &str, base_dir: &Path, origin: Origin) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(source).map_err(|err| {
        let (line, column) = err
            .span()
            .map_or((1, 1), |span| line_column(source, span.start));
        ConfigError::Parse {
            origin: origin.clone(),
            line,
            column,
            message: hide_quoted_values(err.message()),
        }
    })?;
    validate(raw, base_dir).map_err(|(field, problem)| ConfigError::Invalid {
        origin,
        field,
        problem,
    })
}

/// Hides double-quoted values in a parser message, such as the string in
/// `invalid type: string "...", expected u64`, so a secret typed into the wrong field is not
/// echoed. Everything from the first to the last `"` is replaced as one unit, so escaped quotes
/// inside a value cannot expose part of it. Field names, which the parser quotes with backticks,
/// stay visible.
pub(crate) fn hide_quoted_values(message: &str) -> String {
    match (message.find('"'), message.rfind('"')) {
        (Some(first), Some(last)) if first < last => format!(
            "{}\"[value not shown]\"{}",
            &message[..first],
            &message[last + 1..]
        ),
        _ => message.to_owned(),
    }
}

pub(crate) fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let before = source.get(..offset).unwrap_or(source);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

fn validate(raw: RawConfig, base_dir: &Path) -> Result<Config, Invalid> {
    let limits = limits(&raw.limits)?;
    let providers = providers(raw.providers)?;
    let provider_keys: BTreeSet<&str> = providers
        .iter()
        .filter_map(|p| p.api_key.as_ref().map(SecretRef::env_var))
        .collect();
    let servers = servers(raw.servers, base_dir, &provider_keys, &limits)?;
    let permissions = permissions(raw.permissions, &servers)?;
    Ok(Config {
        providers,
        servers,
        permissions,
        limits,
    })
}

fn in_range(
    value: u64,
    range: &RangeInclusive<u64>,
    field: impl Fn() -> String,
) -> Result<u64, Invalid> {
    if range.contains(&value) {
        Ok(value)
    } else {
        Err((
            field(),
            Problem::OutOfRange {
                value,
                min: *range.start(),
                max: *range.end(),
            },
        ))
    }
}

fn limits(raw: &RawLimits) -> Result<Limits, Invalid> {
    let defaults = Limits::default();
    let mut limits = defaults;
    if let Some(value) = raw.max_iterations {
        let value = in_range(value, &MAX_ITERATIONS, || "limits.max_iterations".into())?;
        limits.max_iterations = u32::try_from(value).unwrap_or(defaults.max_iterations);
    }
    if let Some(value) = raw.max_result_bytes {
        let value = in_range(value, &RESULT_BYTES, || "limits.max_result_bytes".into())?;
        limits.max_result_bytes = usize::try_from(value).unwrap_or(defaults.max_result_bytes);
    }
    if let Some(value) = raw.tool_call_timeout_ms {
        let value = in_range(value, &CALL_TIMEOUT_MS, || {
            "limits.tool_call_timeout_ms".into()
        })?;
        limits.tool_call_timeout = Duration::from_millis(value);
    }
    Ok(limits)
}

/// Maximum length of a provider id.
pub(crate) const MAX_PROVIDER_ID_LEN: usize = 32;

/// Provider ids: lowercase ASCII letters, digits, `-` and `_`, starting with a letter, at most
/// [`MAX_PROVIDER_ID_LEN`] characters.
fn is_provider_id(id: &str) -> bool {
    id.len() <= MAX_PROVIDER_ID_LEN
        && id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn providers(raw: Vec<RawProvider>) -> Result<Vec<ProviderConfig>, Invalid> {
    let mut seen = BTreeSet::new();
    raw.into_iter()
        .enumerate()
        .map(|(index, raw)| {
            let field = |name: &str| format!("providers[{index}].{name}");
            if !is_provider_id(&raw.id) {
                return Err((field("id"), Problem::InvalidProviderId(raw.id)));
            }
            if !seen.insert(raw.id.clone()) {
                return Err((field("id"), Problem::DuplicateProviderId(raw.id)));
            }
            let (kind, kind_name) = match raw.kind.as_str() {
                "anthropic" => (ProviderKind::Anthropic, "anthropic"),
                "openai_compatible" => (ProviderKind::OpenAiCompatible, "openai_compatible"),
                _ => return Err((field("kind"), Problem::UnknownProviderKind(raw.kind))),
            };
            if raw.model.trim().is_empty() {
                return Err((field("model"), Problem::Empty));
            }
            let base_url = match (raw.base_url, kind) {
                (Some(url), _) => url,
                (None, ProviderKind::Anthropic) => ANTHROPIC_DEFAULT_BASE_URL.to_owned(),
                (None, ProviderKind::OpenAiCompatible) => {
                    return Err((field("base_url"), Problem::RequiredForKind(kind_name)));
                }
            };
            check_base_url(&base_url).map_err(|problem| (field("base_url"), problem))?;
            let api_key = raw
                .api_key
                .map(|secret| secret_ref(secret, || field("api_key")))
                .transpose()?;
            if kind == ProviderKind::Anthropic && api_key.is_none() {
                return Err((field("api_key"), Problem::RequiredForKind(kind_name)));
            }
            Ok(ProviderConfig {
                id: raw.id,
                kind,
                model: raw.model,
                base_url,
                api_key,
            })
        })
        .collect()
}

fn check_base_url(url: &str) -> Result<(), Problem> {
    let (rest, plain_http) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => (rest, false),
        (None, Some(rest)) => (rest, true),
        (None, None) => return Err(Problem::InvalidUrl("must start with http:// or https://")),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(Problem::InvalidUrl("missing host"));
    }
    if authority.contains('@') {
        return Err(Problem::InvalidUrl(
            "must not embed credentials; use api_key with an env reference",
        ));
    }
    // Plain HTTP would send API keys unencrypted, so allow it only for this machine.
    if plain_http && !is_loopback_host(authority) {
        return Err(Problem::InvalidUrl(
            "plain http:// is only allowed for localhost; use https://",
        ));
    }
    Ok(())
}

/// True when `authority` (host with optional port) names this machine.
fn is_loopback_host(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn secret_ref(raw: RawSecret, field: impl FnOnce() -> String) -> Result<SecretRef, Invalid> {
    match raw {
        RawSecret::Literal => Err((field(), Problem::LiteralSecret)),
        RawSecret::Env(name) if is_env_name(&name) => Ok(SecretRef::from_validated_env(name)),
        RawSecret::Env(_) => Err((field(), Problem::InvalidSecretReference)),
    }
}

fn servers(
    raw: Vec<RawServer>,
    base_dir: &Path,
    provider_keys: &BTreeSet<&str>,
    limits: &Limits,
) -> Result<Vec<ServerConfig>, Invalid> {
    let mut seen = BTreeSet::new();
    raw.into_iter()
        .enumerate()
        .map(|(index, raw)| {
            let field = |name: &str| format!("servers[{index}].{name}");
            let id = ServerId::new(raw.id).map_err(|err| (field("id"), err.into()))?;
            if !seen.insert(id.clone()) {
                return Err((field("id"), Problem::DuplicateServerId(id)));
            }
            if raw.transport != "stdio" {
                return Err((
                    field("transport"),
                    Problem::UnsupportedTransport(raw.transport),
                ));
            }
            if raw.command.trim().is_empty() {
                return Err((field("command"), Problem::Empty));
            }
            let env = server_env(raw.env, provider_keys, |name| field(&format!("env.{name}")))?;
            check_allowlist(&raw.env_allowlist, &env, provider_keys, |i| {
                field(&format!("env_allowlist[{i}]"))
            })?;
            let startup_timeout = match raw.startup_timeout_ms {
                Some(ms) => Duration::from_millis(in_range(ms, &STARTUP_TIMEOUT_MS, || {
                    field("startup_timeout_ms")
                })?),
                None => DEFAULT_STARTUP_TIMEOUT,
            };
            let call_timeout = match raw.call_timeout_ms {
                Some(ms) => Duration::from_millis(in_range(ms, &CALL_TIMEOUT_MS, || {
                    field("call_timeout_ms")
                })?),
                None => limits.tool_call_timeout,
            };
            Ok(ServerConfig {
                id,
                command: resolve_command(&raw.command, base_dir),
                args: raw.args,
                cwd: raw
                    .cwd
                    .map_or_else(|| base_dir.to_path_buf(), |cwd| base_dir.join(cwd)),
                env_allowlist: raw.env_allowlist,
                env,
                startup_timeout,
                call_timeout,
            })
        })
        .collect()
}

fn server_env(
    raw: BTreeMap<String, RawSecret>,
    provider_keys: &BTreeSet<&str>,
    field: impl Fn(&str) -> String,
) -> Result<BTreeMap<String, SecretRef>, Invalid> {
    raw.into_iter()
        .map(|(name, secret)| {
            if !is_env_name(&name) {
                return Err((field(&name), Problem::InvalidEnvName(name)));
            }
            let reference = secret_ref(secret, || field(&name))?;
            if provider_keys.contains(reference.env_var()) {
                let var = reference.env_var().to_owned();
                return Err((field(&name), Problem::ExposesProviderKey(var)));
            }
            Ok((name, reference))
        })
        .collect()
}

fn check_allowlist(
    allowlist: &[String],
    env: &BTreeMap<String, SecretRef>,
    provider_keys: &BTreeSet<&str>,
    field: impl Fn(usize) -> String,
) -> Result<(), Invalid> {
    let mut seen = BTreeSet::new();
    for (index, name) in allowlist.iter().enumerate() {
        let problem = if !is_env_name(name) {
            Problem::InvalidEnvName(name.clone())
        } else if !seen.insert(name.as_str()) {
            Problem::DuplicateEnvName(name.clone())
        } else if env.contains_key(name) {
            Problem::EnvNameConflict(name.clone())
        } else if provider_keys.contains(name.as_str()) {
            Problem::ExposesProviderKey(name.clone())
        } else {
            continue;
        };
        return Err((field(index), problem));
    }
    Ok(())
}

/// Resolves a relative command with a directory component against the configuration directory.
/// A bare program name is left for `PATH` lookup.
fn resolve_command(command: &str, base_dir: &Path) -> PathBuf {
    let path = Path::new(command);
    let has_dir = path.components().count() > 1
        || matches!(
            path.components().next(),
            Some(Component::CurDir | Component::ParentDir)
        );
    if path.is_relative() && has_dir {
        base_dir.join(path)
    } else {
        path.to_path_buf()
    }
}

fn permissions(
    raw: BTreeMap<String, BTreeMap<String, String>>,
    servers: &[ServerConfig],
) -> Result<ToolPermissions, Invalid> {
    let mut by_server = BTreeMap::new();
    for (server, tools) in raw {
        let id = servers
            .iter()
            .map(|s| &s.id)
            .find(|id| id.as_str() == server)
            .cloned()
            .ok_or_else(|| {
                (
                    format!("permissions.{server}"),
                    Problem::UnknownServer(server.clone()),
                )
            })?;
        let mut classes = BTreeMap::new();
        for (tool, class) in tools {
            let field = || format!("permissions.{server}.{tool}");
            if !is_tool_name(&tool) {
                return Err((field(), Problem::InvalidToolName(tool)));
            }
            let class = match class.as_str() {
                "read" => Classification::Read,
                "write" => Classification::Write,
                "denied" => Classification::Denied,
                _ => return Err((field(), Problem::UnknownPermissionClass(class))),
            };
            classes.insert(tool, class);
        }
        by_server.insert(id, classes);
    }
    Ok(ToolPermissions { by_server })
}

fn is_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_TOOL_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_values_are_hidden_but_field_names_stay() {
        assert_eq!(
            hide_quoted_values(r#"invalid type: string "sk-ant-x", expected u64"#),
            r#"invalid type: string "[value not shown]", expected u64"#
        );
        assert_eq!(
            hide_quoted_values("missing field `command`"),
            "missing field `command`"
        );
    }

    #[test]
    fn values_with_escaped_quotes_are_hidden_whole() {
        assert_eq!(
            hide_quoted_values(r#"invalid type: string "ab\"cd", expected u64"#),
            r#"invalid type: string "[value not shown]", expected u64"#
        );
    }

    #[test]
    fn line_column_is_one_based() {
        let source = "a = 1\nbb = 2\n";
        assert_eq!(line_column(source, 0), (1, 1));
        assert_eq!(line_column(source, 7), (2, 2));
    }

    #[test]
    fn commands_resolve_against_config_dir_only_with_a_directory() {
        let base = Path::new("/cfg");
        assert_eq!(resolve_command("node", base), PathBuf::from("node"));
        assert_eq!(
            resolve_command("./bin/server", base),
            PathBuf::from("/cfg/./bin/server")
        );
        assert_eq!(
            resolve_command("../target/debug/server", base),
            PathBuf::from("/cfg/../target/debug/server")
        );
        assert_eq!(
            resolve_command("/usr/bin/x", base),
            PathBuf::from("/usr/bin/x")
        );
    }
}
