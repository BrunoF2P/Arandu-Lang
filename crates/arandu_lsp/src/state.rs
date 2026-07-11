//! Server state: AnalysisHost + DocumentStore + VFS + URI maps.

use crate::vfs::Vfs;
use arandu_middle::resolved::NodeKey;
use arandu_query::db::SourceFile;
use arandu_query::{AnalysisHost, AnalysisRevision, AnalysisSnapshot, DocumentId, DocumentStore};
use arandu_semantics::TypeCheckResult;
use crate::uri_util::{parse_uri, path_from_uri, uri_from_path};
use lsp_types::Uri;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct ServerState {
    pub host: AnalysisHost,
    pub docs: DocumentStore,
    pub vfs: Vfs,
    pub by_uri: FxHashMap<String, DocumentId>,
    /// Numeric compiler `file_id` → open document (multi-file workspace).
    pub by_file_id: FxHashMap<u32, DocumentId>,
    /// Last published diagnostic fingerprint per document (skip no-op publish).
    pub last_diag_fp: FxHashMap<DocumentId, [u8; 32]>,
    /// P3: last per-item IDE diag fingerprints (DocumentId, item local key).
    pub last_item_diag_fp: FxHashMap<(DocumentId, u32, u32), [u8; 32]>,
    next_file_id: u32,
}

