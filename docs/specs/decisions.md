# Decisions and open questions

## Agreed direction

- IGOR is a standalone, embeddable, local-first runtime in its own open-source repository.
- Support multiple MCP servers, a model-provider abstraction and permissions/approvals.
- Keep domain MCP servers, such as a future POS server, independent of IGOR.
- Demo the generic runtime for the interview; a purchasing scenario using sales and weather data is an illustrative example only.
- Keep specifications and user guides under `/docs`, with a root `README.md`.
- Plan manually; use GitHub Issues for agreed tasks after the user's go-ahead.
- Remain in planning until implementation is explicitly approved.

## Accepted decisions — 30 October 2026

| Decision | Accepted answer |
| --- | --- |
| Demo scope | Generic runtime capabilities. The sales-and-weather purchasing scenario is one example, not a requirement. |
| Example data | Realistic sample data served by fixture-backed MCP servers. No live retail or weather service is required. |
| Write boundary | No external writes. The example produces a local draft order preview only. |
| Model providers | Anthropic Claude (remote) and LM Studio (local), both behind the provider abstraction. |
| Core and desktop stack | Rust core; Tauri/Vue desktop UI |
| Licence | GNU GPL version 3 only (`GPL-3.0-only`); applied in the root `LICENSE`. Supersedes the earlier Apache 2.0 selection. |
| Demo platform | macOS |
| Fixture delivery | Fixtures are served by fixture-backed MCP servers that speak the MCP protocol and implement a defined tool contract, so a live server implementing the same contract can replace one through configuration alone. |
| Example contracts | Contract-first. The example scenario's tool contracts are defined in `docs/specs/`. A live server, such as one for the user's own POS once complete, can implement them later. |
| First milestone surface | Desktop UI; optional HTTP API deferred |

## Proposals awaiting confirmation

| Decision | Current proposal |
| --- | --- |
| Approval verification | Exercise approval rules with a fixture write tool in deterministic tests, since the demo has no external write |

## Open questions, in suggested order

1. Which MCP transports must the first demo support (stdio, streamable HTTP, or both)?
2. Which Claude model and which LM Studio model are the demo defaults, and what is the minimum hardware for the LM Studio path?
3. What is the interview demo deadline?

Record accepted answers here and update affected specifications before creating implementation issues. Proposals are not settled decisions.
