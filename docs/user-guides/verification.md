# Prepare and verify a checkout

This guide covers the deterministic checks on the code currently in the workspace. It does not claim the CLI demo or a live model has been verified. Related work: [#39](https://github.com/Solacion/Igor/issues/39).

## macOS prerequisites

- Xcode Command Line Tools (`xcode-select --install` if not installed).
- Rustup with Cargo available on `PATH`. The repository pins Rust **1.99.1**, rustfmt and Clippy in `rust-toolchain.toml`.
- Node.js **22** and pnpm **12.11.2**, as recorded by `package.json`. Record exact installed patch versions in the verification evidence.
- Git.

If Rustup was installed with Homebrew, Cargo may need its directory added to your terminal's `PATH`:

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH"
```

## Prepare dependencies

From a fresh clone, run the following in its root. These preparation steps need network access; they do not call a model API or need a model-provider key.

```sh
git clone https://github.com/Solacion/Igor.git
cd Igor
rustup toolchain install
cargo fetch --locked
pnpm install --frozen-lockfile
```

For a branch under review, check it out before preparing dependencies. Record the commit actually tested, not just the branch name.

## Run verification

```sh
pnpm run verify
```

Alternatively, `bash scripts/verify.sh` runs the same checks. The script stops at the first failure and returns a nonzero exit status. It runs:

1. Rust formatting and Clippy across the workspace and all targets.
2. Workspace tests, including the integration and fixture/contract checks present in that checkout. Ignored live-provider tests are not selected.
3. The same core dependency-boundary check as CI: normal dependencies must not introduce `rmcp`, `reqwest` or Tauri into `igor-core`.
4. The documentation HTML build, including VitePress's link validation.

Cargo is offline during verification, including inside test subprocesses. Tests may use stdio and loopback connections; this script is not a network sandbox. Review new tests to keep external requests opt-in. Documentation dependencies must already be installed. There are no model calls in the verification script itself.

A passing result establishes these checks only. It does not prove the live model asks the right clarification question, calculates a sensible order or completes the interview demo.

## Troubleshooting and cleanup

- **`cargo` not found:** check the Rustup installation and `PATH`, then run `cargo --version`.
- **Offline Cargo resolution fails:** repeat `cargo fetch --locked` with network access. Do not remove `--locked` to work around a dependency mismatch.
- **Documentation dependencies missing:** run `pnpm install --frozen-lockfile` from the checkout being tested.
- **Loopback tests fail in a restricted environment:** record the restriction separately from a code failure and rerun with local connections permitted.
- **A test hangs:** interrupt with Ctrl+C and record the command and last visible test. A completed run is required before recording a pass.

The verification command does not start a persistent docs server. Build output is under `target/` and `docs/.vitepress/dist/`; dependencies are under `node_modules/`, with docs cache under `docs/.vitepress/cache/`. These are ignored by Git. `cargo clean` removes Rust build output. Remove only these generated directories when clearing a checkout; keep configuration and private notes.

## Record the result

Copy the [run template](../verification/run-template.md) into a dated Markdown file under `docs/verification/`. Capture the commit, environment, exact commands, results and limitations. A warmed-cache worktree run is useful evidence, but is not a clean-machine reproduction. Keep deterministic results, manual demo observations and paid live-provider checks separate.
