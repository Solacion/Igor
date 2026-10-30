# Desktop shell preview

This is the first shell slice of [#51](https://github.com/Solacion/Igor/issues/51), not the completed desktop application. It embeds `igor-core`, shows a real read-only IPC connection check and provides a small Vue UI. It does not run conversations, load credentials, connect MCP servers, or send model requests. The terminal MVP remains the priority.

## Start on macOS

Use the [documented prerequisites](verification.md): Xcode Command Line Tools, pinned Rust, Node.js 22 and pnpm 12.11.2. From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm run desktop
```

The command starts Vite on loopback port 1420 and opens the native Tauri window. The first run downloads/builds Tauri's Rust dependencies and needs network access. Click **Check connection** to repeat the local read-only check. Models and tools correctly remain “Not connected yet”.

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
- The only application command, `desktop_status`, accepts a typed `scope: runtime` request, rejects unknown scopes/fields, and returns application version and core event-schema version. A generated command permission is granted only to the local `main` window. There are no shell, filesystem or model-provider capabilities.
- Production CSP allows bundled scripts/styles and local Tauri IPC; development additionally allows the loopback Vite/HMR connection and inline development styles. No remote scripts/fonts are loaded.
- Native diagnostics are not echoed into the frontend on failure. Native version text is rendered with Vue interpolation, never HTML. Component tests mock Tauri IPC to cover success, failure/retry, pending state and markup escaping.
- **Still open in #51:** the real session command/event bridge, approval/clarification commands, cancellation and shutdown integration. **#52** owns provider selection and conversation UI. This status probe must not be presented as evidence that those features work.

The separate root `pnpm run verify` and Rust dependency audit cover the core workspace, not this standalone desktop graph. Use the desktop workflow/commands above for its validation. Desktop-specific dependency audit coverage remains a follow-up when this shell is adopted; no change to runtime dependency policy is implied.
