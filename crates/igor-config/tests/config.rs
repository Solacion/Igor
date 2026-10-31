//! Loader tests: valid and invalid configurations, permissions and secret handling.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::Duration;

use igor_config::{Config, ConfigError, Limits, Problem, ProviderKind};
use igor_core::policy::Classification;
use igor_core::tools::{InvalidServerId, ServerId, ToolId};

type TestResult = Result<(), Box<dyn Error>>;

const SENTINEL: &str = "IGOR-TEST-SECRET-7f3a";

const BASE: &str = r#"
[[providers]]
id = "claude"
kind = "anthropic"
model = "m"
api_key = { env = "ANTHROPIC_API_KEY" }

[[servers]]
id = "sales"
transport = "stdio"
command = "./server"

[[servers]]
id = "inventory"
transport = "stdio"
command = "./server"
"#;

fn parse(source: &str) -> Result<Config, ConfigError> {
    Config::from_toml_str(source, Path::new("/cfg"))
}

fn tool(server: &str, name: &str) -> Result<ToolId, InvalidServerId> {
    Ok(ToolId {
        server: ServerId::new(server)?,
        name: name.into(),
    })
}

fn sample_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/igor.toml")
}

#[test]
fn sample_config_loads_and_needs_no_credentials() -> TestResult {
    let config = Config::load(&sample_path())?;

    let ids: Vec<&str> = config.servers().iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["sales", "supplier"]);
    for server in config.servers() {
        assert!(server.command.ends_with("target/debug/igor-fixture-server"));
        assert!(server.env.is_empty(), "fixture servers need no secrets");
    }
    let permissions = config.permissions();
    assert_eq!(
        permissions.classify(&tool("sales", "create_draft_order")?),
        Classification::Write
    );
    for name in ["get_stock_levels", "get_sales_history"] {
        assert_eq!(
            permissions.classify(&tool("sales", name)?),
            Classification::Read
        );
    }
    assert_eq!(
        permissions.classify(&tool("supplier", "get_catalogue")?),
        Classification::Read
    );
    let local = config.provider("lm_studio").ok_or("lm_studio missing")?;
    assert_eq!(local.kind, ProviderKind::OpenAiCompatible);
    assert!(local.api_key.is_none());
    Ok(())
}

#[test]
fn relative_config_paths_resolve_to_absolute_server_paths() -> TestResult {
    // Tests run in the crate directory, so this path is relative to it.
    let config = Config::load(Path::new("../../examples/igor.toml"))?;
    for server in config.servers() {
        assert!(server.cwd.is_absolute(), "{}", server.cwd.display());
        assert!(server.command.is_absolute(), "{}", server.command.display());
        assert!(server.cwd.ends_with("examples"));
    }
    let inline = Config::from_toml_str(BASE, Path::new(""))?;
    let sales = inline
        .server(&ServerId::new("sales")?)
        .ok_or("sales missing")?;
    assert_eq!(sales.cwd, std::env::current_dir()?);
    Ok(())
}

#[test]
fn parse_errors_do_not_echo_values() -> TestResult {
    let source = format!("{BASE}\n[limits]\nmax_iterations = \"{SENTINEL}\"\n");
    let err = parse(&source).err().ok_or("expected an error")?;
    assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    let message = err.to_string();
    assert!(!message.contains(SENTINEL), "{message}");
    assert!(message.contains("[value not shown]"), "{message}");
    Ok(())
}

#[test]
fn defaults_apply_when_omitted() -> TestResult {
    let config = parse(BASE)?;
    assert_eq!(*config.limits(), Limits::default());
    let sales = config
        .server(&ServerId::new("sales")?)
        .ok_or("sales missing")?;
    assert_eq!(sales.call_timeout, Limits::DEFAULT_TOOL_CALL_TIMEOUT);
    assert_eq!(sales.startup_timeout, igor_config::DEFAULT_STARTUP_TIMEOUT);
    assert_eq!(sales.cwd, PathBuf::from("/cfg"));
    assert_eq!(sales.command, PathBuf::from("/cfg/./server"));
    let claude = config.provider("claude").ok_or("claude missing")?;
    assert_eq!(claude.base_url, "https://api.anthropic.com");
    assert_eq!(
        claude.api_key.as_ref().and_then(|k| k.env_var()),
        Some("ANTHROPIC_API_KEY")
    );
    Ok(())
}

