# Decisions and open questions

## Agreed direction

- IGOR is a standalone, embeddable, local-first runtime in its own open-source repository.
- Support multiple MCP servers, a model-provider abstraction and permissions/approvals.
- Keep domain MCP servers, such as a future POS server, independent of IGOR.
- Demo the generic runtime for the interview; a purchasing scenario using sales and stock data is an illustrative example only.
- Keep specifications and user guides under `/docs`, with a root `README.md`.
- Track agreed work in [GitHub Issues](https://github.com/Solacion/Igor/issues) as epics, features, user stories and tasks.

## Accepted decisions — 30 October 2026

| Decision | Accepted answer |
| --- | --- |
| Demo scope | Generic runtime capabilities. IGOR connects to whichever MCP servers the operator configures. The sales-and-stock purchasing scenario is one example, not a requirement. Weather data is not part of the project. |
| Interview deadline | Tuesday 3 November 2026. |
| Example data | Realistic sample data served by fixture-backed MCP servers. No live retail service is required. |
| Write boundary | IGOR may call write-classified tools on MCP servers that support them, only after explicit approval. The demo's fixture write tool, `create_draft_order`, creates a draft only and changes no external system. |
| Unclassified tools | Denied by default. Host configuration is the only source of permission classification; MCP tool annotations are untrusted hints that can only tighten it. |
| Clarification | When required information is missing or uncertain (for example the order date or period, supplier or quantities), the agent asks the operator instead of assuming. Dates are operator input, not fixed in fixtures. |
| Model providers | Anthropic Claude (remote) and LM Studio (local), both behind the provider abstraction. |
| Demo default models | Claude: `claude-haiku-5-5` (cheapest). LM Studio: `qwen/qwen3-vl-8b` (8B, fast, native tool calling). |
| MCP transport and library | stdio only for the demo. [`rmcp`](https://crates.io/crates/rmcp), the official Rust MCP SDK (Apache-2.0, compatible with GPL-3.0-only). |
| Credentials | Environment-variable secret references only for now. macOS Keychain is deferred. |
| Session persistence | In memory only, plus a redacted activity export. |
| Headless harness | An `igor-cli` harness proves the core API before the desktop and is the fallback demo. |
| Core and desktop stack | Rust core; Tauri/Vue desktop UI |
| Licence | GNU GPL version 3 only (`GPL-3.0-only`); applied in the root `LICENSE`. Supersedes the earlier Apache 2.0 selection. |
| Demo platform | macOS |
| Package manager | pnpm only; one lockfile (`pnpm-lock.yaml`). |
| CI gate | The build and test check becomes a required check on `main` once the workspace builds. |
| Verification evidence | Committed under `docs/verification/`. |
| Fixture delivery | Fixtures are served by fixture-backed MCP servers that speak the MCP protocol and implement a defined tool contract, so a live server implementing the same contract can replace one through configuration alone. |
| Example contracts | Contract-first. The example scenario's tool contracts are defined in `docs/specs/`. A live server, such as one for the user's own POS once complete, can implement them later. |
| First milestone surface | Desktop UI, with `igor-cli` as fallback; optional HTTP API deferred |

## Cut order if time runs short

Items 2 to 4 are already outside the accepted demo scope; they are listed so they stay out if scope grows.

1. Issue and pull request templates
2. macOS Keychain
3. Persistence
4. HTTP transport
5. Crash auto-restart
6. LM Studio polish
7. Desktop polish (fall back to the `igor-cli` demo)

## Open questions

None at present. Record accepted answers here and update affected specifications and issues.
