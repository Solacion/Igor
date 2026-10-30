# Foundation verification — 30 October 2026

## Scope and environment

Codex ran the deterministic checks in a fresh, isolated Git worktree on macOS 26.3.1 (a), build 25D771280a, arm64. RAM was not recorded. The installed toolchains and dependency caches were reused; this is **not** a clean-machine reproduction.

Tested source: `ea8600a9ee9c056f8131e33c160e6614b1ad2d38` plus the uncommitted verification change subsequently committed with this record: `scripts/verify.sh`, the `verify` package script, setup/evidence documentation and navigation links. No runtime source was changed.

Versions: Rust and Cargo 1.99.1; Node.js 22.23.2; pnpm 12.11.2.

## Commands and results

Cargo was absent from the initial terminal's `PATH`. The installed Homebrew Rustup binaries were made available with:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
```

The portable Homebrew command is documented in the [setup guide](../user-guides/verification.md).

| Command / check | Result | Evidence |
| --- | --- | --- |
| `pnpm install --frozen-lockfile --offline` | PASS | 126 cached packages reused; no downloads |
| `cargo fetch --locked --offline` | PASS | Locked dependencies available in cache |
| `bash -n scripts/verify.sh` | PASS | Shell syntax accepted |
| `pnpm run verify` | PASS | All stages completed with Cargo offline |
| Rust formatting | PASS | `cargo fmt --all --check` |
| Rust linting | PASS | Clippy, workspace/all targets, locked; warnings denied |
| Rust tests | PASS | 128 passed, 0 failed, 0 ignored; doc-test suites contained no tests |
| Core dependency boundary | PASS | No normal `rmcp`, `reqwest` or Tauri dependency in `igor-core` |
| Documentation build | PASS | VitePress 1.6.4 built and rendered pages |
| Script failure behaviour | PASS | Temporary fake executables simulated Clippy failure (23), dependency-tree failure (24) and a forbidden dependency (1); each stopped before docs or a success message |
| `git diff --check` | PASS | No whitespace errors |

The 128 tests comprise configuration unit/integration tests (19), core tests (83), fixture-server unit/stdio tests (14), and MCP unit/integration tests (12). MCP integration used real local fixture-server subprocesses. No live model or paid agent review was invoked by this run.

Elapsed time was not measured. The docs build after the full check reported 1.99 seconds; this is not the total verification duration. No CI run is attributed to these local changes.

## Limitations and follow-up

- The worktree was based on `main`; it did not include the unmerged providers or unfinished CLI/weather work in other developers' worktrees.
- No live Claude or LM Studio call, order demo, manual approval demo, desktop test or clean-machine reproduction was performed. Those remain `NOT RUN`.
- Scripted/core tests are not evidence of live-model answer quality or tool-use reliability.
- The script enforces offline Cargo resolution, not network isolation of arbitrary test code. The current checks use local fixture processes; future live tests must remain opt-in.
- A focused manual read compared the verification stages with CI and checked failure propagation. Independent agent review has not run on these changes.
- The later evidence-record page was added after the complete verification; only its documentation build was rerun afterward.

Repeat verification after integration of the providers, CLI and weather fixtures. Then use the [run template](run-template.md) to capture the full demo separately for each live provider, including cost and failures. Do not mark #37 or #39 complete on the strength of this foundation run.
