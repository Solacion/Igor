# Clean-checkout reproduction, 31 October 2026

Actual results from a fresh clone that followed the [preparation guide](../user-guides/verification.md), the [CLI guide](../user-guides/cli.md) and the preparation steps of the [desktop guide](../user-guides/desktop.md). Related work: [#39](https://github.com/Solacion/Igor/issues/39), [#37](https://github.com/Solacion/Igor/issues/37). No model provider was called. The desktop window was not opened.

## Run identity

| Field | Value |
| --- | --- |
| Date, time and timezone | 2026-10-31, finished 14:57 SAST |
| Operator / reviewer | Claude Code (Opus 5.5), at the maintainer's request. The operator also wrote parts of the guides. |
| Commit SHA | `3c3112e462ebffc38482a7392cf6a809bfc1b0d7` (main) |
| Changes outside that commit | None: fresh `git clone` |
| macOS version and hardware / RAM | macOS 26.3.1, Apple M4 Max, 48 GB |
| Rust / Node.js / pnpm versions | rustc and cargo 1.99.1 (pinned by `rust-toolchain.toml`), Node.js 22.23.2, pnpm 12.11.2 |
| Checkout type | Fresh clone into a new directory, with an empty `CARGO_HOME` (every crate downloaded) and no Rust build output. Same macOS account: the Rust toolchain was already installed, and pnpm used its existing global store. Not a new account or machine. |
| Preparation commands | `git clone`, `rustup toolchain install`, `cargo fetch --locked`, `pnpm install --frozen-lockfile` |
| Verification command and elapsed time | `pnpm run verify`: passed. Clone to end of verify: 617 s, mostly the crate download and first build. |
| Whole run, including the CLI demo | 627 s |

## Deterministic verification

| Check | Result | Evidence / failure detail |
| --- | --- | --- |
| Locked dependency preparation | PASS | `cargo fetch --locked` and `pnpm install --frozen-lockfile` |
| Formatting, Clippy | PASS | `pnpm run verify` |
| Workspace tests | PASS | 219 passed, 0 failed, 2 ignored (the live tests) |
| Fixture / contract / MCP integration tests | PASS | Included in the totals |
| Core dependency boundary | PASS | `pnpm run verify` |
| Documentation build | PASS | `pnpm run verify` |
| Desktop frontend checks | PASS | `pnpm run desktop:check`: 4 s |
| Desktop native build | PASS | `pnpm run desktop:build`: `Igor.app` bundled (69.71 MiB debug build) in 225 s |
| Desktop demo preparation | PASS | `pnpm run desktop:demo --prepare-only` built the fixture server and printed a disposable config path. The printed directory was then deleted. |

## CLI demo (scripted provider)

Command, exactly as in the CLI guide, answering `y` to the approval and an empty line to end:

```sh
cargo build -p igor-fixture-server -p igor-cli
./target/debug/igor-cli --config examples/igor.toml \
  --provider scripted --script examples/order-script.json \
  "Prepare a draft order for supplier-x for delivery on 2026-07-15."
```

| Observation | Result |
| --- | --- |
| Both servers connect | PASS: `sales: connected, 3 tools`, `supplier: connected, 1 tools` |
| Reads from both servers before the write | PASS: `get_stock_levels`, `get_sales_history`, `get_catalogue` |
| Write preview before the call | PASS: `create_draft_order` arguments and action hash shown, then `Approve this action? [y/N]` |
| Approved write runs once | PASS: `[approval-1] approved`, `[call-4] done` |
| Outcome | PASS: `[completed]`, exit code 0 |

The scripted provider shows orchestration only, not model reasoning. Live LM Studio results are in the [live verification record](2026-10-31-live-lm-studio.md).

## Deviations from the guides and fixes

| Finding | Effect on this run | Fix |
| --- | --- | --- |
| The README and the user guide index still said no runtime had been implemented. | None, but misleading to a reviewer. | Rewritten to describe the demo build and link the guides. |
| `igor-cli --help` and the CLI guide gave the default configuration path as `~/.config/igor/igor.toml`. The code uses `~/Library/Application Support/igor/config.toml`, as the configuration guide says. | None: the demo passes `--config`. | Help text and CLI guide corrected. |
| The CLI guide said no live model had been checked. | None. | Now links the LM Studio record; Claude is still unchecked. |
| No troubleshooting or cleanup guide for providers, keys, servers and local state. | None. | Added [Troubleshooting and cleanup](../user-guides/troubleshooting.md). |

No undocumented step was needed to build, verify or run the scripted demo.

## Limitations

- Same macOS account: the Rust toolchain and the global pnpm store already existed. A run on a new account (option A in #39) has not been done.
- The operator is not independent of the guides.
- The desktop window, and live LM Studio or Claude turns, were not exercised in this run. The maintainer's desktop smoke test is still to do.
- Confirmation run after the fixes: NOT RUN. The fixes change wording and help text only.
