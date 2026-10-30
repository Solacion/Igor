# Desktop shell preview

This preview embeds `igor-core` and shows a footer readiness indicator. The native session bridge can load configuration, prepare Claude or OpenAI-compatible providers, connect configured stdio MCP servers, and run core sessions. The visible UI is still the shell: provider selection and conversation controls belong to [#52](https://github.com/Solacion/Igor/issues/52). The terminal MVP remains the priority.

## Start on macOS

Use the [documented prerequisites](verification.md): Xcode Command Line Tools, pinned Rust, Node.js 22 and pnpm 12.11.2. From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm run desktop
```

The command starts Vite on loopback port 1420 and opens the native Tauri window. The first run downloads/builds Tauri's Rust dependencies and needs network access. A green circle and **Ready** in the footer appear after the local read-only check passes. This indicates that the embedded desktop backend is ready, not that a model or MCP server is connected. An unavailable backend shows a retry action; there is no separate connection card.

Opening just the frontend in a browser cannot access native IPC; the UI explains that it needs the native window rather than simulating a successful connection. Close the native window or press Ctrl+C in the launch terminal to stop the preview.

## Check or build

```sh
pnpm run desktop:check
pnpm --filter @igor/desktop build
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
pnpm run desktop:build
```

The dedicated macOS workflow runs those checks. The debug app is at `apps/desktop/src-tauri/target/debug/bundle/macos/Igor.app`, with its executable also at `apps/desktop/src-tauri/target/debug/igor-desktop`. Distribution signing, installers and releases are outside this slice.

## Boundaries and handoff

- The desktop is a pnpm workspace package under `apps/desktop`. Its Rust package has a separate workspace and committed lockfile so shell work does not edit the core/CLI Cargo graph. It depends inward on `igor-core`; the core has no desktop dependency.
- `desktop_status` accepts a typed `scope: runtime` request and returns application and core schema versions. Session commands use typed requests and generated permissions granted only to the local `main` window. There are no general shell or filesystem commands.
- Production CSP allows bundled scripts/styles and local Tauri IPC; development additionally allows the loopback Vite/HMR connection and inline development styles. No remote scripts/fonts are loaded.
- Native diagnostics are not echoed into the frontend on failure. Native version text is rendered with Vue interpolation, never HTML. Component tests mock Tauri IPC to cover success, failure/retry, pending state and markup escaping.
- The [session bridge contract](../specs/contracts/desktop-sessions.md) documents commands, events, approval boundaries and lifecycle. Deterministic native tests exercise real core sessions with scripted providers; frontend tests verify IPC routing. No paid model calls are made by these tests. **#52** owns provider selection and conversation UI; the footer probe alone does not verify provider availability.

The separate root `pnpm run verify` and Rust dependency audit cover the core workspace, not this standalone desktop graph. Use the desktop workflow/commands above for its validation. Desktop-specific dependency audit coverage remains a follow-up when this shell is adopted; no change to runtime dependency policy is implied.
