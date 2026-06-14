use fxhash::FxHashMap;
use crate::line_index::LineIndex;

/// An entry in the `SourceRegistry` representing a loaded source file.
// TODO: Refactor to hold &'sess str session references instead of owned String to avoid memory duplication.
pub struct SourceFile {
    pub path: String,
    pub source: String,
    pub line_index: LineIndex,
}

/// A registry mapping file identifiers to file paths, contents, and line indices.
#[derive(Default)]
pub struct SourceRegistry {
    files: Vec<SourceFile>,
    path_to_id: FxHashMap<String, u32>,
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
        self.files.push(SourceFile {
            path: path.to_string(),
            source: source.to_string(),
            line_index,
        });
        self.path_to_id.insert(path.to_string(), id);
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
