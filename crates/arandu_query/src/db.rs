use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::explain::RebuildLog;
use salsa::Storage;

pub type FileId = u32;

/// Per-file CST cache for incremental [`crate::passes::syntax_tree`] rebuilds.
#[derive(Default)]
struct CstCache {
    /// Last successful tree per file (text must match before reuse).
    by_file: HashMap<FileId, arandu_parser::SyntaxTree>,
}

pub use crate::stable_hash::StableHash;

#[derive(Clone)]
pub struct HashEq<T> {
    pub value: Arc<T>,
    hash: blake3::Hash,
}

impl<T: StableHash> HashEq<T> {
    pub fn new(value: T) -> Self {
        let hash = value.stable_hash();
        Self {
            value: Arc::new(value),
            hash,
        }
    }

    /// Wrap an existing `Arc` (compute hash once).
    #[must_use]
    pub fn from_arc(value: Arc<T>) -> Self {
        let hash = value.stable_hash();
        Self { value, hash }
    }

    /// Share the same `Arc` and hash without re-hashing or deep-cloning `T`.
    #[must_use]
    pub fn share(other: &Self) -> Self {
        Self {
            value: Arc::clone(&other.value),
            hash: other.hash,
        }
    }
}

impl<T> PartialEq for HashEq<T> {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}
impl<T> Eq for HashEq<T> {}

impl<T> std::ops::Deref for HashEq<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

#[salsa::db]
pub trait ArandCompilerDb: salsa::Database {
    fn source_text(&self, file: FileId) -> Arc<str>;
    fn file_path(&self, file: FileId) -> Arc<PathBuf>;
    /// Registered Salsa input for this numeric file id, if any.
    fn source_file_by_id(&self, file: FileId) -> Option<SourceFile>;
    fn as_source_db(&self) -> &dyn arandu_middle::db::SourceDatabase;
    /// Downcast to [`DatabaseImpl`] for CST cache / incremental reparse (default: none).
    fn as_db_impl(&self) -> Option<&DatabaseImpl> {
        None
    }
}

pub use arandu_middle::db::SourceFile;

/// Internal shared state for the file registry.
///
/// Two maps are kept in sync at every insertion point:
/// - `by_path`  — `String → SourceFile` for import path resolution (O(1) by path)
/// - `by_id`    — `FileId → SourceFile` for Salsa queries (O(1) by FileId)
///
/// Before this change both `source_text` and `file_path` performed an O(N)
/// linear scan over `by_path.values()` to find a file by its numeric ID.
#[derive(Default, Clone)]
struct FileRegistry {
    by_path: HashMap<String, SourceFile>,
    by_id: HashMap<FileId, SourceFile>,
}

impl FileRegistry {
    /// Insert a file into both indexes simultaneously.
    fn insert(&mut self, path: String, file_id: FileId, file: SourceFile) {
        self.by_path.insert(path, file);
        self.by_id.insert(file_id, file);
    }

    /// Next available FileId (starts at 100 to avoid collisions with test stubs).
    fn next_id(&self) -> FileId {
        self.by_path.len() as FileId + 100
    }
}

/// Salsa database with optional DX.5 rebuild logging.
///
/// Prefer [`Self::default`] / [`Self::new`] for production (no event callback).
/// Use [`Self::with_rebuild_log`] when `-Zexplain-rebuild` is active.
#[salsa::db]
pub struct DatabaseImpl {
    storage: Storage<Self>,
    files: Arc<Mutex<FileRegistry>>,
    /// Incremental CST reuse across `syntax_tree` queries (side cache; result still pure in text).
    cst_cache: Arc<Mutex<CstCache>>,
    /// Shared with the Salsa event callback when explain mode is on.
    rebuild_log: Option<Arc<RebuildLog>>,
}

// Manual Clone: Storage is cloneable; share Arc file registry + log + CST cache.
impl Clone for DatabaseImpl {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            files: Arc::clone(&self.files),
            cst_cache: Arc::clone(&self.cst_cache),
            rebuild_log: self.rebuild_log.clone(),
        }
    }
}

impl Default for DatabaseImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[salsa::db]
impl salsa::Database for DatabaseImpl {}

impl DatabaseImpl {
    /// Database without rebuild event overhead.
    #[must_use]
    pub fn new() -> Self {
        Self {
            storage: Storage::new(None),
            files: Arc::new(Mutex::new(FileRegistry::default())),
            cst_cache: Arc::new(Mutex::new(CstCache::default())),
            rebuild_log: None,
        }
    }

    /// Database with DX.5 causal-chain recording (Salsa `WillExecute` / validate).
    #[must_use]
    pub fn with_rebuild_log() -> (Self, Arc<RebuildLog>) {
        let log = RebuildLog::new();
        let callback = RebuildLog::salsa_callback(Arc::clone(&log));
        let db = Self {
            storage: Storage::new(Some(callback)),
            files: Arc::new(Mutex::new(FileRegistry::default())),
            cst_cache: Arc::new(Mutex::new(CstCache::default())),
            rebuild_log: Some(Arc::clone(&log)),
        };
        (db, log)
    }

