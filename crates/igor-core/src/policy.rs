//! Host-controlled tool permission classes.
//!
//! Host configuration is the only source of classification. Tools without a classification are
//! [`Classification::Unclassified`]: hidden from the model and refused if called.

use serde::{Deserialize, Serialize};

/// Permission class of a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    /// Runs without approval.
    Read,
    /// Runs only after explicit operator approval of the exact call.
    Write,
    /// Never runs.
    Denied,
    /// Not classified by the host: hidden and refused.
    Unclassified,
}

impl Classification {
    /// True when the model may be offered this tool.
    pub fn is_offered(self) -> bool {
        matches!(self, Self::Read | Self::Write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_read_and_write_tools_are_offered() {
        assert!(Classification::Read.is_offered());
        assert!(Classification::Write.is_offered());
        assert!(!Classification::Denied.is_offered());
        assert!(!Classification::Unclassified.is_offered());
    }
}
