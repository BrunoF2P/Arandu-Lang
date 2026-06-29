use std::sync::Arc;
use crate::line_index::LineIndex;
use rustc_hash::FxHashMap;

/// An entry in the `SourceRegistry` representing a loaded source file.
/// Uses `Arc<str>` for zero-copy memory interning and lock-free thread safety across compilation sessions.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: Arc<str>,
    pub source: Arc<str>,
    pub line_index: LineIndex,
}

/// A registry mapping file identifiers to file paths, contents, and line indices.
#[derive(Default, Clone, Debug)]
pub struct SourceRegistry {
    files: Vec<SourceFile>,
    path_to_id: FxHashMap<Arc<str>, u32>,
}

impl SourceRegistry {
    /// Creates a new `SourceRegistry`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            path_to_id: FxHashMap::default(),
        }
    }

    /// Registers a file path and content, returning a unique file identifier.
    /// If the path is already registered, returns the existing identifier.
    pub fn register(&mut self, path: &str, source: &str) -> u32 {
        if let Some(&id) = self.path_to_id.get(path) {
            return id;
        }
        let id = self.files.len() as u32;
        let line_index = LineIndex::new(source);
        let path_arc: Arc<str> = Arc::from(path);
        let source_arc: Arc<str> = Arc::from(source);
        self.files.push(SourceFile {
            path: path_arc.clone(),
            source: source_arc,
            line_index,
        });
        self.path_to_id.insert(path_arc, id);
        id
    }

    /// Resolves a file identifier back to its `SourceFile`.
    #[must_use]
    pub fn get_file(&self, id: u32) -> Option<&SourceFile> {
        self.files.get(id as usize)
    }

    /// Resolves a file path back to its identifier.
    #[must_use]
    pub fn get_id_by_path(&self, path: &str) -> Option<u32> {
        self.path_to_id.get(path).copied()
    }
}
