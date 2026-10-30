#!/usr/bin/env bash
# Dependency downloads are a separate preparation step; verification must not update the lockfile.
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in cargo pnpm; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing $tool on PATH. See docs/user-guides/verification.md." >&2
    exit 1
  fi
done
if [[ ! -x node_modules/.bin/vitepress ]]; then
  echo "Missing documentation dependencies. Run pnpm install --frozen-lockfile first." >&2
  exit 1
fi

# Also applies to Cargo subprocesses spawned by integration tests.
export CARGO_NET_OFFLINE=true

echo 'Checking Rust formatting'
cargo fmt --all --check

echo 'Linting Rust'
cargo clippy --workspace --all-targets --locked -- -D warnings

echo 'Running deterministic tests (live/ignored tests are not selected)'
cargo test --workspace --locked

echo 'Checking the core dependency boundary'
tree=$(cargo tree -p igor-core --edges normal --prefix none --locked)
if grep -E '^(rmcp|reqwest|tauri)([-_][a-z0-9_-]+)?[ @]' <<< "$tree"; then
  echo 'igor-core depends on a forbidden MCP, HTTP-provider or desktop crate.' >&2
  exit 1
fi

echo 'Building documentation'
pnpm run docs:build

echo 'Verification passed. This does not establish live-provider or end-to-end demo readiness.'
