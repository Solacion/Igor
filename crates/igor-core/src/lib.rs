//! Domain-neutral IGOR agent runtime.
//!
//! The core defines the boundaries every other crate builds on:
//!
//! - [`provider`]: the model-provider interface and provider-neutral conversation types.
//! - [`tools`]: the tool-backend interface that MCP clients implement, and server-qualified tool identity.
//! - [`policy`]: host-controlled tool permission classes.
//! - [`events`]: the runtime event stream shared by every client (CLI, desktop, future HTTP API).
//! - [`registry`]: host tool aliases, permission gating and argument validation before dispatch.
//! - [`approval`]: approval of write calls and the dispatch permit, the only path to a server.
//! - [`limits`]: bounds on tool results passed to the model.
//! - [`clock`] and [`ids`]: injected time and identifiers, for deterministic tests.
//! - `testing` (feature `testing`): a scripted provider, a recording tool backend and a manual clock for
//!   tests.
//!
//! The core must not depend on a desktop UI, a specific model provider, an MCP library or any
//! example-scenario schema.

pub mod approval;
mod canonical;
pub mod clock;
pub mod events;
pub mod ids;
pub mod limits;
pub mod policy;
pub mod provider;
pub mod registry;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod tools;

pub use tokio_util::sync::CancellationToken;
