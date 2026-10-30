# Pull request review standards

Every pull request into `main` must pass two automated agent reviews before it can merge: a code review and a security review. Both run in [`.github/workflows/agent-review.yml`](../../.github/workflows/agent-review.yml) and read this file from `main`, so a pull request cannot weaken the standards it is reviewed against.

## Severity and gating

| Severity | Meaning | Blocks merge |
| --- | --- | --- |
| critical | Exploitable vulnerability, data loss, or approval bypass | Yes |
| high | Incorrect behaviour, security weakness, or a clear violation of these standards | Yes |
| medium | Code smell, framework anti-pattern, or avoidable inefficiency | Yes |
| low | Optional suggestion that is a matter of taste, not a defect | No |

A review that fails to run, or returns no result, blocks the merge.

## Code review

### Code smells

- Duplicated logic that should be shared.
- Functions or components doing more than one job, or too long to follow.
- Deep nesting where early returns or smaller functions would read better.
- Dead code, commented-out code, unused dependencies, and leftover debugging output.
- Magic numbers and strings that should be named constants or configuration.
- Unclear names, misleading comments, or comments that restate the code.
- Speculative abstraction, configuration or extension points with no current use.
- Tests that assert nothing meaningful, depend on timing, or need network access outside live checks.

### Rust

- Code passes `cargo fmt` and `cargo clippy` without new `allow` attributes unless justified in a comment.
- Library code returns typed errors (`thiserror` or equivalent); no `unwrap`, `expect` or `panic!` outside tests and proven invariants.
- No `unsafe` unless essential, minimal, and documented with a `// SAFETY:` comment.
- Prefer borrowing over cloning; avoid needless allocations, `Box`, `Arc` or `Mutex`.
- Async code does not block the executor, holds no locks across `.await`, and supports cancellation.
- Public items are documented; visibility is as narrow as possible.
- The core crate does not depend on the desktop UI, a specific model provider, or any example-scenario schema.

### Tauri

- Capabilities and permissions grant only what each window needs.
- Every IPC command validates its input and returns typed errors; no business logic lives in the frontend.
- A strict Content Security Policy is set; no remote code is loaded.
- Secrets never reach the frontend.

### Vue and TypeScript

- Composition API with `<script setup lang="ts">`; TypeScript strict mode; no `any` without justification.
- Components are small and focused; shared state uses composables or a store, not prop drilling.
- No `v-html` on untrusted content; tool and model output is always rendered as text.
- Reactive work is not repeated unnecessarily: use `computed` over methods for derived state and stable `key`s in lists.

### Performance and legibility

- Flag inefficient algorithms, repeated work, unbounded growth, and unnecessary I/O or network calls.
- Do not request micro-optimisations that make code harder to read without a measurable benefit.

## Security review

- **Secrets:** no credentials, tokens or keys in code, configuration, tests, fixtures or logs.
- **Approvals:** write-classified tool calls cannot run without approval of the exact server, tool and arguments. Model output and tool content can never grant approval.
- **Untrusted input:** model output, MCP tool results and fixture data are untrusted. Check for prompt injection paths, injection into shells, file paths or HTML, and unsafe deserialisation.
- **Tool routing:** tools are identified by server and name; arguments are validated against `inputSchema` before dispatch.
- **Resource limits:** timeouts, iteration limits and result-size limits are present; writes with an unknown outcome are never retried automatically.
- **Dependencies:** new dependencies are maintained, necessary and pinned through lock files.
- **CI and workflows:** changes to `.github/` do not widen token permissions, expose secrets to untrusted code, or weaken the review gate.

## Review process limits

The agent reviews reduce risk but do not replace human judgement. Treat a pass as necessary, not sufficient. Changes to `.github/` or to this file need a careful human read before merging.
