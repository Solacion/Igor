# Desktop shell preview

This is the first shell slice of [#51](https://github.com/Solacion/Igor/issues/51), not the completed desktop application. It embeds `igor-core`, shows a footer readiness indicator and has a Settings area for MCP servers ([#74](https://github.com/Solacion/Igor/issues/74)). It does not run conversations or send model requests yet. The terminal MVP remains the priority.

## Start on macOS

Use the [documented prerequisites](verification.md): Xcode Command Line Tools, pinned Rust, Node.js 22 and pnpm 12.11.2. From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm run desktop
```

The command starts Vite on loopback port 1420 and opens the native Tauri window. The first run downloads/builds Tauri's Rust dependencies and needs network access. A green circle and **Ready** in the footer appear after the local read-only check passes. This indicates that the embedded desktop backend is ready, not that a model or MCP server is connected. An unavailable backend shows a retry action; there is no separate connection card.

Opening just the frontend in a browser cannot access native IPC; the UI explains that it needs the native window rather than simulating a successful connection. Close the native window or press Ctrl+C in the launch terminal to stop the preview.

## Manage MCP servers

Choose **Settings** in the top bar. Under **MCP servers** you can:

- **Add server**: give it a name, the program to run and its arguments. **Test connection** starts the server, lists its tools and stops it, without saving. **Save** shows the exact command line and asks you to confirm before Igor runs it.
- See each server's connection status. A failed start shows the reason. **Reconnect** tries again.
- **Edit** or **Remove** a server. Removing also removes its tool permissions.
- Open **Tools and permissions** to choose what Igor may do with each tool: use it freely (`read`), ask you each time (`write`) or never (`denied`). A new tool is not allowed until you choose. Server descriptions and hints are shown as hints only.

Under **Advanced**, set the working directory, the environment variables the server may inherit (such as `PATH`), secrets and timeouts. A secret is written as `NAME=IGOR_VARIABLE`: the server gets `NAME`, read from Igor's own environment variable `IGOR_VARIABLE`. Only the variable name is saved.

Changes are saved to the [configuration file](configuration.md) shown at the bottom of the page: `$IGOR_CONFIG`, or `~/Library/Application Support/igor/config.toml`. The file is checked with the same rules as at startup and replaced atomically, and comments in a hand-edited file are kept. Configured servers connect when the app opens.

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
- `desktop_status` accepts a typed `scope: runtime` request, rejects unknown scopes/fields, and returns application version and core event-schema version.
- The `settings_*` commands read and edit MCP servers and tool permissions through `igor-config`, and start servers through `igor-mcp`. Requests reject unknown fields. Secret values never cross IPC; only environment-variable names do. Each command has a generated permission granted only to the local `main` window. There are no shell, general filesystem or model-provider capabilities.
- Production CSP allows bundled scripts/styles and local Tauri IPC; development additionally allows the loopback Vite/HMR connection and inline development styles. No remote scripts/fonts are loaded.
- Native diagnostics are not echoed into the frontend on failure. Native version text is rendered with Vue interpolation, never HTML. Component tests mock Tauri IPC to cover success, failure/retry, pending state and markup escaping.
- **Still open in #51:** the real session command/event bridge, approval/clarification commands, cancellation and shutdown integration. **#52** owns provider selection and conversation UI. Settings and the status probe must not be presented as evidence that those features work.

The separate root `pnpm run verify` and Rust dependency audit cover the core workspace, not this standalone desktop graph. Use the desktop workflow/commands above for its validation. Desktop-specific dependency audit coverage remains a follow-up when this shell is adopted; no change to runtime dependency policy is implied.
