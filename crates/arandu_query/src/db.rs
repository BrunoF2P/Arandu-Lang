use std::path::PathBuf;
use std::sync::Arc;
pub type FileId = u32;

pub trait StableHash {
    fn stable_hash(&self) -> blake3::Hash;
}

impl<T: std::fmt::Debug> StableHash for T {
    fn stable_hash(&self) -> blake3::Hash {
        blake3::hash(format!("{:?}", self).as_bytes())
    }
}

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

#[derive(Default, Clone)]
#[salsa::db]
pub struct DatabaseImpl {
    storage: salsa::Storage<Self>,
    module_files: Arc<std::sync::Mutex<std::collections::HashMap<String, SourceFile>>>,
}

#[salsa::db]
impl salsa::Database for DatabaseImpl {}

impl DatabaseImpl {
    pub fn new_file(&mut self, path: String, text: String) -> SourceFile {
        let mut cache = self.module_files.lock().unwrap();
        let file_id = cache.len() as u32 + 100;
        let file = SourceFile::new(
            self,
            file_id,
            Arc::from(text),
            Arc::new(std::path::PathBuf::from(&path)),
        );
        cache.insert(path, file);
        file
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
        // Fast path: check cache
        {
            let cache = self.module_files.lock().unwrap();
            if let Some(file) = cache.get(path) {
                return Some(*file);
            }
        }

        // Uncached path resolution
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

        let mut cache = self.module_files.lock().unwrap();
        // Check again in case another thread inserted it while we were reading
        if let Some(file) = cache.get(path) {
            return Some(*file);
        }

        // Generate a new unique FileId using the number of items in the cache + 100
        // (starting at 100 to avoid colliding with small file IDs used in tests or prelude)
        let file_id = cache.len() as u32 + 100;
        let file = SourceFile::new(self, file_id, Arc::from(text), Arc::new(found_path));
        cache.insert(path.to_string(), file);

        Some(file)
    }
}

#[salsa::db]
impl ArandCompilerDb for DatabaseImpl {
    fn source_text(&self, _file: FileId) -> Arc<str> {
        Arc::from("")
    }
    fn file_path(&self, _file: FileId) -> Arc<PathBuf> {
        Arc::new(PathBuf::new())
    }
    fn as_source_db(&self) -> &dyn arandu_middle::db::SourceDatabase {
        self
    }
}
