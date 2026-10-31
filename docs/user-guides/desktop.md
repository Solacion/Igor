# Desktop conversation preview

The desktop embeds `igor-core` and provides conversations, provider selection, MCP server status, tool activity, explicit write approvals and clarification replies. Settings owns the live server connections; conversations reuse them. The terminal `igor-cli` demo remains the fallback if desktop integration or live-provider smoke checks fail.

## Start on macOS

Use the [documented prerequisites](verification.md): Xcode Command Line Tools, pinned Rust, Node.js 22 and pnpm 12.11.2. From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm run desktop
```

The command starts Vite on loopback port 1420 and opens the native Tauri window. The first run downloads/builds Tauri's Rust dependencies and needs network access. A green circle and **Ready** in the footer appear after the local read-only check passes. This indicates that the embedded desktop backend is ready, not that a model or MCP server is connected. An unavailable backend shows a retry action; there is no separate connection card.

Opening just the frontend in a browser cannot access native IPC; the UI explains that it needs the native window rather than simulating a successful connection. Close the native window or choose **Quit Igor** for orderly shutdown. Igor keeps the event loop responsive while cancelling the session and awaiting all server connections, including removed/replaced connections still held by session leases. Cleanup runs concurrently with a 15-second total deadline; if the deadline expires, a diagnostic states that cleanup may be incomplete. A forced process kill cannot perform this cleanup.

## One-command fixture demo

Use Node.js 22, pnpm 12.11.2 and the pinned Rust toolchain from the prerequisites guide. Install the locked dependencies from the repository root with `pnpm install --frozen-lockfile`. Before launching the no-cost live option, start LM Studio's OpenAI-compatible server at `http://127.0.0.1:1234/v1` and load **qwen/qwen3-vl-8b**. Then, from the repository root, run:

```sh
pnpm run desktop:demo
```

The launcher lives under `apps/desktop`; the root script forwards to that package. The command builds `igor-fixture-server` with Cargo's lockfile, copies `examples/igor.toml` into a fresh temporary directory with absolute executable/fixture paths, and launches the native app with `IGOR_CONFIG` pointing at that copy. Both **sales** and **supplier** should connect. It does not modify the example or your usual config. Each run starts with a fresh copy; Settings edits are discarded when the launcher exits. No provider request occurs until you send a message.

Choose **lm_studio** in Igor for the no-cost live run. Choosing **claude** requires `ANTHROPIC_API_KEY` in the launch environment and can incur provider charges; the key is never written to the demo config. An unavailable LM Studio server or model is a real failed turn, not an offline scripted response.

### Exact demo script — manual live run NOT RUN

1. Choose **New conversation**, then send this clear request:

   > Create a draft order for supplier-x for delivery on 2026-07-15. Use the stock snapshot from 2026-07-13 and sales from 2026-07-06 to 2026-07-12.

   Expect calls to `sales/get_stock_levels`, `sales/get_sales_history` and `supplier/get_catalogue`, followed by a write preview for `sales/create_draft_order`. Before approval, no draft write should have started. Inspect the full arguments and action hash, and **Approve once**. Expect one started/completed draft call and a draft-only response (`draft-0001`, `submitted: false`), with no supplier submission. If the model needs an ordering quantity or policy, it should ask rather than invent a fact.

2. Choose **New conversation**, send the same clear request and **Deny once** when a write preview appears. Expect a denied approval, no started `create_draft_order` for that request and no claim that a draft was created. Any changed proposal must ask for fresh approval; the prior decision is not reused.

3. Choose **New conversation**, then send the vague request:

   > Create an order for supplier X

   Expect a question about missing delivery/stock/sales dates or other required inputs. Answer with `Supplier supplier-x, delivery 2026-07-15, stock 2026-07-13, sales 2026-07-06 to 2026-07-12.` Expect the answer to be acknowledged and the turn to resume. You can **Stop turn** during clarification; cancelled turns must not be presented as a completed answer. Live model behavior is not guaranteed by scripted tests: a silent date assumption is a failed manual check.

