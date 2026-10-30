//! Host-controlled tool permission classes.
//!
//! Host configuration is the only source of classification. Tools without a classification are
//! [`Classification::Unclassified`]: hidden from the model and refused if called. Behaviour hints
//! reported by a server may only tighten a classification: see [`effective_classification`].

use serde::{Deserialize, Serialize};

use crate::tools::ToolHints;

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

/// Applies untrusted server hints to the host classification. Hints only tighten, never relax.
///
/// A host [`Classification::Read`] tool becomes [`Classification::Write`], and so needs approval,
/// when its server reports that it is not read-only or that it may be destructive. Every other
/// classification is returned unchanged: no hint can make a tool more permissive.
pub fn effective_classification(host: Classification, hints: &ToolHints) -> Classification {
    let may_modify = hints.read_only == Some(false) || hints.destructive == Some(true);
    match host {
        Classification::Read if may_modify => Classification::Write,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints(read_only: Option<bool>, destructive: Option<bool>) -> ToolHints {
        ToolHints {
            read_only,
            destructive,
        }
    }

    #[test]
    fn hints_tighten_read_to_write() {
        let host = Classification::Read;
        assert_eq!(effective_classification(host, &hints(None, None)), host);
        assert_eq!(
            effective_classification(host, &hints(Some(true), Some(false))),
            host
        );
        for tightening in [
            hints(Some(false), None),
            hints(None, Some(true)),
            hints(Some(true), Some(true)),
        ] {
            assert_eq!(
                effective_classification(host, &tightening),
                Classification::Write
            );
        }
    }

    #[test]
    fn hints_never_relax() {
        let relaxing = hints(Some(true), Some(false));
        for host in [
            Classification::Write,
            Classification::Denied,
            Classification::Unclassified,
        ] {
            assert_eq!(effective_classification(host, &relaxing), host);
            assert_eq!(
                effective_classification(host, &hints(Some(false), Some(true))),
                host
            );
        }
    }

    #[test]
    fn only_read_and_write_tools_are_offered() {
        assert!(Classification::Read.is_offered());
        assert!(Classification::Write.is_offered());
        assert!(!Classification::Denied.is_offered());
        assert!(!Classification::Unclassified.is_offered());
    }
}
