# Architecture proposal

Status: stack agreed (Rust core, Tauri/Vue UI); no implementation exists.

## Components

| Component | Responsibility | Proposed technology |
| --- | --- | --- |
| Core runtime | Session orchestration, provider interface, tool routing, permissions and approvals | Rust |
| Desktop application | User conversation, connection status, tool activity and approval UI | Tauri + Vue |
| Model adapters | Translate provider requests, responses and tool calls into core types | Rust adapters; Anthropic Claude and LM Studio first |
| MCP client | Connect to multiple servers, discover tools and route validated calls | [`rmcp`](https://crates.io/crates/rmcp); stdio transport for the demo |
| `igor-cli` | Headless harness over the same session API and events; fallback demo | Rust binary |
| Optional API | Expose the same runtime to other clients | HTTP; deferred from initial demo |
| Example MCP servers | Serve sample sales/stock and weather data for the example scenario | Fixture-backed MCP servers; not part of the runtime |

## Dependency boundaries

The desktop and optional API depend on the core. The core must not depend on the desktop UI, example-scenario schemas or a specific model provider. Domain-specific interpretation belongs in integration contracts and configuration rather than in the runtime's generic permission policy.

Use one orchestration and permission path across clients. An API or embedded caller must not bypass approval enforcement.

## Proposed execution flow

1. Load local configuration and establish configured connections.
2. Discover tools and assign server-qualified identities and host-controlled permission classifications.
3. Send the user request and permitted tool definitions to the selected model provider.
4. Validate proposed tool arguments and check runtime policy before dispatch.
5. For an action requiring approval, suspend dispatch and expose the exact action for user review. Denial returns a controlled result without executing the action.
6. Route permitted calls to the correct server and return bounded results to the model.
7. Present the final answer and any action previews, retaining useful activity information with sensitive fields redacted.

The implementation must define iteration limits, cancellation and result-size limits to bound agent execution.

## Fixture MCP servers

Fixture data reaches the core only through the MCP protocol. A fixture server is a real MCP server: it answers `initialize`, `tools/list` and `tools/call`, and returns results in the shape the tool contract defines, including error results. Fixtures are stored as `tools/list` definitions and `tools/call` results keyed by tool name and arguments.

The example scenario's tool contracts are defined in this repository, and its fixture servers implement them. A live server, such as one for the user's own POS, can replace a fixture server later by implementing the same contract.

The core cannot tell a fixture server from a live one. Integration tests and the default demo configuration point at fixture servers; a live demo changes only the connection configuration. A contract check validates each server's `tools/list` definitions and sample `tools/call` results against the contract. The same check applies to any live replacement.

## Settled for the demo

See [decisions](decisions.md): stdio transport with `rmcp`; environment-variable secret references; in-memory sessions with a redacted activity export; host-configured permissions with deny by default; clarification requests instead of assumptions.

## Contracts to settle during implementation

Tracked in the linked issues.

- Default models for Anthropic Claude and LM Studio; LM Studio must support native tool calling (#12, #13, #14).
- Server lifecycle and connection configuration (#16, #9).
- Example scenario tool contracts, including `create_draft_order` and date inputs (#25).
- Fixture file format, argument matching rules and behaviour for unrecorded calls (#27).
- Approval binding, lifetime and cancellation semantics (#19).

## Starting core and UI

Different implementation languages do not require separate manual startup steps. With the proposed Tauri design, the desktop application can embed the Rust core, and a documented development command can start the Vue frontend and desktop host together. Independently hosted MCP servers may require separate processes; the demo startup contract must explain how these are started and stopped. An independently hosted core is only needed if the HTTP deployment option is selected.
