//! Bounds on data passed between tools and the model.

use thiserror::Error;

/// Marker appended to a result cut at the size limit, visible to the model and the operator.
pub const TRUNCATION_MARKER: &str = "\n[IGOR: result truncated at the size limit]";

/// Default maximum size of a tool result passed to the model, in bytes.
pub const DEFAULT_MAX_RESULT_BYTES: usize = 32 * 1024;

/// Maximum size, in UTF-8 bytes, of tool-result text passed to the model, including the
/// [`TRUNCATION_MARKER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultSizeLimit {
    max_bytes: usize,
}

impl ResultSizeLimit {
    /// Creates a limit. It must leave room for the [`TRUNCATION_MARKER`].
    pub fn new(max_bytes: usize) -> Result<Self, LimitTooSmall> {
        if max_bytes < TRUNCATION_MARKER.len() {
            return Err(LimitTooSmall {
                requested: max_bytes,
                minimum: TRUNCATION_MARKER.len(),
            });
        }
        Ok(Self { max_bytes })
    }

    /// Maximum size in bytes.
    pub fn max_bytes(self) -> usize {
        self.max_bytes
    }

    /// Returns `text` unchanged when it fits. Otherwise cuts it at a character boundary and
    /// appends the [`TRUNCATION_MARKER`], so the result is at most [`Self::max_bytes`] long.
    pub fn apply(self, mut text: String) -> BoundedText {
        let original_bytes = text.len();
        let truncated = truncate_with_marker(&mut text, self.max_bytes, TRUNCATION_MARKER);
        BoundedText {
            text,
            truncated,
            original_bytes,
        }
    }
}

/// Cuts `text` at a character boundary and appends `marker` so the result is at most `max_bytes`
/// long. Returns whether it was cut. `max_bytes` must be at least `marker.len()`; if it is not,
/// the result is `marker` alone.
pub(crate) fn truncate_with_marker(text: &mut String, max_bytes: usize, marker: &str) -> bool {
    if text.len() <= max_bytes {
        return false;
    }
    let mut keep = max_bytes.saturating_sub(marker.len());
    while !text.is_char_boundary(keep) {
        keep -= 1;
    }
    text.truncate(keep);
    text.push_str(marker);
    true
}

impl Default for ResultSizeLimit {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_RESULT_BYTES,
        }
    }
}

/// Text after applying a [`ResultSizeLimit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedText {
    /// The text, ending in [`TRUNCATION_MARKER`] when truncated.
    pub text: String,
    /// The original text exceeded the limit.
    pub truncated: bool,
    /// Size of the original text in bytes.
    pub original_bytes: usize,
}

/// A requested limit cannot hold the truncation marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("result size limit of {requested} bytes is below the minimum of {minimum} bytes")]
pub struct LimitTooSmall {
    /// The requested limit.
    pub requested: usize,
    /// The smallest accepted limit.
    pub minimum: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 100;

    fn limit() -> Result<ResultSizeLimit, LimitTooSmall> {
        ResultSizeLimit::new(LIMIT)
    }

    #[test]
    fn text_below_the_limit_is_unchanged() -> Result<(), LimitTooSmall> {
        let bounded = limit()?.apply("a".repeat(LIMIT - 1));
        assert!(!bounded.truncated);
        assert_eq!(bounded.text, "a".repeat(LIMIT - 1));
        Ok(())
    }

    #[test]
    fn text_at_the_limit_is_unchanged() -> Result<(), LimitTooSmall> {
        let bounded = limit()?.apply("a".repeat(LIMIT));
        assert!(!bounded.truncated);
        assert_eq!(bounded.text.len(), LIMIT);
        Ok(())
    }

    #[test]
    fn text_above_the_limit_is_truncated_with_marker() -> Result<(), LimitTooSmall> {
        let bounded = limit()?.apply("a".repeat(LIMIT + 1));
        assert!(bounded.truncated);
        assert_eq!(bounded.original_bytes, LIMIT + 1);
        assert_eq!(bounded.text.len(), LIMIT);
        assert!(bounded.text.ends_with(TRUNCATION_MARKER));
        Ok(())
    }

    #[test]
    fn truncation_respects_character_boundaries() -> Result<(), LimitTooSmall> {
        let bounded = limit()?.apply("é".repeat(LIMIT));
        assert!(bounded.truncated);
        assert!(bounded.text.len() <= LIMIT);
        let kept = bounded.text.trim_end_matches(TRUNCATION_MARKER);
        assert!(kept.chars().all(|c| c == 'é'));
        Ok(())
    }

    #[test]
    fn limit_must_hold_the_marker() {
        assert!(ResultSizeLimit::new(TRUNCATION_MARKER.len() - 1).is_err());
        assert!(ResultSizeLimit::new(TRUNCATION_MARKER.len()).is_ok());
        assert_eq!(
            ResultSizeLimit::default().max_bytes(),
            DEFAULT_MAX_RESULT_BYTES
        );
    }
}
