use std::collections::HashMap;
use std::collections::hash_map;
use std::path::{Path, PathBuf};

/// Memoized parse results keyed by file path.
///
/// Once the name resolver parses a stdlib file, the type checker reuses the
/// cached AST instead of re-parsing from disk — reducing `parse_with_file_id`
/// calls from 11 to 6 for a typical single-file build.
///
/// In a future Salsa-based incremental engine this cache will be absorbed into
/// a `salsa::Database` where `parse(path) -> &Program` becomes a memoized
/// query with automatic dependency tracking.
#[derive(Debug, Default)]
pub struct ParseCache {
    entries: HashMap<PathBuf, arandu_parser::Program>,
}

impl ParseCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached [`arandu_parser::Program`] for `path`, or parses
    /// `source`, stores the result, and returns a reference to it.
    pub fn get_or_parse(
        &mut self,
        path: &Path,
        source: &str,
    ) -> Result<&arandu_parser::Program, arandu_parser::ParseError> {
        let entry = self.entries.entry(path.to_path_buf());
        match entry {
            hash_map::Entry::Occupied(e) => Ok(e.into_mut()),
            hash_map::Entry::Vacant(e) => {
                let program = arandu_parser::parse(source)?;
                Ok(e.insert(program))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn get_or_parse_returns_cached() {
        let mut cache = ParseCache::new();
        let path = Path::new("test.aru");
        let source = "func main() {}";

        let first_addr = cache.get_or_parse(path, source).unwrap() as *const _;
        let second_addr = cache.get_or_parse(path, source).unwrap() as *const _;
        assert_eq!(first_addr, second_addr);
    }

    #[test]
    fn get_or_parse_different_paths() {
        let mut cache = ParseCache::new();
        let a_addr = cache.get_or_parse(Path::new("a.aru"), "func foo() {}").unwrap() as *const _;
        let b_addr = cache.get_or_parse(Path::new("b.aru"), "func bar() {}").unwrap() as *const _;
        assert_ne!(a_addr, b_addr);
    }

    #[test]
    fn get_or_parse_returns_error() {
        let mut cache = ParseCache::new();
        let result = cache.get_or_parse(Path::new("bad.aru"), "invalid syntax !!!");
        assert!(result.is_err());
    }
}