impl ServerState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            host: AnalysisHost::new(),
            docs: DocumentStore::new(),
            vfs: Vfs::new(),
            by_uri: FxHashMap::default(),
            by_file_id: FxHashMap::default(),
            last_diag_fp: FxHashMap::default(),
            last_item_diag_fp: FxHashMap::default(),
            next_file_id: 10_000,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> AnalysisSnapshot {
        self.host.snapshot()
    }

    #[must_use]
    pub fn revision(&self) -> AnalysisRevision {
        self.host.revision()
    }

    fn path_of(uri: &Uri) -> PathBuf {
        path_from_uri(uri)
    }

    /// Open document or apply committed text (after VFS flush).
    pub fn open_or_commit(&mut self, uri: &Uri, text: String) -> DocumentId {
        let path = Self::path_of(uri);
        let uri_s = uri.as_str().to_string();
        if let Some(&id) = self.by_uri.get(&uri_s) {
            if let Some(doc) = self.docs.get_mut(id) {
                let source = doc.source;
                let fid = source.file_id(self.host.db());
                self.host.set_text(source, Arc::from(text));
                self.by_file_id.insert(fid, id);
                return id;
            }
            self.by_uri.remove(&uri_s);
        }
        let file_id = self.next_file_id;
        self.next_file_id = self.next_file_id.wrapping_add(1);
        let source = SourceFile::new(
            self.host.db(),
            file_id,
            Arc::from(text),
            Arc::new(path.clone()),
        );
        self.host
            .register_source_file(path.to_string_lossy().into_owned(), source);
        let id = self.docs.open(path, source);
        self.by_uri.insert(uri_s, id);
        self.by_file_id.insert(file_id, id);
        id
    }

    pub fn close_uri(&mut self, uri: &Uri) {
        let uri_s = uri.as_str();
        if let Some(id) = self.by_uri.remove(uri_s) {
            if let Some(doc) = self.docs.get(id) {
                let fid = doc.source.file_id(self.host.db());
                self.by_file_id.remove(&fid);
            }
            self.docs.close(id);
            self.last_diag_fp.remove(&id);
            self.last_item_diag_fp.retain(|&(doc, _, _), _| doc != id);
        }
        // Drop pending edits for this URI; re-queue the rest.
        let remaining: Vec<(String, String)> = self
            .vfs
            .take_all()
            .into_iter()
            .filter(|(u, _)| u != uri_s)
            .collect();
        for (u, text) in remaining {
            self.vfs.push_full_text(u, text);
        }
    }

    /// Queue a change; does **not** bump Salsa revision until flush.
    pub fn queue_change(&mut self, uri: &Uri, text: String) {
        self.vfs.push_full_text(uri.as_str().to_string(), text);
    }

    /// Commit due VFS edits; returns (uri, DocumentId) pairs that were committed.
    pub fn flush_due(&mut self) -> Vec<(Uri, DocumentId)> {
        let due = self.vfs.take_due();
        self.commit_edits(due)
    }

    /// Commit all pending (didSave / flush).
    pub fn flush_all(&mut self) -> Vec<(Uri, DocumentId)> {
        let all = self.vfs.take_all();
        self.commit_edits(all)
    }

    fn commit_edits(&mut self, edits: Vec<(String, String)>) -> Vec<(Uri, DocumentId)> {
        let mut out = Vec::with_capacity(edits.len());
        for (uri_s, text) in edits {
            let Some(uri) = parse_uri(&uri_s) else {
                continue;
            };
            let id = self.open_or_commit(&uri, text);
            out.push((uri, id));
        }
        out
    }

    /// Tightest name/ref node containing `offset`.
    pub fn symbol_at(tc: &TypeCheckResult, offset: u32) -> Option<arandu_middle::SymbolId> {
        let mut best: Option<(u32, arandu_middle::SymbolId)> = None;
        let consider = |map: &rustc_hash::FxHashMap<NodeKey, arandu_middle::SymbolId>,
                        best: &mut Option<(u32, arandu_middle::SymbolId)>| {
            for (key, &sym) in map {
                if key.start <= offset && offset < key.end {
                    let w = key.end.saturating_sub(key.start);
                    if best.is_none_or(|(bw, _)| w < bw) {
                        *best = Some((w, sym));
                    }
                }
            }
        };
        consider(&tc.resolved.value_refs, &mut best);
        consider(&tc.resolved.type_refs, &mut best);
        consider(&tc.resolved.definitions, &mut best);
        best.map(|(_, s)| s)
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Register `.aru` files under `root` into the Salsa DB (without editor open).
/// Caps at 256 files to keep initialize cheap.
pub fn walk_register_aru(st: &mut ServerState, root: &std::path::Path) {
    let mut stack = vec![root.to_path_buf()];
    let mut n = 0u32;
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() {
                if p.file_name().and_then(|s| s.to_str()) == Some("target") {
                    continue;
                }
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("aru") {
                if n >= 256 {
                    return;
                }
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                let Some(uri) = uri_from_path(&p) else {
                    continue;
                };
                st.open_or_commit(&uri, text);
                n += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn file_url(name: &str) -> Uri {
        uri_from_path(std::path::Path::new(&format!("/tmp/{name}")))
            .or_else(|| parse_uri(&format!("file:///tmp/{name}")))
            .expect("uri")
    }

    #[test]
    fn queue_change_does_not_bump_revision_until_flush() {
        let mut st = ServerState::new();
        // Instant flush for test.
        st.vfs = Vfs::with_debounce(Duration::from_millis(0));
        let uri = file_url("a.aru");
        let id = st.open_or_commit(&uri, "func main() {}".into());
        let r0 = st.revision();

        st.queue_change(&uri, "func main() { let x = 1; }".into());
        assert_eq!(
            st.revision(),
            r0,
            "pending VFS must not touch AnalysisRevision"
        );

        let committed = st.flush_all();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].1, id);
        assert_ne!(st.revision(), r0, "commit must advance revision");
    }

    #[test]
    fn n_changes_one_commit_one_revision_bump() {
        let mut st = ServerState::new();
        st.vfs = Vfs::with_debounce(Duration::from_millis(0));
        let uri = file_url("b.aru");
        st.open_or_commit(&uri, "func main() {}".into());
        let r0 = st.revision();

        st.queue_change(&uri, "v1".into());
        st.queue_change(&uri, "v2".into());
        st.queue_change(&uri, "v3".into());
        assert_eq!(st.vfs.pending_count(), 1);

        let committed = st.flush_all();
        assert_eq!(committed.len(), 1);
        // One flush of one file → one set_text → one bump from r0.
        assert_eq!(st.revision().as_u64(), r0.as_u64() + 1);
    }

    #[test]
    fn closed_document_id_is_stale() {
        let mut st = ServerState::new();
        let uri = file_url("c.aru");
        let id = st.open_or_commit(&uri, "func main() {}".into());
        assert!(st.docs.get(id).is_some());
        st.close_uri(&uri);
        assert!(st.docs.get(id).is_none());
        assert!(!st.by_uri.contains_key(uri.as_str()));
    }
}
