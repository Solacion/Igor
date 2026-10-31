# Troubleshooting and cleanup

IGOR reports failures rather than hiding them. This page explains the common ones and how to remove what a demo run leaves behind. For build and verification problems, see [Prepare and verify a checkout](verification.md#troubleshooting-and-cleanup).

## The model provider is unavailable

- **LM Studio:** open LM Studio, load `qwen/qwen3-vl-8b` and start the local server (Developer tab) on port 1234. Check it answers with `curl http://127.0.0.1:1234/v1/models`. Until it does, each turn fails with a connection error. IGOR never substitutes a scripted reply.
- **"cannot call tools":** LM Studio does not list tool use for the configured model, so IGOR refused to send the request. Choose a model that LM Studio marks with tool use, such as `qwen/qwen3-vl-8b`.
- **Wrong model name:** the `model` in the provider entry must match the identifier LM Studio lists, exactly.
- **"the conversation needs about N tokens of context":** LM Studio loaded the model with a context window too small for the conversation and the tool definitions. LM Studio would cut the conversation, so the model would lose earlier tool results and repeat its calls; IGOR refuses the request instead. In LM Studio, eject the model and load it again with a larger **Context Length**, such as 32768, or run `lms load qwen/qwen3-vl-8b --context-length 32768`. Servers with many tools need more: the 32 tools of `chrome-devtools-mcp` alone take about 7,500 tokens.
- **Claude:** a turn that fails with an authentication error means the key was refused. The error shows `[REDACTED]` in place of the key.

## The API key is missing

The Anthropic provider reads its key from the variable named by `api_key`, `ANTHROPIC_API_KEY` in the sample. Set it in the terminal that starts IGOR:

```sh
export ANTHROPIC_API_KEY=...   # not saved anywhere by IGOR
pnpm run desktop:demo
```

If the variable is unset, choosing that provider fails with a message naming the variable, never a value. Do not put a key in the configuration file: validation accepts only `{ env = "NAME" }` references.

## An MCP server does not start

A server that fails to start, crashes or exceeds its startup timeout is shown as **Not connected** (desktop) or reported as unavailable (CLI). The rest of the session continues without it, and the model is told which servers are unavailable.

- Read the server's own output. IGOR keeps the last 50 lines it wrote to stderr, each cut to 500 characters, with the values of its secrets replaced by `[REDACTED]`. `igor-cli` prints them under the unavailable message. Redaction matches each secret's exact text, so a server that prints a secret encoded or split is not caught.
- Check `command` is an absolute path, or a relative path with a directory part such as `./bin/server`, which resolves against the configuration file's directory.
- For the fixture servers, build them first: `cargo build -p igor-fixture-server`. `pnpm run desktop:demo` does this for you.
- A server inherits only the variables in `env_allowlist`. A server that needs `PATH` or `HOME` must list it.
- In the desktop, fix the entry under **Settings → MCP servers**, then choose **Reconnect**.

## A tool is never used

New tools are blocked until classified. In **Settings**, set the tool to **Use it freely (read)** or **Ask me each time (write)**, or add it under `[permissions.<server>]` in the configuration file. Unclassified and denied tools are hidden from the model.

## A read tool still asks for approval

A tool you set to **Use it freely (read)** still needs approval when its server reports that it may modify data or is not read-only. The desktop and `igor-cli` list these tools when the server connects. IGOR only adds approval on such reports and never removes it, so a server's warning always wins. `chrome-devtools-mcp`, for example, reports this for `new_page`, `navigate_page` and the performance trace tools.

## An answer is marked incomplete

The desktop shows an incomplete answer, and `igor-cli` exits with `5`, when the model answered but some tool calls failed, were refused or returned an error. Check the tool activity for the failed calls before relying on the answer.

## Stop everything

- **Desktop:** close the window or choose **Quit Igor**. IGOR stops every MCP server it started, waiting up to 15 seconds. Then stop `pnpm run desktop:demo` with Ctrl-C if it is still running.
- **CLI:** at the `You:` prompt, enter an empty line or press Ctrl-D to end the session. Ctrl-C cancels only the turn in progress. Servers stop when `igor-cli` exits.
- A forced kill cannot stop child servers. Find leftovers with `pgrep -fl igor-fixture-server` and stop them with `pkill -f igor-fixture-server`.

## Remove local state

IGOR writes only these locations:

| What | Where | Remove with |
| --- | --- | --- |
| Configuration saved by Settings | `$IGOR_CONFIG`, else `~/Library/Application Support/igor/config.toml` | Delete the file to start empty. |
| Desktop activity exports | `~/Library/Application Support/com.solacion.igor/` | Delete the exported files. |
| CLI activity exports | The `--export` path you chose | Delete the file. |
| Fixture call logs | The `--call-log` path or `IGOR_FIXTURE_CALL_LOG`, if set | Delete the file. |
| `desktop:demo` configuration copy | A temporary directory, removed when the launcher exits | `--prepare-only` prints it; delete it yourself. |
| Build output and dependencies | `target/`, `apps/desktop/src-tauri/target/`, `node_modules/` | `cargo clean`, then delete `node_modules/`. |

`pnpm run desktop:demo` never changes your usual configuration. Model providers keep their own logs; see the provider's documentation.
