# Configuration

IGOR reads one local TOML file. It lists the model providers, the MCP servers, the tool permissions and the execution limits. The `igor-config` crate loads and validates it.

## File location

IGOR uses the first of these that applies:

1. A path given on the command line.
2. The `IGOR_CONFIG` environment variable.
3. `~/Library/Application Support/igor/config.toml`.

Relative paths in the file resolve against the file's directory, made absolute when the file is loaded.

## Sample

[`examples/igor.toml`](https://github.com/Solacion/Igor/blob/main/examples/igor.toml) configures the example scenario. It has two fixture servers, `sales` and `supplier`, and needs no credentials to load. A test loads it in CI.

```toml
[[providers]]
id = "claude"
kind = "anthropic"            # or "openai_compatible" (LM Studio)
model = "claude-haiku-5-5"
api_key = { env = "ANTHROPIC_API_KEY" }

[[servers]]
id = "sales"                  # [a-z][a-z0-9_-]*, at most 32 characters, unique
transport = "stdio"           # "stdio" (local process) or "http" (remote server)
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
| `api_key` | `anthropic` only | A secret reference: `{ env = "…" }` or `{ keychain = "…" }`. |

## Secrets

Secrets are never written in the file. A secret field holds a reference to where the value is kept:

- `{ env = "ANTHROPIC_API_KEY" }`: an environment variable. IGOR reads it when it uses the secret. If it is unset or empty, IGOR reports an error that names the variable.
- `{ keychain = "claude" }`: an item in your macOS login Keychain, under the service `com.solacion.igor`. The desktop stores a key you paste in **Settings → Model providers** this way. If the item is missing, IGOR reports an error that names it. MCP server secrets (`env`, `headers` and `bearer_token`) can use the Keychain too.

IGOR rejects a literal value in a secret field, and the error does not repeat that value. Resolved secrets are redacted in debug output, display output and serialised output. To remove a stored key outside IGOR, open Keychain Access and delete the `com.solacion.igor` item, or run `security delete-generic-password -s com.solacion.igor -a claude`.

## MCP servers

Each `[[servers]]` entry is a local process (`transport = "stdio"`) or a remote server over Streamable HTTP (`transport = "http"`). Tool discovery, permissions, approvals, connection status, reconnect, timeouts and cancellation work the same for both.

| Key | Default | Meaning |
| --- | --- | --- |
| `id` | (required) | A unique server id. Tools are identified by server id and tool name. |
| `transport` | `stdio` | `stdio`, `http` (Streamable HTTP) or `sse` (the legacy HTTP+SSE transport of MCP 2024-11-05). |
| `command` | (required for `stdio`) | The program to run. A relative path with a directory part, such as `./bin/server`, resolves against the configuration directory. A bare name, such as `node`, is looked up on `PATH`. |
| `args` | `[]` | Program arguments. |
| `cwd` | configuration directory | The working directory. |
| `env_allowlist` | `[]` | IGOR environment variables that the server may inherit. The server inherits nothing else. A bare command needs `PATH` here. |
| `env` | `{}` | Variables to set in the server, each as a secret reference: `env = { TOKEN = { env = "IGOR_SALES_TOKEN" } }`. |
| `startup_timeout_ms` | `10000` | The time limit for connecting, `initialize` and `tools/list`, from 100 to 120000. |
| `call_timeout_ms` | `limits.tool_call_timeout_ms` | The time limit for each tool call to this server. |

`command`, `args`, `cwd`, `env_allowlist` and `env` apply to `stdio` only.

### Remote servers

```toml
[[servers]]
id = "crm"
transport = "http"
url = "https://mcp.example.com/mcp"
bearer_token = { keychain = "crm-mcp" }
headers = { "X-Api-Key" = { env = "IGOR_CRM_KEY" } }
```

| Key | Default | Meaning |
| --- | --- | --- |
| `url` | (required for `http`) | The server's MCP endpoint. It must use `https://`; plain `http://` is allowed only for `localhost`, `127.0.0.1` and `[::1]`. It must not contain a user name or password. |
| `bearer_token` | none | A secret reference, sent as `Authorization: Bearer <token>`. |
| `auth` | none | `"oauth"` for a server that needs you to sign in with OAuth 2.1. Not allowed together with `bearer_token` or an `Authorization` header. |
| `headers` | `{}` | Headers sent with every request, each value a secret reference. Names are HTTP tokens and are case-insensitive. `Host`, `Content-Type`, `Content-Length`, `Accept`, `Mcp-Session-Id`, `Mcp-Protocol-Version` and `Last-Event-Id` are set by IGOR. `Authorization` is not allowed together with `bearer_token`. |

Header and token values are read when the server connects. They never appear in the configuration file, logs, events or exported activity, and IGOR removes them from connection errors. IGOR does not follow redirects, so headers are only ever sent to the configured URL. To store a token in the Keychain, run `security add-generic-password -U -s com.solacion.igor -a crm-mcp -w` and type the token when asked.

You can also add and edit remote servers in **Settings → MCP servers**: a key typed there is stored in the Keychain item `mcp-<id>` and referenced as `{ keychain = "mcp-<id>" }`. For a server that only speaks the older HTTP+SSE transport, use `transport = "sse"` and set `url` to its event stream, often ending in `/sse`. IGOR posts messages to the endpoint the server names, which must be on the same host. `headers` and `bearer_token` work as for `http`; `auth = "oauth"` does not.

#### OAuth sign-in

With `auth = "oauth"`, IGOR finds the server's authorization server, registers itself as a client when the server allows it, and opens the sign-in page in your browser with PKCE. The browser comes back to a one-off address on this computer (`http://127.0.0.1:<port>/callback`). IGOR keeps the tokens in the Keychain item `mcp-<id>-oauth` and refreshes them when they expire. Until you sign in, the server shows as needing sign-in and its tools are not offered.

Sign in from **Settings → MCP servers → Sign in**, or from a terminal with `igor-cli --sign-in <id>`. **Sign out**, or removing the server, deletes the tokens. Only an `https` sign-in page (or one on this computer) is opened.

A server can never receive a provider API key. IGOR rejects an allowlist entry or an `env` reference that names a variable used by a provider's `api_key`.

## Tool permissions

Host configuration is the only source of tool classification. Under `[permissions.<server id>]`, map each exact tool name to one of these classes:

- `read`: the tool runs without approval, unless its server says it may change data (see below).
- `allow`: the tool always runs without approval, even when its server says it may change data. Settings writes this when you choose **Always allow** for such a tool. Each such call shows in the activity as "Ran without asking — Always allow".
- `write`: the tool runs only after you approve the exact call.
- `denied`: the tool never runs.

A server can make a `read` tool stricter: a `read` tool whose server reports that it may modify data, or that it is not read-only, needs approval too. For such a tool you can choose to approve it for the rest of the session, and later calls to it then run without asking. A `write` tool always needs approval for each call.

A tool that is not listed is unclassified. IGOR hides it from the model and refuses any call to it. A permission applies only to its own server: `sales/x` does not classify `inventory/x`. A permission for an undeclared server is an error.

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
