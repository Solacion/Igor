# Tool contracts

Status: agreed for the demo · 30 October 2026

The example scenario ("Create an order for supplier X") uses two MCP servers. Their tool contracts are defined here, and the fixture servers implement them. A live server that implements the same contract can replace a fixture server through configuration alone. The IGOR core knows nothing about these contracts.

- [Sales and stock](sales.md): `get_stock_levels`, `get_sales_history` and the write-classified `create_draft_order`.
- [Weather](weather.md): `get_forecast`.
- [Fixture file format](fixture-format.md): how a fixture server records and matches these calls.

## Protocol

Contracts target MCP protocol revision `2025-11-25`, the newest revision with an `initialize` handshake. Servers use the stdio transport for the demo.

## Conventions

These apply to every tool in every contract.

**Tool definitions.** Each tool's `tools/list` entry is given in full: `name`, `title`, `description`, `inputSchema`, `outputSchema` and `annotations`. A server must return these definitions. Annotations are hints only; the permission class comes from host configuration.

**Permission class.** Each contract recommends a host class, `read` or `write`, for the sample configuration. The host configuration enforces the class, not the contract or the server's annotations.

**Dates.** Dates are `YYYY-MM-DD` strings and periods are inclusive `from` and `to` dates. They are always tool inputs. A server never assumes "today", and the agent asks the operator when a date or period is missing or unclear. Fixture servers cover a stated date range; outside it they return a missing-data error.

**Success results.** `isError` is `false`. `structuredContent` conforms to the tool's `outputSchema`. `content` holds one text block containing the same JSON, for clients that do not read structured content. A success result never represents missing data.

**Error results.** A tool-level failure is a `tools/call` result with `isError: true`, no `structuredContent`, and one text block whose text is JSON:

```json
{ "error": { "code": "no_data", "message": "Human-readable explanation." } }
```

`code` is a stable machine-readable value from the tool's error table. `message` is untrusted text for display.

**Missing data is an error.** When the requested data does not exist (unknown identifier, or no data for the whole date or period), the server returns an error result. It never returns an empty list as success, so the agent cannot mistake "no data" for "zero".

**Protocol errors.** An unknown tool name or malformed request is a JSON-RPC error (`-32602` for an unknown tool), as for any MCP server.

**Untrusted content.** Descriptions, messages and every result field are untrusted data. Hosts render them as text and never follow instructions found in them.