    #[must_use]
    pub fn rebuild_log(&self) -> Option<&Arc<RebuildLog>> {
        self.rebuild_log.as_ref()
    }

    pub fn new_file(&mut self, path: String, text: String) -> SourceFile {
        let mut reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let file_id = reg.next_id();
        let file = SourceFile::new(
            self,
            file_id,
            Arc::from(text),
            Arc::new(std::path::PathBuf::from(&path)),
        );
        reg.insert(path, file_id, file);
        file
    }

    pub fn register_source_file(&self, path: String, file: SourceFile) {
        let mut reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
        let file_id = file.file_id(self.as_source_db());
        reg.insert(path, file_id, file);
    }

    /// O(1) reverse lookup: compiler `FileId` → open/registered [`SourceFile`].
    #[must_use]
    pub fn source_file_by_id(&self, file_id: FileId) -> Option<SourceFile> {
        let reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
        reg.by_id.get(&file_id).copied()
    }

    /// Build or reuse CST for `file_id`/`text` via [`arandu_parser::reparse_subtree`] when possible.
    /// Shares the `Arc<str>` buffer with the tree (no extra text copy).
    pub(crate) fn syntax_tree_for_arc(
        &self,
        file_id: FileId,
        text: Arc<str>,
    ) -> arandu_parser::SyntaxTree {
        let mut cache = self.cst_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(prev) = cache.by_file.get(&file_id) {
            if prev.text() == text.as_ref() {
                return prev.clone();
            }
            if let Some((start, end, repl)) =
                arandu_parser::single_contiguous_edit(prev.text(), text.as_ref())
            {
                let (_src, tree) = arandu_parser::reparse_subtree(prev, start, end, &repl);
                if tree.text() == text.as_ref() {
                    cache.by_file.insert(file_id, tree.clone());
                    return tree;
                }
            }
        }
        let tree = arandu_parser::parse_syntax_arc(text);
        cache.by_file.insert(file_id, tree.clone());
        tree
    }
}

impl arandu_middle::db::SourceDatabase for DatabaseImpl {
    fn exported_symbols(&self, file: SourceFile) -> Arc<arandu_middle::ExportedSymbolTable> {
        crate::passes::exported_symbols(self, file)
    }

    fn symbol_span(&self, symbol_id: arandu_middle::SymbolId) -> arandu_base::Span {
        crate::passes::symbol_span(self, symbol_id)
    }

    fn parse_file(
        &self,
        file: SourceFile,
    ) -> Result<Arc<arandu_parser::Program>, arandu_parser::ParseError> {
        let res = crate::passes::parse(self, file);
        match &*res {
            Ok(p) => Ok(Arc::clone(p)),
            Err(e) => Err(e.clone()),
        }
    }

    fn resolve_file(&self, file: SourceFile) -> Arc<arandu_middle::ResolutionResult> {
        crate::passes::resolve(self, file).value.clone()
    }

    fn resolve_module_path(&self, path: &str) -> Option<SourceFile> {
        // Fast path: O(1) lookup by import path string.
        {
            let reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(file) = reg.by_path.get(path) {
                return Some(*file);
            }
        }

        // Uncached: walk up the directory tree until we find the file.
        let mut current = std::env::current_dir().ok()?;
        let mut found_path = None;
        loop {
            let candidate = current.join(path);
            if candidate.exists() {
                found_path = Some(candidate);
                break;
            }
            if let Some(parent) = current.parent() {
                current = parent.to_path_buf();
            } else {
                break;
            }
        }

        let found_path = found_path?;
        let text = std::fs::read_to_string(&found_path).ok()?;

        let mut reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
        // Double-check: another thread may have inserted it while we were reading.
        if let Some(file) = reg.by_path.get(path) {
            return Some(*file);
        }

        let file_id = reg.next_id();
        let file = SourceFile::new(self, file_id, Arc::from(text), Arc::new(found_path));
        reg.insert(path.to_string(), file_id, file);

        Some(file)
    }
}

#[salsa::db]
impl ArandCompilerDb for DatabaseImpl {
    /// O(1) lookup by FileId via the reverse index.
    fn source_text(&self, file: FileId) -> Arc<str> {
        let reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
        reg.by_id
            .get(&file)
            .map(|f| f.text(self.as_source_db()))
            .unwrap_or_else(|| Arc::from(""))
    }

    /// O(1) lookup by FileId via the reverse index.
    fn file_path(&self, file: FileId) -> Arc<PathBuf> {
        let reg = self.files.lock().unwrap_or_else(|e| e.into_inner());
        reg.by_id
            .get(&file)
            .map(|f| f.path(self.as_source_db()))
            .unwrap_or_else(|| Arc::new(PathBuf::new()))
    }

    fn source_file_by_id(&self, file: FileId) -> Option<SourceFile> {
        DatabaseImpl::source_file_by_id(self, file)
    }

    fn as_source_db(&self) -> &dyn arandu_middle::db::SourceDatabase {
        self
    }

    fn as_db_impl(&self) -> Option<&DatabaseImpl> {
        Some(self)
    }
}
