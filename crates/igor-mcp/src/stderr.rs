//! Bounded, redacted capture of a server's stderr.
//!
//! A background task reads the child's stderr line by line until it closes. Each line has the
//! secret values given to the server replaced with [`REDACTED`] before it is stored, and is cut
//! to [`MAX_STDERR_LINE_CHARS`]. Only the last [`MAX_STDERR_LINES`] lines are kept. The text is
//! untrusted server output: display it as plain text only.
//!
//! Redaction matches each secret's exact text. A server that prints a secret transformed, such
//! as encoded or split across lines, is not caught.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStderr;
use tokio::task::JoinHandle;

/// Number of most recent stderr lines kept per server.
pub const MAX_STDERR_LINES: usize = 50;

/// Characters kept from each stderr line; the rest is replaced with `…`.
pub const MAX_STDERR_LINE_CHARS: usize = 500;

/// Replaces a secret value in captured stderr.
pub const REDACTED: &str = "[REDACTED]";

/// Bytes read from one line before the rest of it is skipped. Larger than the kept characters,
/// so a secret that starts within the kept part is read whole and redacted.
const MAX_READ_LINE_BYTES: usize = 16 * 1024;

/// How long a failed start waits for the server's last stderr lines.
pub(crate) const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);

/// The most recent stderr lines of one server.
#[derive(Debug, Clone, Default)]
pub(crate) struct StderrLog {
    lines: Arc<Mutex<VecDeque<String>>>,
}

impl StderrLog {
    fn lock(&self) -> MutexGuard<'_, VecDeque<String>> {
        self.lines.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, line: String) {
        let mut lines = self.lock();
        if lines.len() == MAX_STDERR_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// The kept lines, oldest first.
    pub(crate) fn snapshot(&self) -> Vec<String> {
        self.lock().iter().cloned().collect()
    }
}

/// Starts reading `stderr` into a new log. The task ends when stderr closes.
pub(crate) fn capture(stderr: ChildStderr, secrets: Vec<String>) -> (StderrLog, JoinHandle<()>) {
    let log = StderrLog::default();
    let mut secrets: Vec<String> = secrets.into_iter().filter(|s| !s.is_empty()).collect();
    // Longest first, so a secret that contains another is replaced whole.
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    let task = tokio::spawn(read_lines(BufReader::new(stderr), log.clone(), secrets));
    (log, task)
}

async fn read_lines(mut reader: BufReader<ChildStderr>, log: StderrLog, secrets: Vec<String>) {
    let mut line = Vec::new();
    loop {
        let (consumed, end_of_line) = match reader.fill_buf().await {
            Ok([]) | Err(_) => break,
            Ok(buffer) => {
                let (chunk, end_of_line) = match buffer.iter().position(|&b| b == b'\n') {
                    Some(newline) => (buffer.get(..newline).unwrap_or_default(), true),
                    None => (buffer, false),
                };
                let room = MAX_READ_LINE_BYTES.saturating_sub(line.len());
                line.extend_from_slice(chunk.get(..room.min(chunk.len())).unwrap_or_default());
                (chunk.len() + usize::from(end_of_line), end_of_line)
            }
        };
        reader.consume(consumed);
        if end_of_line {
            log.push(clean(&line, &secrets));
            line.clear();
        }
    }
    if !line.is_empty() {
        log.push(clean(&line, &secrets));
    }
}

/// Redacts `secrets` in one raw line, then cuts it to [`MAX_STDERR_LINE_CHARS`].
fn clean(raw: &[u8], secrets: &[String]) -> String {
    let mut text = String::from_utf8_lossy(raw)
        .trim_end_matches('\r')
        .to_owned();
    for secret in secrets {
        if text.contains(secret.as_str()) {
            text = text.replace(secret.as_str(), REDACTED);
        }
    }
    match text.char_indices().nth(MAX_STDERR_LINE_CHARS) {
        Some((cut, _)) => {
            text.truncate(cut);
            text.push('…');
            text
        }
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_every_secret_longest_first() {
        let secrets = vec!["abc".to_owned(), "abcdef".to_owned()];
        let mut sorted = secrets.clone();
        sorted.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        assert_eq!(
            clean(b"key=abcdef and abc\r", &sorted),
            "key=[REDACTED] and [REDACTED]"
        );
    }

    #[test]
    fn long_lines_are_cut_after_redaction() {
        let secret = "s".repeat(40);
        let raw = format!("{}{secret}{}", "a".repeat(480), "b".repeat(100));
        let line = clean(raw.as_bytes(), std::slice::from_ref(&secret));
        assert!(!line.contains(&secret));
        assert!(line.ends_with('…'));
        assert_eq!(line.chars().count(), MAX_STDERR_LINE_CHARS + 1);
    }

    #[test]
    fn keeps_only_the_most_recent_lines() {
        let log = StderrLog::default();
        for n in 0..MAX_STDERR_LINES + 5 {
            log.push(n.to_string());
        }
        let lines = log.snapshot();
        assert_eq!(lines.len(), MAX_STDERR_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("5"));
    }

    #[test]
    fn invalid_utf8_is_replaced_not_dropped() {
        assert_eq!(clean(b"bad \xff byte", &[]), "bad \u{fffd} byte");
    }
}
