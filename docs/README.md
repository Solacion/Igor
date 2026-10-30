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

- [Prepare and verify a checkout](user-guides/verification.md).
- [Foundation verification, 30 October 2026](verification/2026-10-30-foundation-checks.md): actual local results and limitations.
- [Verification record template](verification/run-template.md): separate automated, manual and live-provider evidence.

## User guides

- [Desktop shell preview](user-guides/desktop.md): local startup and current limits.
- [User guide status and intended workflow](user-guides/README.md).
- [Command-line harness](user-guides/cli.md): run a request headlessly, including the offline scripted demo.

Specifications describe intended behaviour, not existing capabilities. Update the relevant specification when an approved decision changes scope. Add operational instructions only once they can be verified against a working implementation.
