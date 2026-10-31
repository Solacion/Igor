//! Native-only activity capture. Store core-redacted lines, never a second raw transcript.
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use igor_core::events::EventEnvelope;
use igor_core::session::EventSink;

const MAX_EXPORT_BYTES: usize = 16 * 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
pub struct ActivityLog {
    bytes: Vec<u8>,
    incomplete: bool,
}
impl ActivityLog {
    fn record(&mut self, event: &EventEnvelope) {
        if self.incomplete {
            return;
        }
        let mut line = Vec::new();
        if igor_core::export::write_event(event, &mut line).is_err()
            || self.bytes.len().saturating_add(line.len()) > MAX_EXPORT_BYTES
        {
            self.incomplete = true;
            self.bytes.clear();
        } else {
            self.bytes.extend(line);
        }
    }
    pub fn snapshot(&self) -> Result<Vec<u8>, crate::sessions::BridgeError> {
        if self.incomplete {
            return Err(crate::sessions::BridgeError::Export);
        }
        Ok(self.bytes.clone())
    }
}

pub struct RecordingSink {
    pub next: Arc<dyn EventSink>,
    pub log: Arc<Mutex<ActivityLog>>,
}
impl EventSink for RecordingSink {
    fn emit(&self, event: EventEnvelope) {
        if let Ok(mut log) = self.log.lock() {
            log.record(&event);
        }
        self.next.emit(event);
    }
}

/// Fixed native-owned path, atomic replacement; frontend cannot select a write destination.
pub fn save(directory: &Path, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(directory)?;
    let target = directory.join("activity.jsonl");
    let temp = directory.join(format!(
        ".activity-{}-{}.tmp",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, &target)?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use igor_core::events::RuntimeEvent;
    use igor_core::tools::{ServerId, ToolId};
    use serde_json::json;
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn saves_core_redacted_json_lines_and_replaces_previous_export() -> TestResult {
        let mut log = ActivityLog::default();
        log.record(&EventEnvelope::new(
            0,
            RuntimeEvent::ToolCallProposed {
                call_id: "call-1".into(),
                tool: ToolId {
                    server: ServerId::new("sales")?,
                    name: "order".into(),
                },
                arguments: json!({"supplier":"private-supplier", "lines":[{"quantity":123}]}),
            },
        ));
        log.record(&EventEnvelope::new(
            1,
            RuntimeEvent::AssistantText {
                text: "private-model-answer".into(),
            },
        ));
        let dir = std::env::temp_dir().join(format!("igor-activity-export-{}", std::process::id()));
        let path = save(&dir, &log.snapshot()?)?;
        let text = std::fs::read_to_string(&path)?;
        assert!(!text.contains("private-supplier"));
        assert!(!text.contains("private-model-answer"));
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0]["event"]["arguments"],
            json!({"supplier":"<redacted>", "lines":"<redacted>"})
        );
        assert_eq!(lines[1]["event"]["text"], "<redacted> (20 bytes)");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)?.permissions().mode() & 0o777,
                0o600
            );
        }
        save(&dir, b"replacement\n")?;
        assert_eq!(std::fs::read_to_string(path)?, "replacement\n");
        std::fs::remove_dir_all(dir)?;
        Ok(())
    }

    #[test]
    fn refuses_partial_exports_when_capture_exceeds_limit() {
        let mut log = ActivityLog {
            bytes: vec![0; MAX_EXPORT_BYTES],
            incomplete: false,
        };
        log.record(&EventEnvelope::new(
            0,
            RuntimeEvent::AssistantText {
                text: "answer".into(),
            },
        ));
        assert!(log.snapshot().is_err());
        assert!(log.bytes.is_empty());
    }
}
