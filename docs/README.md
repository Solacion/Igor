# Documentation

## Specifications

- [Product and demo specification](specs/igor.md): purpose, scope, requirements and acceptance criteria.
- [Architecture proposal](specs/architecture.md): boundaries, interfaces and execution flow.
- [Decisions and open questions](specs/decisions.md): agreed direction, proposals and unresolved choices.
- [Tool contracts](specs/contracts/README.md): example scenario tools, result shapes, error results and the fixture file format.

## Planning

- [Delivery plan](planning/delivery-plan.md): proposed milestones, validation and review process.

## Development

- [Pull request review standards](development/review-standards.md): gated agent code and security reviews.
- [Workspace](development/workspace.md): crates, dependency rules and local checks.

## Verification

- [Desktop smoke-test checklist](verification/desktop-smoke-test.md): manual LM Studio and Claude result columns; initially not run.

- [Prepare and verify a checkout](user-guides/verification.md).
- [Foundation verification, 30 October 2026](verification/2026-10-30-foundation-checks.md): actual local results and limitations.
- [Live LM Studio verification, 31 October 2026](verification/2026-10-31-live-lm-studio.md): deterministic checks plus live CLI demo scenarios with a local model.
- [Clean-checkout reproduction, 31 October 2026](verification/2026-10-31-clean-checkout.md): fresh clone, verify, scripted CLI demo and desktop build.
- [Local model evaluation, 31 October 2026](verification/2026-10-31-model-evaluation.md): which LM Studio model suits the voice demo.
- [Verification record template](verification/run-template.md): separate automated, manual and live-provider evidence.

## User guides

- [User guide](user-guides/README.md): where to start, the demo workflow and model choices.
- [Desktop](user-guides/desktop.md): `pnpm run desktop:demo` and the demo script.
- [Command-line harness](user-guides/cli.md): run a request headlessly, including the offline scripted demo.
- [Troubleshooting and cleanup](user-guides/troubleshooting.md).

Specifications describe intended behaviour, not existing capabilities. Update the relevant specification when an approved decision changes scope. Add operational instructions only once they can be verified against a working implementation.
