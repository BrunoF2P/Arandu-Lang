//! Virtual file system layer: pending edits + debounce before Salsa commit.
//!
//! `didChange` accumulates here; the main loop flushes after
//! [`DEFAULT_DEBOUNCE`] of quiet time (or on explicit flush / didSave).

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Quiet period before committing pending text to the Salsa DB.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(100);

/// Pending full-document texts keyed by URI string.
#[derive(Debug, Default)]
pub struct Vfs {
    pending: HashMap<String, PendingEdit>,
    debounce: Duration,
}

#[derive(Debug, Clone)]
struct PendingEdit {
    text: String,
    /// Last change time; flush when `now >= changed_at + debounce`.
    changed_at: Instant,
}

impl Vfs {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    #[must_use]
    #[allow(dead_code)] // used by ServerState tests + unit tests
    pub fn with_debounce(debounce: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            debounce,
        }
    }

    /// Queue a full-document replace (LSP `TextDocumentSyncKind::FULL`).
    pub fn push_full_text(&mut self, uri: String, text: String) {
        self.pending.insert(
            uri,
            PendingEdit {
                text,
                changed_at: Instant::now(),
            },
        );
    }

    /// Whether any file is waiting to be committed.
    #[must_use]
    #[allow(dead_code)] // unit tests
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Time until the next pending edit becomes due, if any.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Duration> {
        let now = Instant::now();
        self.pending
            .values()
            .map(|p| {
                let due = p.changed_at + self.debounce;
                if due <= now {
                    Duration::ZERO
                } else {
                    due.saturating_duration_since(now)
                }
            })
            .min()
    }

    /// Drain edits whose debounce window has elapsed.
    pub fn take_due(&mut self) -> Vec<(String, String)> {
        let now = Instant::now();
        let mut due = Vec::new();
        let mut keep = HashMap::new();
        for (uri, edit) in self.pending.drain() {
            if edit.changed_at + self.debounce <= now {
                due.push((uri, edit.text));
            } else {
                keep.insert(uri, edit);
            }
        }
        self.pending = keep;
        due
    }

    /// Drain **all** pending edits immediately (didSave / shutdown / request flush).
    pub fn take_all(&mut self) -> Vec<(String, String)> {
        self.pending.drain().map(|(uri, e)| (uri, e.text)).collect()
    }

    /// Number of pending files (tests / diagnostics).
    #[must_use]
    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn debounce_batches_multiple_pushes() {
        let mut vfs = Vfs::with_debounce(Duration::from_millis(40));
        vfs.push_full_text("file:///a.aru".into(), "v1".into());
        vfs.push_full_text("file:///a.aru".into(), "v2".into());
        vfs.push_full_text("file:///a.aru".into(), "v3".into());
        assert_eq!(vfs.pending_count(), 1);
        assert!(vfs.take_due().is_empty(), "should still be inside debounce");
        thread::sleep(Duration::from_millis(50));
        let due = vfs.take_due();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].1, "v3");
        assert!(!vfs.has_pending());
    }

    #[test]
    fn take_all_flushes_immediately() {
        let mut vfs = Vfs::with_debounce(Duration::from_secs(10));
        vfs.push_full_text("file:///a.aru".into(), "x".into());
        let all = vfs.take_all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].1, "x");
    }
}
