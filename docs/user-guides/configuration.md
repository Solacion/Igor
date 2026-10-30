# Configuration

IGOR reads one local TOML file. It lists the model providers, the MCP servers, the tool permissions and the execution limits. The `igor-config` crate loads and validates it.

## File location

IGOR uses the first of these that applies:

1. A path given on the command line.
2. The `IGOR_CONFIG` environment variable.
3. `~/Library/Application Support/igor/config.toml`.

Relative paths in the file resolve against the file's directory, made absolute when the file is loaded.

## Sample

[`examples/igor.toml`](https://github.com/Solacion/Igor/blob/main/examples/igor.toml) configures the example scenario. It has two fixture servers, `sales` and `weather`, and needs no credentials to load. A test loads it in CI. The weather fixture file is added by [#50](https://github.com/Solacion/Igor/issues/50); until then the `weather` server exits at startup.

```toml
[[providers]]
id = "claude"
kind = "anthropic"            # or "openai_compatible" (LM Studio)
model = "claude-haiku-5-5"
api_key = { env = "ANTHROPIC_API_KEY" }

[[servers]]
id = "sales"                  # [a-z][a-z0-9_-]*, at most 32 characters, unique
transport = "stdio"           # only stdio is supported
command = "../target/debug/igor-fixture-server"
args = ["--fixture", "../crates/igor-fixture-server/fixtures/sales.json"]

[permissions.sales]
create_draft_order = "write"
get_sales_history = "read"

[limits]
max_iterations = 10
```

## Providers

Each `[[providers]]` entry has the following keys:

| Key | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Your name for the provider. It must be unique. |
| `kind` | yes | `anthropic` or `openai_compatible`. LM Studio uses `openai_compatible`. |
| `model` | yes | Model name sent to the provider. |
| `base_url` | `openai_compatible` only | An `https` URL without credentials. Plain `http` is accepted only for `localhost`, `127.0.0.1` or `[::1]`, so keys are never sent unencrypted over a network. For `anthropic`, it defaults to `https://api.anthropic.com`. |
| `api_key` | `anthropic` only | A secret reference. |

## Secrets

Secrets are never written in the file. A secret field holds a reference to an environment variable, for example `{ env = "ANTHROPIC_API_KEY" }`. IGOR reads the variable when it uses the secret. If the variable is unset or empty, IGOR reports an error that names the variable.

IGOR rejects a literal value in a secret field, and the error does not repeat that value. Resolved secrets are redacted in debug output, display output and serialised output. The macOS Keychain is not supported yet.

## MCP servers

Each `[[servers]]` entry starts one stdio child process.

| Key | Default | Meaning |
| --- | --- | --- |
| `id` | (required) | A unique server id. Tools are identified by server id and tool name. |
| `transport` | (required) | Must be `stdio`. |
| `command` | (required) | The program to run. A relative path with a directory part, such as `./bin/server`, resolves against the configuration directory. A bare name, such as `node`, is looked up on `PATH`. |
| `args` | `[]` | Program arguments. |
| `cwd` | configuration directory | The working directory. |
| `env_allowlist` | `[]` | IGOR environment variables that the server may inherit. The server inherits nothing else. A bare command needs `PATH` here. |
| `env` | `{}` | Variables to set in the server, each as a secret reference: `env = { TOKEN = { env = "IGOR_SALES_TOKEN" } }`. |
| `startup_timeout_ms` | `10000` | The time limit for `initialize` plus `tools/list`, from 100 to 120000. |
| `call_timeout_ms` | `limits.tool_call_timeout_ms` | The time limit for each tool call to this server. |

A server can never receive a provider API key. IGOR rejects an allowlist entry or an `env` reference that names a variable used by a provider's `api_key`.

## Tool permissions

Host configuration is the only source of tool classification. Under `[permissions.<server id>]`, map each exact tool name to one of these classes:

- `read`: the tool runs without approval.
- `write`: the tool runs only after you approve the exact call.
- `denied`: the tool never runs.

A tool that is not listed is unclassified. IGOR hides it from the model and refuses any call to it. A permission applies only to its own server: `sales/x` does not classify `weather/x`. A permission for an undeclared server is an error.

## Limits

| Key | Default | Range |
| --- | --- | --- |
| `max_iterations` | 10 | 1 to 100 model turns per request |
| `max_result_bytes` | 32768 | 1024 to 16777216 bytes per tool result |
| `tool_call_timeout_ms` | 30000 | 100 to 600000 |

## Limitations

- `env` accepts only secret references. A non-secret value must be exported in IGOR's environment and referenced, or passed as an argument.
- Resolved secrets are redacted in output but are not zeroed in memory when dropped.

## Errors

IGOR stops at the first problem it finds. Unknown keys are errors.

- A parse error gives the file, line and column, for example ``config.toml:12:1: missing field `command` ``. Quoted values in parse errors are replaced with `[value not shown]`, so a secret typed into the wrong field is not echoed.
- A validation error gives the file, the field path and the problem, for example `config.toml: servers[1].id: server id "sales" is declared more than once`.
