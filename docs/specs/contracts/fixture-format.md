# Fixture file format

Status: implemented by `igor-fixture-server` · issue #49

A fixture server is a real MCP server. It answers `initialize`, `tools/list` and `tools/call` over stdio from one fixture file. It has no clock, randomness or network access in its results: the same fixture file and the same calls always produce the same responses.

## Running

```sh
cargo run -p igor-fixture-server -- --fixture crates/igor-fixture-server/fixtures/sales.json --call-log /tmp/sales-calls.jsonl
```

| Flag | Environment variable | Meaning |
| --- | --- | --- |
| `--fixture <path>` | `IGOR_FIXTURE_FILE` | Fixture file to serve. Required. |
| `--call-log <path>` | `IGOR_FIXTURE_CALL_LOG` | JSON Lines call log, created or truncated at start. Optional. |

Flags override environment variables. Stdout carries only MCP messages. An invalid fixture file stops the server at start with a message on stderr and exit status 1.

## File

```json
{
  "server": { "name": "igor-fixture-sales", "version": "0.1.0" },
  "tools": [ { "name": "get_stock_levels", "inputSchema": { "type": "object" } } ],
  "calls": [
    {
      "tool": "get_stock_levels",
      "arguments": { "supplier_id": "supplier-x", "date": "2026-07-13" },
      "result": { "content": [ { "type": "text", "text": "…" } ], "structuredContent": { }, "isError": false }
    },
    {
      "tool": "get_stock_levels",
      "arguments": "*",
      "result": { "content": [ { "type": "text", "text": "{\"error\":{\"code\":\"no_data\",\"message\":\"…\"}}" } ], "isError": true }
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `server` | Name and version reported in the `initialize` result. |
| `tools` | Tool definitions returned verbatim by `tools/list`, in MCP shape. |
| `calls[].tool` | Tool name. Must be listed in `tools`. |
| `calls[].arguments` | The exact arguments object, or the string `"*"` for the tool's fallback entry. |
| `calls[].result` | The `tools/call` result returned verbatim, in MCP shape. |
| `calls[].fault` | Optional fault, see below. |

Unknown fields are rejected, so a typo cannot silently change behaviour. The server also refuses to start when a call names an unlisted tool, two entries for a tool have the same canonical arguments, a tool has more than one `"*"` entry, or an entry does not have exactly one of `result`, `fault.tool_error`, `fault.oversized_result`, `fault.hang`, `fault.crash_before_response`, `fault.crash_after_record` and `fault.malformed_response`.

## Matching

1. The call's `arguments` (an absent object counts as `{}`) are converted to canonical JSON: object keys sorted, no whitespace, numbers compared by value (`12`, `12.0` and `1.2e1` are equal), as in RFC 8785.
2. An entry whose canonical `arguments` are equal is used. Schema defaults are not filled in, so extra or missing keys do not match.
3. Otherwise the tool's `"*"` entry is used, if it has one. Use it only for documented catch-all answers such as a `no_data` error.
4. Otherwise the call is **unrecorded**. The server returns an `isError: true` result whose text names the tool and canonical arguments. It never invents data.
5. A tool name that is not in `tools` gets a JSON-RPC error (`-32602`, invalid params), as a live server would return.

## Faults

| Fault | Effect | Status |
| --- | --- | --- |
| `"delay_ms": <n>` | Waits `n` milliseconds before returning the result. Combines with `result` or `tool_error`. If the client cancels the request during the wait, the server stops waiting and returns an error. | Implemented |
| `"tool_error": "<message>"` | Returns `isError: true` with `message` as text, in place of `result`. | Implemented |
| `"oversized_result": <bytes>` | Returns a successful result with one text block of `bytes` characters, in place of `result`. At most 64 MiB. | Implemented |
| `"hang": true` | Logs the call, then never responds. | Implemented |
| `"crash_before_response": true` | Exits with status 3 before the call is logged or answered. | Implemented |
| `"crash_after_record": true` | Logs the call, then exits with status 3 without responding. | Implemented |
| `"malformed_response": true` | Logs the call, writes a line that is not JSON-RPC to stdout, then never responds. | Implemented |

`delay_ms` combines with any other fault except `crash_before_response`. The protocol-level faults replace `result`.

Example: `"fault": { "delay_ms": 200, "tool_error": "upstream timeout" }`.

## Call log

When a call log is configured, every `tools/call` is appended as one JSON line, written and flushed before the response is sent:

```json
{"seq":1,"tool":"get_stock_levels","arguments":{"date":"2026-07-13","supplier_id":"supplier-x"},"match":"exact","matched":true}
```

| Field | Meaning |
| --- | --- |
| `seq` | 1-based order in which calls were received. |
| `tool` | Requested tool name. |
| `arguments` | Canonical arguments. |
| `match` | `exact`, `fallback`, `unrecorded` or `unknown_tool`. |
| `matched` | `true` for `exact` and `fallback`. |

Tests use the log as the server-side record of what was invoked, for example "no call after denial" or "exactly one call for an unknown write outcome". Tests should fail when it contains an unmatched call, so a missing fixture is caught instead of silently degrading an answer.

## Fixture sets

| File | Contract | Covers |
| --- | --- | --- |
| `crates/igor-fixture-server/fixtures/sales.json` | [Sales and stock](sales.md) | `supplier-x`: sales 2026-07-06 to 2026-07-12, stock on 2026-07-13; `supplier-z`: stock on 2026-07-13 with a prompt-injection attempt in a product `name`; `unknown_supplier` for `supplier-unknown`; `no_data` for anything else; any `create_draft_order` returns `draft-0001` |
| `crates/igor-fixture-server/fixtures/supplier.json` | [Supplier catalogue](supplier.md) | `supplier-x` and `supplier-z` catalogues; `unknown_supplier` for anything else |

Arguments match exactly, so a period not listed gets the missing-data error even inside the covered range.

## Contract check

`cargo test -p igor-fixture-server --test contracts` checks every file in `crates/igor-fixture-server/fixtures/` against the contract of the same name in this directory. It runs in CI with the other tests. It fails when:

- a tool definition differs from the contract, or a tool is missing or extra;
- recorded arguments do not satisfy the tool's `inputSchema`;
- a success result does not satisfy the `outputSchema`, or its text block is not the same JSON as `structuredContent`;
- an error result has `structuredContent`, or an error code that is not in the tool's error table.
