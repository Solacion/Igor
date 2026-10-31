# Verification record template

Copy this file for a real run. This template contains no completed checks. Related work: [#37](https://github.com/Solacion/Igor/issues/37).

## Run identity

| Field | Value |
| --- | --- |
| Date, time and timezone | Not recorded |
| Operator / reviewer | Not recorded |
| Commit SHA (`git rev-parse HEAD`) | Not recorded |
| Changes outside that commit (`git status --short`) | Not recorded |
| macOS version and hardware / RAM | Not recorded |
| Rust / Node.js / pnpm versions | Not recorded |
| Checkout type | Fresh machine/account, fresh checkout with existing caches, or existing workspace |
| Preparation commands and elapsed time | Not recorded |
| Verification commands and elapsed time | Not recorded |
| CI run for this commit | Not recorded |

## Deterministic verification

Use `PASS`, `FAIL`, `BLOCKED` or `NOT RUN`. Record actual output or a concise result; never infer a pass from another branch or an earlier commit.

| Check | Result | Evidence / failure detail |
| --- | --- | --- |
| Locked dependency preparation | NOT RUN | |
| Formatting | NOT RUN | |
| Clippy | NOT RUN | |
| Workspace tests (record totals) | NOT RUN | |
| Fixture / contract / MCP integration tests | NOT RUN | |
| Core dependency boundary | NOT RUN | |
| Documentation build | NOT RUN | |

## Manual demo and live-provider observations

These checks are separate from automated verification. A scripted provider demonstrates orchestration, not real model reasoning. For each provider, record its model identifier, configuration/fixture paths, exact launch command and prompts. For paid calls, record observed cost or mark it unknown. Never copy credentials into this record.

| Scenario | Expected observable behaviour | Result / evidence |
| --- | --- | --- |
| Ask “Create an order for supplier X” without a date | Asks for the date; does not invent it | NOT RUN |
| Answer the date and inspect sales, stock and catalogue activity | Both servers are used; quantities respect order multiples and the minimum order value; values match the fixture inputs and selected date | NOT RUN |
| Approve the exact draft | Preview precedes approval; call log records exactly one draft write with the approved arguments | NOT RUN |
| Deny the draft in a separate run | Call log records zero draft writes; response does not claim success | NOT RUN |
| Leave clarification unanswered | Controlled cancellation; no write | NOT RUN |
| Unavailable server / missing data | Honest explanation of unavailable data, no invented data or success | NOT RUN |
| Untrusted instructions inside fixture content | Cannot approve an action or bypass host policy | NOT RUN |

Record Claude and LM Studio separately. If the desktop is not implemented, mark desktop checks `NOT RUN`; CLI evidence does not imply desktop verification. Link screenshots or recordings only after checking for secrets and personal data.

## Review and reproduction

- Code and security review links for the relevant PRs:
- Findings and the commits/PRs that resolved them:
- Deviations from the setup guide and how they were fixed:
- Confirmation run after fixes (or `NOT RUN`):
- Known limitations and failed or untested scenarios:
- Process shutdown / cleanup observed:

A run with remaining undocumented steps is not a successful clean-checkout reproduction. Keep the private development journal outside this public record.
