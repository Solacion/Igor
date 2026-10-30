//! Per-run JSON Lines log of every `tools/call` the server receives.
//!
//! Tests use the log as the server-side oracle for "this tool was or was not invoked". A record is
//! written and flushed before the response is sent, so it exists even if the response is delayed
//! or lost.

use std::path::Path;

use thiserror::Error;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::canonical::canonical_json;
use crate::fixture::MatchKind;

/// The call log could not be opened or written.
#[derive(Debug, Error)]
#[error("call log {path}: {source}")]
pub struct CallLogError {
    path: String,
    source: std::io::Error,
}

/// Append-only call log.
#[derive(Debug)]
pub struct CallLog {
    path: String,
    // Async mutex: it serialises whole records, and the write is awaited while it is held.
    state: Mutex<LogState>,
}

#[derive(Debug)]
struct LogState {
    file: File,
    next_seq: u64,
}

impl CallLog {
    /// Creates or truncates the log file at `path`.
    pub async fn create(path: &Path) -> Result<Self, CallLogError> {
        let display = path.display().to_string();
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
            .map_err(|source| CallLogError {
                path: display.clone(),
                source,
            })?;
        Ok(Self {
            path: display,
            state: Mutex::new(LogState { file, next_seq: 1 }),
        })
    }

    /// Appends one record and flushes it.
    ///
    /// Record shape: `{"seq":1,"tool":"…","arguments":{…canonical…},"match":"exact","matched":true}`.
    pub async fn record(
        &self,
        tool: &str,
        canonical_arguments: &str,
        kind: MatchKind,
    ) -> Result<(), CallLogError> {
        let mut state = self.state.lock().await;
        let line = format!(
            "{{\"seq\":{},\"tool\":{},\"arguments\":{},\"match\":\"{}\",\"matched\":{}}}\n",
            state.next_seq,
            canonical_json(&serde_json::Value::String(tool.to_owned())),
            canonical_arguments,
            kind.as_str(),
            kind.is_matched(),
        );
        let write = async {
            state.file.write_all(line.as_bytes()).await?;
            state.file.flush().await
        };
        write.await.map_err(|source| CallLogError {
            path: self.path.clone(),
            source,
        })?;
        state.next_seq += 1;
        Ok(())
    }
}