4. Finish or stop the active turn, choose **New conversation** to release its old connection leases, then use **Settings → sales → Remove** and confirm. Return home: sales should be absent; supplier should remain connected. Send the clear request again. Expect an explanation that the sales tools/data are unavailable, without invented stock or sales values or a claimed completed draft. Removal is a configuration change, not necessarily a failed-connection event. Restart the demo command to restore both servers. Alternatively choose **Reconnect** on a configured server to replace its connection and verify it becomes connected again; a brief starting state may finish between polls. Edits do not interrupt a snapshot already being used by a running turn.

### Fixture facts to check

| Product | Stock on 2026-07-13 (cases) | Sales 2026-07-06–12 (cases) | Catalogue price per case | Order multiple | Available |
| --- | ---: | ---: | ---: | ---: | --- |
| COLA-330-24 | 18 | 59 | ZAR 189 | 2 | yes |
| WATER-500-24 | 22 | 76 | ZAR 96 | 2 | yes |
| LEMON-330-24 | 10 | 31 | ZAR 179 | 1 | yes |
| COCOA-1KG-6 | 9 | 8 | ZAR 540 | 1 | no |

Supplier X has a ZAR 1,500 minimum, a two-day lead time and Monday/Wednesday/Friday delivery. 2026-07-15 is a Wednesday, two days after the stock snapshot. Cocoa must be excluded while unavailable. Quantities must be whole ordering units and respect order multiples/minimum value; a live model may propose a different valid replenishment policy, which must be explained. The recorded example's 12 cola cases total ZAR 2,268; that example is not a mandated live-model quantity.

Close Igor's window or choose **Quit Igor** before stopping the launcher so normal app shutdown can reap the fixture children. The launcher removes its temporary directory when the desktop command ends. `pnpm run desktop:demo --prepare-only` from the repository root only builds/prepares and prints the config path; it starts no app/model and intentionally leaves that temporary directory for inspection. Delete that printed directory when finished. This preparation check does not count as a manual/live demo pass.

## Manage MCP servers

Choose **Settings** in the top bar. Under **MCP servers** you can:

- **Add server**: give it a name, the program to run and its arguments. **Test connection** starts the server, lists its tools and stops it, without saving. **Save** shows the exact command line and asks you to confirm before Igor runs it.
- See each server's connection status. A failed start shows the reason. **Reconnect** tries again.
- **Edit** or **Remove** a server. Removing also removes its tool permissions.
- Open **Tools and permissions** to choose what Igor may do with each tool: use it freely (`read`), ask you each time (`write`) or never (`denied`). A new tool is not allowed until you choose. Server descriptions and hints are shown as hints only.

Under **Advanced**, set the working directory, the environment variables the server may inherit (such as `PATH`), secrets and timeouts. A secret is written as `NAME=IGOR_VARIABLE`: the server gets `NAME`, read from Igor's own environment variable `IGOR_VARIABLE`. Only the variable name is saved.

Changes are saved to the [configuration file](configuration.md) shown at the bottom of the page: `$IGOR_CONFIG`, or `~/Library/Application Support/igor/config.toml`. The file is checked with the same rules as at startup and replaced atomically, and comments in a hand-edited file are kept. Configured servers connect when the app opens.

## Conversation and operator decisions

Choose a configured provider on the home screen, enter a message and choose **Send message**. Provider choices come from the configuration file. Missing API credentials show the environment-variable name to set before launching Igor; the desktop never requests or displays key values. Provider calls are made by the embedded runtime, not the browser.

Server status refreshes every three seconds. **Ready** in the footer describes the embedded backend only. The conversation shows each tool proposal, refusal, start, completion and failure. A server error result, truncated result, limit, cancelled turn or failed model request is labelled explicitly. If a tool's effect is unknown, check the external system before retrying; Igor does not automatically retry the action.

