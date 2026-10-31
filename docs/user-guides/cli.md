# Command-line harness

`igor-cli` runs one request through the same session API and runtime events as the desktop. It connects the configured MCP servers, prints tool activity, and asks in the terminal for approvals and clarification answers. It is the fallback demo if the desktop is not ready. Related work: [#44](https://github.com/Solacion/Igor/issues/44).

## Run the offline demo

From the repository root, build the fixture server and the CLI, then run the scripted example:

```sh
cargo build -p igor-fixture-server -p igor-cli
./target/debug/igor-cli --config examples/igor.toml \
  --provider scripted --script examples/order-script.json \
  "Prepare a draft order for supplier-x for delivery on 2026-07-15."
```

The scripted provider replays model turns from [`examples/order-script.json`](../../examples/order-script.json), so the run needs no network or model. It reads stock and sales from the `sales` fixture server and ordering terms from the `supplier` fixture server, then proposes `create_draft_order`. Answer `y` to approve it. Any other answer, or end of input, denies it and the call is never sent.

A server that fails to start is reported as unavailable, and the run continues without it. Its last lines of error output follow, prefixed with `  | `. The values of secrets given to the server are shown as `[REDACTED]`. The model is told only that the server is unavailable, never its output.

## Options

```text
igor-cli [--config <path>] --provider <scripted|ID> [--script <path>] [--export <path>] [REQUEST...]
```

| Option | Meaning |
| --- | --- |
| `--config <path>` | Configuration file. Default: `$IGOR_CONFIG`, then `~/Library/Application Support/igor/config.toml`. See [Configuration](configuration.md). |
| `--provider <name>` | `scripted`, or a provider `id` from the configuration, such as `claude` or `lm_studio`. |
| `--script <path>` | Script of model turns. Required with `--provider scripted`, refused otherwise. |
| `--export <path>` | Also write every runtime event to this file as JSON Lines, redacted (see below). |
| `REQUEST` | The request. Read as one line from stdin when omitted. |

After each turn, `igor-cli` shows `You:` and reads a follow-up message, which continues the same conversation; use it to answer a question the model asked in its reply. An empty line or end of input ends the session. Press Ctrl-C to cancel the turn in progress; the conversation can continue afterwards.

Exit codes, from the last turn: `0` completed, `1` startup error (configuration, provider or arguments), `2` the turn failed, `3` a limit stopped the turn, `4` cancelled, `5` completed with gaps: the model answered, but some tool calls failed, were refused or returned an error such as missing data.

## Configured providers

With `--provider claude`, the key is read from the variable named in the configuration (`ANTHROPIC_API_KEY` in the sample). With `--provider lm_studio`, load the model in LM Studio and start its local server first. The LM Studio path was checked with `qwen/qwen3-vl-8b` on 31 October 2026; see the [live verification record](../verification/2026-10-31-live-lm-studio.md). The Claude path has not been checked against the live API yet.

## Script format

A script is a JSON object with a `turns` array, one entry per model request. Each turn has optional `text` and optional `tool_calls`, each `{ "id", "name", "input" }`. `name` is the alias the model sees, `<server>__<tool>`, such as `sales__get_stock_levels`. A turn may instead be `{ "error": "message" }`, which fails that model request. If the run does not use the script exactly, `igor-cli` prints a warning on stderr.

## Activity export

`--export` writes one JSON object per event: `schema_version`, `sequence` and the `event` as the desktop receives it. Business data and model text are redacted wherever they appear:

- `arguments`: keys are kept and each value becomes `"<redacted>"`.
- `canonical_arguments`, `text` and `question`: replaced by `"<redacted> (N bytes)"`.

Identifiers, tool names, outcomes, the action hash and diagnostic messages are kept. Diagnostic messages come from servers and providers and may still quote input; review an export before sharing it. The request text is not exported.