#[test]
fn explicit_values_override_defaults() -> TestResult {
    let source = format!(
        "{BASE}\n[limits]\nmax_iterations = 5\nmax_result_bytes = 2048\ntool_call_timeout_ms = 1500\n"
    );
    let source = source.replacen(
        "command = \"./server\"",
        "command = \"./server\"\ncall_timeout_ms = 900\nstartup_timeout_ms = 200\ncwd = \"run\"\nenv_allowlist = [\"PATH\"]\nenv = { SALES_TOKEN = { env = \"IGOR_SALES_TOKEN\" } }",
        1,
    );
    let config = parse(&source)?;
    let limits = config.limits();
    assert_eq!(limits.max_iterations, 5);
    assert_eq!(limits.max_result_bytes, 2048);
    assert_eq!(limits.tool_call_timeout, Duration::from_millis(1500));
    let sales = config
        .server(&ServerId::new("sales")?)
        .ok_or("sales missing")?;
    assert_eq!(sales.call_timeout, Duration::from_millis(900));
    assert_eq!(sales.startup_timeout, Duration::from_millis(200));
    assert_eq!(sales.cwd, PathBuf::from("/cfg/run"));
    assert_eq!(sales.env_allowlist, ["PATH"]);
    assert_eq!(
        sales.env.get("SALES_TOKEN").and_then(|r| r.env_var()),
        Some("IGOR_SALES_TOKEN")
    );
    let inventory = config
        .server(&ServerId::new("inventory")?)
        .ok_or("inventory missing")?;
    assert_eq!(inventory.call_timeout, Duration::from_millis(1500));
    Ok(())
}

#[test]
fn permission_is_scoped_to_its_server() -> TestResult {
    let config = parse(&format!("{BASE}\n[permissions.sales]\nx = \"write\"\n"))?;
    let permissions = config.permissions();
    assert_eq!(
        permissions.classify(&tool("sales", "x")?),
        Classification::Write
    );
    assert_eq!(
        permissions.classify(&tool("inventory", "x")?),
        Classification::Unclassified
    );
    assert_eq!(
        permissions.classify(&tool("sales", "y")?),
        Classification::Unclassified
    );
    Ok(())
}

/// An invalid-value case: name, `(from, to)` replacement applied to [`BASE`], text appended after
/// it, expected field path and expected problem.
type InvalidCase = (
    &'static str,
    (&'static str, &'static str),
    &'static str,
    &'static str,
    Problem,
);

