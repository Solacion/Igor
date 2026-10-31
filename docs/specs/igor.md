# IGOR product specification

Status: draft for review · 30 October 2026

## Purpose

Build a standalone, embeddable, local-first AI agent runtime that coordinates model providers and tools exposed by multiple MCP servers. A desktop application is one consumer of the runtime; domain integrations remain independent.

The near-term goal is a small working demo for Sanderson Design Group's second-stage Software Engineer – AI & Automation interview. It should demonstrate engineering judgement, tests, review and reproducibility. Claude Code is the intended implementation assistant once planning is approved.

## Intended users

- An operator asking questions and reviewing proposed actions through the desktop application.
- A developer embedding the runtime or connecting additional providers and MCP servers.
- An interview reviewer reproducing the demo and inspecting its design and validation evidence.

## Scope

The runtime must support a model-provider abstraction, simultaneous connections to multiple MCP servers, tool execution, and explicit permission and approval handling. Configuration and application-owned state should remain local by default. Remote providers and MCP servers may receive data when configured; local-first does not imply offline inference or that data never leaves the machine.

Agreed components are a Rust core and a Tauri/Vue desktop UI; an optional HTTP API is deferred. Voice is a later capability. Live domain integrations, such as a retail POS MCP server, are independent projects and not part of the core runtime.

## First demo

The demo shows the generic runtime: a selectable model provider, tools from at least two independent MCP servers in one session, server-qualified tool routing, and host-enforced approvals. It runs on macOS and supports Anthropic Claude and a local LM Studio model as providers.

### Example scenario

One illustrative scenario exercises the runtime. It is an example of how IGOR could be embedded in an application, not a product requirement, and the runtime contains no knowledge of it.

An operator asks, for example, "Create an order for supplier X". IGOR reads sales and stock data from one MCP server and the supplier's catalogue and ordering terms from another, reasons about likely demand, fits the order to the supplier's pack sizes, order multiples and minimum order value, explains its recommendation, and prepares a draft order preview for review.

Both example servers are fixture-backed MCP servers with realistic sample data; the sales and stock server stands in for a point-of-sale server. No live retail service is required.

### Functional requirements

1. Load configured model-provider and MCP-server connections without embedding credentials in source code.
2. Connect to at least two independent MCP servers in one session and discover their tools.
3. Identify tools by server and tool name so identical names cannot cause ambiguous execution.
4. Accept a user request, call tools across servers as needed, and produce an answer supported by returned data.
5. Explain unavailable data and assumptions; never present invented values as retrieved facts.
6. Display tool activity, results and failures sufficiently clearly for the operator to understand the answer.
7. Present proposed write actions as inspectable previews before any approval.
8. Require explicit approval before any write-classified tool invocation. Approval must identify the destination and exact proposed action and arguments. Changed arguments require fresh approval.
9. Refuse denied or unclassified write actions. Model output and tool content cannot grant permissions or approve actions.
10. Handle connection failures, model errors, invalid tool arguments and missing data visibly, without claiming success or silently repeating writes.
11. Ask the operator for input when required information is missing or uncertain, such as dates, periods, suppliers or quantities, instead of assuming it.

### Boundaries

IGOR may call write-classified tools on MCP servers that support them, only after explicit approval. In the example scenario, the fixture tool `create_draft_order` creates a draft only; the demo submits no orders, makes no payments and changes no external system. Approval rules are verified with deterministic fixtures and shown in the demo.

Production deployment, broad provider compatibility, voice, background scheduling, multi-user access and a full HTTP API are outside the first milestone unless explicitly brought into scope.

## Quality requirements

- Keep the core independent of the desktop UI and of any domain logic, including the example scenario.
- Keep secrets out of repository files and diagnostic output; document the selected local credential mechanism during implementation.
- Treat retrieved text and tool responses as untrusted data. Permissions are enforced by the host runtime.
- Give tool operations bounded timeouts and cancellation. Specify safe retry behaviour per operation; never automatically retry a write whose outcome is unknown.
- Provide reproducible fixtures and a documented verification path that requires no live external services.
- Serve fixtures through fixture-backed MCP servers, not through test doubles inside the core. Each fixture server implements a defined tool contract (tool names, input schemas and `tools/call` result shapes), so a live server implementing the same contract can replace it through configuration alone.
- Keep dependencies, configuration examples and startup instructions reproducible. The demo targets macOS and is due on Tuesday 3 November 2026; exact commands will be established with the implementation.

## Demo acceptance criteria

- A reviewer can follow the eventual setup guide from a clean checkout using documented prerequisites and configuration.
- A single request uses tools from at least two MCP servers and yields an answer grounded in their results; the example scenario demonstrates this.
- The operator can inspect a proposed action's preview before approving or denying it.
- A write cannot occur without the required approval; denial and altered arguments are tested.
- A disconnected server or missing data produces an honest, useful explanation and no fabricated success.
- Automated tests exercise the core boundaries using deterministic fixtures; a documented smoke test covers the desktop workflow.
- Review evidence records checks run, known limitations and changes made after review.

## Work tracking

Agreed work is tracked in [GitHub Issues](https://github.com/Solacion/Igor/issues). Each pull request into `main` must pass the gated agent code and security reviews. See [decisions](decisions.md) for accepted choices and the cut order.
