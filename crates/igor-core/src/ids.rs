//! Injected identifier generation.
//!
//! The runtime creates identifiers only through [`IdGenerator`], so tests can produce
//! byte-stable events and snapshots.

use std::sync::atomic::{AtomicU64, Ordering};

/// Source of identifiers that are unique within one generator.
pub trait IdGenerator: Send + Sync {
    /// Returns a new identifier, different from every earlier one from this generator.
    fn next_id(&self) -> String;
}

/// Generates `<prefix>-1`, `<prefix>-2` and so on.
///
/// Deterministic and unique per generator, but predictable: do not use it where an identifier
/// must be unguessable.
#[derive(Debug)]
pub struct SequentialIds {
    prefix: String,
    next: AtomicU64,
}

impl SequentialIds {
    /// Creates a generator whose first identifier is `<prefix>-1`.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            next: AtomicU64::new(1),
        }
    }
}

impl IdGenerator for SequentialIds {
    fn next_id(&self) -> String {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        format!("{}-{n}", self.prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_ids_are_deterministic_and_unique() {
        let ids = SequentialIds::new("call");
        assert_eq!(ids.next_id(), "call-1");
        assert_eq!(ids.next_id(), "call-2");
        assert_eq!(SequentialIds::new("call").next_id(), "call-1");
    }
}