/// Each case expects a validation error naming the field and the problem.
#[test]
fn invalid_values_name_field_and_problem() -> TestResult {
    let provider_key_by_reference = "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv = { KEY = { env = \"ANTHROPIC_API_KEY\" } }\n";
    let cases: Vec<InvalidCase> = vec![
        (
            "duplicate server id",
            ("id = \"inventory\"", "id = \"sales\""),
            "",
            "servers[1].id",
            Problem::DuplicateServerId(ServerId::new("sales")?),
        ),
        (
            "invalid server id",
            ("id = \"inventory\"", "id = \"Inventory\""),
            "",
            "servers[1].id",
            Problem::InvalidServerId(InvalidServerId("Inventory".into())),
        ),
        (
            "unknown provider kind",
            ("kind = \"anthropic\"", "kind = \"anthorpic\""),
            "",
            "providers[0].kind",
            Problem::UnknownProviderKind("anthorpic".into()),
        ),
        (
            "invalid provider id",
            ("id = \"claude\"", "id = \"Claude\""),
            "",
            "providers[0].id",
            Problem::InvalidProviderId("Claude".into()),
        ),
        (
            "duplicate provider id",
            ("", ""),
            "[[providers]]\nid = \"claude\"\nkind = \"openai_compatible\"\nmodel = \"m\"\nbase_url = \"http://localhost:1234/v1\"\n",
            "providers[1].id",
            Problem::DuplicateProviderId("claude".into()),
        ),
        (
            "anthropic without api key",
            ("api_key = { env = \"ANTHROPIC_API_KEY\" }", ""),
            "",
            "providers[0].api_key",
            Problem::RequiredForKind("anthropic"),
        ),
        (
            "openai compatible without base url",
            ("kind = \"anthropic\"", "kind = \"openai_compatible\""),
            "",
            "providers[0].base_url",
            Problem::RequiredForKind("openai_compatible"),
        ),
        (
            "empty model",
            ("model = \"m\"", "model = \" \""),
            "",
            "providers[0].model",
            Problem::Empty,
        ),
        (
            "base url with credentials",
            (
                "model = \"m\"",
                "model = \"m\"\nbase_url = \"https://user:pw@example.com\"",
            ),
            "",
            "providers[0].base_url",
            Problem::InvalidUrl("must not embed credentials; use api_key with an env reference"),
        ),
        (
            "plain http to a remote host",
            (
                "model = \"m\"",
                "model = \"m\"\nbase_url = \"http://api.example.com/v1\"",
            ),
            "",
            "providers[0].base_url",
            Problem::InvalidUrl("plain http:// is only allowed for localhost; use https://"),
        ),
        (
            "base url without scheme",
            ("model = \"m\"", "model = \"m\"\nbase_url = \"example.com\""),
            "",
            "providers[0].base_url",
            Problem::InvalidUrl("must start with http:// or https://"),
        ),
        (
            "unsupported transport",
            (
                "transport = \"stdio\"\ncommand = \"./server\"\n\n[[servers]]\nid = \"inventory\"",
                "transport = \"http\"\ncommand = \"./server\"\n\n[[servers]]\nid = \"inventory\"",
            ),
            "",
            "servers[0].transport",
            Problem::UnsupportedTransport("http".into()),
        ),
        (
            "empty command",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"\"\n",
            "servers[2].command",
            Problem::Empty,
        ),
        (
            "invalid allowlist name",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv_allowlist = [\"path\"]\n",
            "servers[2].env_allowlist[0]",
            Problem::InvalidEnvName("path".into()),
        ),
        (
            "duplicate allowlist name",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv_allowlist = [\"PATH\", \"PATH\"]\n",
            "servers[2].env_allowlist[1]",
            Problem::DuplicateEnvName("PATH".into()),
        ),
        (
            "allowlist and env both set",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv_allowlist = [\"TOKEN\"]\nenv = { TOKEN = { env = \"IGOR_TOKEN\" } }\n",
            "servers[2].env_allowlist[0]",
            Problem::EnvNameConflict("TOKEN".into()),
        ),
        (
            "allowlisted provider key",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv_allowlist = [\"ANTHROPIC_API_KEY\"]\n",
            "servers[2].env_allowlist[0]",
            Problem::ExposesProviderKey("ANTHROPIC_API_KEY".into()),
        ),
        (
            "provider key passed by reference",
            ("", ""),
            provider_key_by_reference,
            "servers[2].env.KEY",
            Problem::ExposesProviderKey("ANTHROPIC_API_KEY".into()),
        ),
        (
            "invalid server env name",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv = { key = { env = \"IGOR_KEY\" } }\n",
            "servers[2].env.key",
            Problem::InvalidEnvName("key".into()),
        ),
        (
            "literal provider secret",
            (
                "api_key = { env = \"ANTHROPIC_API_KEY\" }",
                "api_key = \"sk-ant-literal\"",
            ),
            "",
            "providers[0].api_key",
            Problem::LiteralSecret,
        ),
        (
            "literal server env secret",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv = { TOKEN = \"abc\" }\n",
            "servers[2].env.TOKEN",
            Problem::LiteralSecret,
        ),
        (
            "secret reference that is not a variable name",
            (
                "{ env = \"ANTHROPIC_API_KEY\" }",
                "{ env = \"sk-ant-literal\" }",
            ),
            "",
            "providers[0].api_key",
            Problem::InvalidSecretReference,
        ),
        (
            "permission for undeclared server",
            ("", ""),
            "[permissions.pos]\nlookup = \"read\"\n",
            "permissions.pos",
            Problem::UnknownServer("pos".into()),
        ),
        (
            "unknown permission class",
            ("", ""),
            "[permissions.sales]\nlookup = \"admin\"\n",
            "permissions.sales.lookup",
            Problem::UnknownPermissionClass("admin".into()),
        ),
        (
            "unclassified is not a configurable class",
            ("", ""),
            "[permissions.sales]\nlookup = \"unclassified\"\n",
            "permissions.sales.lookup",
            Problem::UnknownPermissionClass("unclassified".into()),
        ),
        (
            "invalid tool name",
            ("", ""),
            "[permissions.sales]\n\"bad name\" = \"read\"\n",
            "permissions.sales.bad name",
            Problem::InvalidToolName("bad name".into()),
        ),
        (
            "iteration limit out of range",
            ("", ""),
            "[limits]\nmax_iterations = 0\n",
            "limits.max_iterations",
            Problem::OutOfRange {
                value: 0,
                min: 1,
                max: 100,
            },
        ),
        (
            "server timeout out of range",
            ("", ""),
            "[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nstartup_timeout_ms = 1\n",
            "servers[2].startup_timeout_ms",
            Problem::OutOfRange {
                value: 1,
                min: 100,
                max: 120_000,
            },
        ),
    ];

    for (name, (from, to), extra, field, problem) in cases {
        let source = format!("{}\n{extra}", BASE.replacen(from, to, 1));
        match parse(&source) {
            Err(err) => {
                assert_eq!(err.field(), Some(field), "{name}: {err}");
                assert_eq!(err.problem(), Some(&problem), "{name}: {err}");
                let message = err.to_string();
                assert!(message.contains(field), "{name}: {message}");
                assert!(
                    message.starts_with("<inline configuration>: "),
                    "{name}: {message}"
                );
            }
            Ok(_) => return Err(format!("{name}: expected an error").into()),
        }
    }
    Ok(())
}

/// Schema violations caught by the parser report line, column and the problem.
#[test]
fn schema_violations_report_location() -> TestResult {
    let cases = [
        (
            "missing field",
            BASE.replacen("command = \"./server\"\n", "", 1),
            "missing field `command`",
        ),
        (
            "unknown key",
            format!("{BASE}\n[limits]\nmax_turns = 3\n"),
            "unknown field `max_turns`",
        ),
        (
            "unknown top-level table",
            format!("{BASE}\n[provider]\nid = \"x\"\n"),
            "unknown field `provider`",
        ),
        (
            "unsupported secret source",
            BASE.replacen("{ env = ", "{ file = ", 1),
            "unsupported secret source `file`",
        ),
        ("invalid TOML", format!("{BASE}\n[[servers]\n"), ""),
    ];
    for (name, source, expected) in cases {
        match parse(&source) {
            Err(ConfigError::Parse {
                line,
                column,
                message,
                ..
            }) => {
                assert!(line > 1 && column >= 1, "{name}: {line}:{column}");
                assert!(message.contains(expected), "{name}: {message}");
            }
            other => return Err(format!("{name}: expected a parse error, got {other:?}").into()),
        }
    }
    Ok(())
}

#[test]
fn file_errors_name_the_file() -> TestResult {
    let dir = std::env::temp_dir().join(format!("igor-config-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("igor.toml");
    std::fs::write(&path, format!("{BASE}\n[permissions.pos]\nx = \"read\"\n"))?;
    let invalid = Config::load(&path).err().ok_or("expected an error")?;
    std::fs::remove_dir_all(&dir)?;
    assert!(
        invalid
            .to_string()
            .starts_with(&format!("{}: permissions.pos: ", path.display())),
        "{invalid}"
    );

    let missing = Config::load(&dir.join("absent.toml"))
        .err()
        .ok_or("expected an error")?;
    assert!(matches!(missing, ConfigError::Read { .. }));
    assert!(missing.to_string().contains("absent.toml"));
    Ok(())
}

/// A sentinel secret written literally in any secret position never appears in an error, and a
/// resolved sentinel never appears in debug output of the configuration.
#[test]
fn sentinel_secret_never_leaks() -> TestResult {
    let literal_sources = [
        BASE.replacen(
            "{ env = \"ANTHROPIC_API_KEY\" }",
            &format!("\"{SENTINEL}\""),
            1,
        ),
        BASE.replacen("ANTHROPIC_API_KEY\" }", &format!("{SENTINEL}\" }}"), 1),
        format!(
            "{BASE}\n[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv = {{ T = \"{SENTINEL}\" }}\n"
        ),
        format!(
            "{BASE}\n[[servers]]\nid = \"third\"\ntransport = \"stdio\"\ncommand = \"x\"\nenv = {{ T = {{ env = \"{SENTINEL}\" }} }}\n"
        ),
    ];
    for source in literal_sources {
        let err = parse(&source).err().ok_or("expected an error")?;
        for output in [err.to_string(), format!("{err:?}"), format!("{err:#?}")] {
            assert!(!output.contains(SENTINEL), "leaked: {output}");
        }
    }

    let config = parse(BASE)?;
    let key = config
        .provider("claude")
        .and_then(|p| p.api_key.as_ref())
        .ok_or("api key missing")?;
    let secret = key.resolve_with(|_| Some(SENTINEL.into()))?;
    assert_eq!(secret.expose(), SENTINEL);
    for output in [
        format!("{config:?}"),
        format!("{secret:?}"),
        secret.to_string(),
        serde_json::to_string(&secret)?,
    ] {
        assert!(!output.contains(SENTINEL), "leaked: {output}");
    }
    Ok(())
}
