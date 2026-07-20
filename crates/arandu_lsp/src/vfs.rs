//! Re-export shared text-edit VFS from `arandu_query`.
//!
//! Debounce lives in one place so CLI watch and LSP cannot drift.

pub use arandu_query::EditVfs as Vfs;
// Debounce constant lives in arandu_query::DEFAULT_DEBOUNCE (shared with CLI watch).

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

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
