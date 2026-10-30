//! Domain-neutral IGOR agent runtime.
//!
//! The core defines the boundaries every other crate builds on:
//!
//! - [`provider`]: the model-provider interface and provider-neutral conversation types.
//! - [`tools`]: the tool-backend interface that MCP clients implement, and server-qualified tool identity.
//! - [`policy`]: host-controlled tool permission classes.
//! - [`events`]: the runtime event stream shared by every client (CLI, desktop, future HTTP API).
//! - [`registry`]: host tool aliases, permission gating and argument validation before dispatch.
//! - [`limits`]: bounds on tool results passed to the model.
//!
//! The core must not depend on a desktop UI, a specific model provider, an MCP library or any
//! example-scenario schema.

mod canonical;
pub mod events;
pub mod limits;
pub mod policy;
pub mod provider;
pub mod registry;
pub mod tools;

pub use tokio_util::sync::CancellationToken;
