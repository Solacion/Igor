# Workspace

IGOR is a Cargo workspace. The toolchain is pinned in `rust-toolchain.toml`; documentation tooling uses pnpm.

## Crates

| Crate | Purpose | Depends on |
| --- | --- | --- |
| `igor-core` | Domain-neutral runtime: provider and tool-backend interfaces, routing, permissions, approvals, session loop, runtime events | No MCP library, provider SDK or UI |
| `igor-config` | Loads and validates the TOML host configuration: providers, servers, tool permissions, limits, secret references | `igor-core`, `toml` |
| `igor-provider-anthropic` | Anthropic Messages API adapter implementing `igor_core::provider::Provider` | `igor-core`, `igor-config`, `reqwest` (rustls) |
| `igor-mcp` | `rmcp` client implementing `igor_core::tools::ToolBackend` over stdio | `igor-core`, `rmcp` |
| `igor-fixture-server` | Generic fixture-backed MCP server for tests and the demo | `rmcp`; never `igor-core` |
| `igor-cli` | Headless harness over the session API; fallback demo | `igor-core`, `igor-mcp`, providers |

Crates other than `igor-core` are added by the issues that introduce them.

`igor-core` has a non-default `testing` feature with a scripted model provider and a manual clock for deterministic tests. Enable it only from `[dev-dependencies]` so the runtime does not ship it.

## Checks

Run before every pull request; CI runs the same on macOS.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
pnpm install --frozen-lockfile
pnpm run docs:build
```

Library code must not use `unwrap`, `expect` or `panic!` (denied by workspace lints), and public items must be documented. Tests may use `?` with a `Result` return type instead.
