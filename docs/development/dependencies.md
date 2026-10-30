# Rust dependency checks

The [Dependency checks workflow](../../.github/workflows/dependencies.yml) checks the locked workspace with **cargo-deny 0.20.2** on pull requests, pushes to `main` and manual dispatch. It uses no model provider, API key or paid agent review. This addresses the dependency-check portion of [#6](https://github.com/Solacion/Igor/issues/6).

## Policy

The root `deny.toml` defines an explicit SPDX licence allowlist for the current graph. It includes the workspace's `GPL-3.0-only` licence and the selected permissive dependency licences. New or unidentified licences fail rather than being accepted automatically. Dual-licensed crates need an allowed choice; conjunctions need every required licence allowed. Workspace crates are not exempted just because `publish = false`.

The advisory check uses the public [RustSec database](https://github.com/RustSec/advisory-db). Known vulnerabilities fail the check; yanked releases are denied, and unmaintained advisories are considered across the graph. The configuration contains no advisory ignores or per-crate licence exceptions. This is a check of the resolved Rust dependencies, not proof that all code is secure or that a distribution has met every licence obligation. Preserve third-party notices when packaging.

No target filter is configured, so the default resolved graph includes target-specific dependencies. The workflow downloads the pinned official macOS audit binary and verifies its hard-coded SHA-256 before execution. Both Apple Silicon and Intel runner archives are supported.

## Run locally

Install the pinned tool separately from the application:

```sh
cargo install --locked --version 0.20.2 cargo-deny
cargo deny --locked check licenses advisories
```

The first run may download crates and the advisory database. No live model calls are involved. To run only the licence check after dependency preparation:

```sh
cargo deny --locked --offline check licenses
```

Keep this separate from `pnpm run verify`: advisory results depend on a changing external database, while the deterministic verification command keeps Cargo offline. An offline advisory scan requires a previously fetched database and can miss newly published advisories; it is not a substitute for the online CI check.

## Handle a failure

Read the affected crate/version and dependency path in the diagnostic. For a vulnerability or yanked release, first look for a compatible fixed version. For a new licence, review its terms and the actual dependency before changing the allowlist. Do not add a blanket ignore to make a demo pass. A fetch or checksum failure means the check did not complete; it is not a clean audit.

The workflow does not change `Cargo.lock`, update runtime dependencies, enable agent reviews or change branch rules. It is not a required merge check unless a maintainer adds it separately.