Write actions pause on **Review write action**. Read the server, tool, exact arguments and action hash, then choose **Approve** or **Deny**. The canonical argument string comes directly from core and is displayed without reparsing, including large integers. Approval is single-use and tied to that hash; a changed proposal needs a fresh decision. Model/tool instructions cannot click these controls. Denial sends nothing to the tool server.

When Igor asks a question, enter **Your answer** and choose **Send answer**. The controls wait for core confirmation. **Stop turn** remains available during model work, approval and clarification. Cancelling after a tool starts cannot undo an action that already ran. Only one turn runs at a time; provider selection and **New conversation** are disabled until it ends. Opening Settings keeps the conversation mounted; return home to answer or stop it.

Settings edits apply to the next message. An active turn keeps its original permissions and connection leases, even if its server is edited or removed. The desktop has not yet wired the new core history-preserving registry replacement API: when settings/connections change, the next message currently starts a **new model conversation**, with an explicit notice. Earlier messages remain on screen but are not sent as model history after that reset. Changing provider also starts a new model conversation. **New conversation** clears the visible conversation. No conversation persistence is provided.

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
- Session commands stream core event envelopes on a channel scoped to the session. Replies include the session and request identifiers; approval replies echo core's action hash. Unknown request fields are rejected, commands have generated permissions restricted to the local main window, and closing the window cancels the session. Resolved credential values are blocked before core events cross IPC; this is a defensive exact-value check, not a general-purpose secret detector. All model, tool and server content is rendered as plain text.

The separate root `pnpm run verify` and Rust dependency audit cover the core workspace, not this standalone desktop graph. Use the desktop workflow/commands above for its validation. Desktop-specific dependency audit coverage remains a follow-up when this shell is adopted; no change to runtime dependency policy is implied.

## Manual smoke-test checklist — NOT RUN

These are pending checks for the native app and a real configured provider. Automated ScriptedProvider/fixture tests do not establish live-provider or visual correctness. Do not mark these checked without running them.

Use the one-command disposable fixture demo above and follow its exact prompts and expected results. Never put key values in the config, chat or checklist evidence. Preparation/scripted checks do not count as a manual/live pass.

- [ ] **Scenario, not run:** select the provider and confirm the sales fixture server connects. Ask for supplier-x stock as of 13 July 2026 and sales from 6–12 July 2026, then a proposed replenishment draft. Verify each read appears in activity, returned values match the fixture, and missing dates/quantities cause a clarification. Answer it and confirm the turn resumes. Before approving a draft, compare its server/tool, exact arguments and hash with the requested action; approve once, verify one successful `create_draft_order` call and a draft-only result, with no supplier submission.
- [ ] **Denied action, not run:** request another draft and choose Deny. Verify the approval resolves as denied, no call starts for that write, and Igor does not claim it created a draft. Change the proposed quantity and verify a fresh approval and hash are shown.
- [ ] **Stopped server, not run:** stop only the disposable fixture child process. Within a polling interval, verify the server is failed. The next turn must use refreshed connections and must not invent retrieved data. Reconnect in Settings, send another message, and verify recovery. If a call was already in flight when stopped, verify any unknown effect warning remains explicit.
- [ ] **Cancellation, not run:** Stop while a model request is running, while an approval is pending and while a clarification is pending. Confirm the turn ends as cancelled, pending controls become inactive, no pending write starts, and another message can be sent.
- [ ] **Settings during a turn, not run:** while awaiting approval, edit/remove the server or change permissions. Confirm the active turn keeps its snapshot; the next message visibly resets model history and uses the new configuration. Verify only one live connection per current configured server (an old active turn may retain its previous connection until it ends).
- [ ] **Presentation/security, not run:** inspect light/dark mode and the minimum 420×620 window, keyboard focus, long argument/hash wrapping, literal hostile text, missing-key guidance, provider switch, empty-server Settings link, and shutdown of fixture children after closing Igor.

If any required desktop check remains blocked before the Tuesday 3 November 2026 interview, use the CLI demo and record the exact failing step instead of treating automated tests as manual evidence.
