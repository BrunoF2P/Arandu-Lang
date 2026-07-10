use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub type FileId = u32;

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
    fn as_source_db(&self) -> &dyn arandu_middle::db::SourceDatabase;
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

#[derive(Default, Clone)]
#[salsa::db]
pub struct DatabaseImpl {
    storage: salsa::Storage<Self>,
    files: Arc<std::sync::Mutex<FileRegistry>>,
}

#[salsa::db]
impl salsa::Database for DatabaseImpl {}

impl DatabaseImpl {
    pub fn new_file(&mut self, path: String, text: String) -> SourceFile {
        let mut reg = self.files.lock().unwrap();
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
        let mut reg = self.files.lock().unwrap();
        let file_id = file.file_id(self.as_source_db());
        reg.insert(path, file_id, file);
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
            Ok(p) => Ok(Arc::new(p.clone())),
            Err(e) => Err(e.clone()),
        }
    }

    fn resolve_file(&self, file: SourceFile) -> Arc<arandu_middle::ResolutionResult> {
        crate::passes::resolve(self, file).value.clone()
    }

    fn resolve_module_path(&self, path: &str) -> Option<SourceFile> {
        // Fast path: O(1) lookup by import path string.
        {
            let reg = self.files.lock().unwrap();
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

        let mut reg = self.files.lock().unwrap();
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
        let reg = self.files.lock().unwrap();
        reg.by_id
            .get(&file)
            .map(|f| f.text(self.as_source_db()))
            .unwrap_or_else(|| Arc::from(""))
    }

    /// O(1) lookup by FileId via the reverse index.
    fn file_path(&self, file: FileId) -> Arc<PathBuf> {
        let reg = self.files.lock().unwrap();
        reg.by_id
            .get(&file)
            .map(|f| f.path(self.as_source_db()))
            .unwrap_or_else(|| Arc::new(PathBuf::new()))
    }

    fn as_source_db(&self) -> &dyn arandu_middle::db::SourceDatabase {
        self
    }
}
