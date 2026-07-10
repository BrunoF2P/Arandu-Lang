//! A10.c — generational document handles via [`slotmap`].
//!
//! Open LSP / session documents are stored in a [`SlotMap`]. Keys embed a
//! generation counter: after `remove`, an old [`DocumentId`] fails `get` and
//! cannot be confused with a reopened file at the same slot index.
//!
//! This is the library solution (not hand-rolled gen+index pairs).

use slotmap::{new_key_type, SlotMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::db::SourceFile;

new_key_type! {
    /// Generational handle for an open source document.
    pub struct DocumentId;
}

/// Metadata for one open document (path + Salsa input).
#[derive(Clone)]
pub struct OpenDocument {
    pub path: Arc<PathBuf>,
    pub source: SourceFile,
}

/// Registry of open documents with generational IDs.
#[derive(Default)]
pub struct DocumentStore {
    docs: SlotMap<DocumentId, OpenDocument>,
    /// Path string → id (one owned key; no linear scan on open set).
    by_path: rustc_hash::FxHashMap<PathBuf, DocumentId>,
}

impl DocumentStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a document at `path`. Returns the generational id.
    pub fn open(&mut self, path: PathBuf, source: SourceFile) -> DocumentId {
        if let Some(old) = self.by_path.remove(&path) {
            // Remove+insert so clients holding the old id become invalid (A10.c).
            self.docs.remove(old);
        }
        let path_arc = Arc::new(path.clone());
        let id = self.docs.insert(OpenDocument {
            path: path_arc,
            source,
        });
        self.by_path.insert(path, id);
        id
    }

    /// Drop document; subsequent use of `id` returns `None`.
    pub fn close(&mut self, id: DocumentId) -> Option<OpenDocument> {
        let doc = self.docs.remove(id)?;
        self.by_path.remove(doc.path.as_ref());
        Some(doc)
    }

    #[must_use]
    pub fn get(&self, id: DocumentId) -> Option<&OpenDocument> {
        self.docs.get(id)
    }

    #[must_use]
    pub fn get_mut(&mut self, id: DocumentId) -> Option<&mut OpenDocument> {
        self.docs.get_mut(id)
    }

    #[must_use]
    pub fn by_path(&self, path: &Path) -> Option<DocumentId> {
        self.by_path.get(path).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DatabaseImpl;

    #[test]
    fn closed_id_is_stale() {
        let db = DatabaseImpl::default();
        let mut store = DocumentStore::new();
        let src = crate::db::SourceFile::new(
            &db,
            1,
            Arc::from("func main() {}"),
            Arc::new(PathBuf::from("a.aru")),
        );
        let id = store.open(PathBuf::from("a.aru"), src);
        assert!(store.get(id).is_some());
        store.close(id);
        assert!(store.get(id).is_none(), "generation must invalidate handle");
    }

    #[test]
    fn reopen_does_not_revive_old_id() {
        let db = DatabaseImpl::default();
        let mut store = DocumentStore::new();
        let path = PathBuf::from("a.aru");
        let src1 =
            crate::db::SourceFile::new(&db, 1, Arc::from("func main() {}"), Arc::new(path.clone()));
        let old = store.open(path.clone(), src1);
        store.close(old);
        let src2 =
            crate::db::SourceFile::new(&db, 2, Arc::from("func main() {}"), Arc::new(path.clone()));
        let new = store.open(path, src2);
        assert_ne!(old, new);
        assert!(store.get(old).is_none());
        assert!(store.get(new).is_some());
    }
}
