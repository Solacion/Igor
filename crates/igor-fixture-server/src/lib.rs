//! Generic fixture-backed MCP server.
//!
//! Serves recorded `tools/list` definitions and `tools/call` results from a fixture file over
//! stdio, so tests and demos need no live services. It speaks MCP through `rmcp` and does not
//! depend on `igor-core`: the IGOR runtime cannot tell it from a live server.
//!
//! The fixture format, matching rules and call log are documented in
//! `docs/specs/contracts/fixture-format.md`.

mod call_log;
mod canonical;
mod fixture;
mod server;

pub use call_log::{CallLog, CallLogError};
pub use fixture::{Fixture, FixtureError};
pub use server::FixtureServer;
