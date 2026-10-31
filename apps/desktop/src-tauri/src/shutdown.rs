//! Delay process exit while native MCP children are reaped; never block the UI event loop.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::{sessions::DesktopRuntime, settings::Settings};

/// Includes a startup grace period and concurrent MCP shutdown, independent of server count.
pub const EXIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Default)]
pub struct ExitState {
    started: AtomicBool,
    finished: AtomicBool,
}
impl ExitState {
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
    pub fn begin(&self) -> bool {
        !self.started.swap(true, Ordering::AcqRel)
    }
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }
}

/// All server connections, including retired connections retained by session leases, belong
/// to Settings' shutdown ledger. Cancel the turn first so no new tool work is dispatched.
pub async fn shutdown(settings: Arc<Settings>, runtime: Arc<DesktopRuntime>) -> bool {
    settings.stop_accepting();
    runtime.shutdown();
    tokio::time::timeout(EXIT_TIMEOUT, settings.shutdown())
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_exit_requests_cannot_start_more_shutdown_tasks() {
        let state = ExitState::default();
        assert!(!state.finished());
        assert!(state.begin());
        assert!(!state.begin());
        state.finish();
        assert!(state.finished());
    }
}
