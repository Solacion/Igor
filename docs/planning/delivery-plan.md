# Proposed delivery plan

These are planning milestones, not created GitHub Issues. No code work has started.

| Milestone | Intended outcome | Completion evidence |
| --- | --- | --- |
| 1. Agree demo contract | Select data mode, draft boundary, provider, MCP connections and target platform | Updated specification and decisions |
| 2. Establish core boundaries | Minimal provider, MCP routing and permission interfaces | Deterministic tests for routing, validation and approval rules |
| 3. Connect example integrations | Example tool contracts; fixture-backed MCP servers implementing them | Contract checks for both servers and two-server integration test |
| 4. Deliver desktop flow | Ask, inspect recommendation, review draft and approve/deny applicable actions | Recorded manual smoke test and failure-path checks |
| 5. Prepare interview handoff | Clean setup, demo script, limitations and review evidence | Successful documented reproduction from a clean checkout |

## Implementation process after approval

Create GitHub Issues manually from the agreed milestones. Each issue should state the user-visible result, scope, acceptance criteria, dependencies and relevant specification links. Keep tasks small enough for focused implementation and review.

Use Claude Code to implement approved tasks. Every pull request into `main` must pass the gated agent code and security reviews described in the [review standards](../development/review-standards.md). Review resulting changes, run relevant checks, record findings and address material defects before considering a task complete. Automation through Oddyseus/n8n is not required.

## Planned verification

- Unit tests: tool identity collisions, argument validation, permission defaults, approval binding, denial and cancellation.
- Integration tests: both MCP servers, provider responses, missing data, timeouts and uncertain write outcomes using controlled fixtures.
- Desktop smoke test: one complete example scenario plus a denied action and a server failure.
- Reproduction check: documented prerequisites, safe sample configuration, exact startup/test steps and expected demo results.

Live-provider checks should be clearly separated from deterministic tests and document any credentials, network access and cost required. Add actual commands and results only when the implementation exists.
