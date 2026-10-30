# Desktop session bridge

The Tauri host composes existing config, provider, MCP and core crates. The frontend API is `apps/desktop/src/session.ts`; orchestration and policy stay in Rust. This is the native bridge slice of #51, ready for the conversation UI in #52.

## Commands

Commands are restricted to the local main window. Typed request objects reject unknown fields. One session and one turn may be active at a time.

| Command | Request | Result |
| --- | --- | --- |
| `start_session` | `config_path` (absolute), `provider_id`; separate `events` channel | Session ID, provider/model names, initial server status/tool counts |
| `send_message` | `session_id`, `text` | Core `TurnOutcome` |
| `resolve_approval` | `session_id`, `approval_id`, `action_hash`, `decision` (`approve`/`deny`) | Input queued; observe events for acceptance/rejection |
| `answer_clarification` | `session_id`, `request_id`, `answer` | Input queued; observe events |
| `cancel_turn` | `session_id` | Requests cancellation; idle cancellation is harmless |
| `close_session` | `session_id` | Cancels work and disconnects MCP children |

Text/answers are nonempty, NUL-free and at most 32 KiB. IDs are nonempty, contain no control characters and are at most 128 bytes. Paths are at most 4096 bytes. Bridge errors serialize as fixed `{code: "..."}` values without native paths or provider diagnostics.

Startup loads existing TOML configuration and resolves credentials in the native process. Provider IDs must exist in the file. Stdio servers inherit only the configured allowlist and explicit environment entries. Failed MCP connections are returned as disconnected server status; their tools are absent. Provider preparation makes no model request and does not prove credentials/network availability.

## Events and approvals

Each start creates its own Tauri channel carrying unchanged core `EventEnvelope` values: schema version, per-session sequence, and tagged runtime event. There are no global listeners. Consumers must ignore unknown variants and render all model/tool strings as plain text. Text arrives when the provider completes its response; token streaming is outside this slice.

Show the exact approval tool, canonical arguments and action hash supplied by the core, then echo that hash with the operator's decision. Native commands queue answers; only the core validates identity/hash and permits dispatch. A successful IPC return does not mean an approval was accepted. Wrong/stale inputs produce `input_rejected`. There is no frontend command for calling a tool directly.

Resolved provider keys and explicit server credential values are checked against event string values and object keys. If a known credential appears, the whole event is blocked and the session cancelled; approval arguments are never rewritten. The turn returns `sensitive_output`. This is an exact-value defensive check, not a general secret detector: encoded secrets and arbitrary inherited environment values are not identified.

## Lifecycle and limits

Short state locks allow approval, clarification and cancellation while a model turn waits. Concurrent turns/starts return `busy`. Session IDs increase during the process lifetime; old IDs cannot control new sessions. Closing the window cancels startup/turns and disconnects children. A failed channel cancels its own session. Dropped invocation futures release their reservations.

MCP discovery happens once at startup. Live reconnect, tool-list refresh, dynamic connection events and provider switching are outside this slice. Initial server availability is in `SessionInfo`; the green footer reports desktop backend readiness only. Interactive UI and live-provider evidence are required before claiming a complete desktop demo.

## Verification

Native tests use the real core with scripted providers and recording tool backends to verify ordered events/history, write approval, wrong hashes, denial, clarification, cancellation, stale IDs, dropped-turn recovery and credential blocking. Frontend tests verify scoped commands, approval arguments and channel disposal. The macOS workflow builds the native app. Tests never call a paid model.
