# Pull request review standards

Every pull request into `main` must pass two automated agent reviews before it can merge: a code review and a security review.

## How the gate works

1. [`agent-review-trigger.yml`](../../.github/workflows/agent-review-trigger.yml) runs on the pull request. It holds no secrets and only records the pull request number.
2. [`agent-review.yml`](../../.github/workflows/agent-review.yml) runs through `workflow_run`, which always uses the copy on `main`. It verifies the recorded number against the triggering commit, reads these standards from `main`, and reviews the pull request as data: no pull request code, hooks, Claude settings or MCP servers run.
3. Results are posted as the `Agent code review` and `Agent security review` check runs by the review GitHub App. The `main` ruleset accepts these checks only from that app, so a pull request that edits any workflow cannot produce a passing check.

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

The agent reviews reduce risk but do not replace human judgement. Treat a pass as necessary, not sufficient.

- Changes to `.github/` or to this file only take effect after they merge, but still need a careful human read before merging.
- Workflows in pull requests from branches in this repository receive repository secrets. Only grant write access to people you trust; fork pull requests get no secrets and need maintainer approval to run workflows.
- A new or changed gate is reviewed by the gate already on `main`. The first gate had to be bootstrapped manually.
