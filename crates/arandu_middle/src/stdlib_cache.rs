use std::collections::HashMap;
use std::path::PathBuf;

/// Memoized stdlib file-system path resolution.
///
/// `get_or_resolve` calls the fallible `find_stdlib_path_uncached` at most once
/// per logical module name.  A cached `None` means "already proved absent,
/// don't stat the filesystem again".
///
/// Lives inside [`CompileSession`](crate::session::CompileSession) alongside
/// [`ParseCache`](crate::parse_cache::ParseCache) — same memoization pattern,
/// same lifecycle.
#[derive(Debug, Default)]
pub struct StdlibPathCache {
    entries: HashMap<String, Option<PathBuf>>,
}

impl StdlibPathCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached path for `module`, or resolves it via
    /// `find_stdlib_path_uncached`, stores the result, and returns it.
    pub fn get_or_resolve(&mut self, module: &str) -> Option<PathBuf> {
        if let Some(entry) = self.entries.get(module) {
            return entry.clone();
        }
        let resolved = find_stdlib_path_uncached(module);
        self.entries.insert(module.to_string(), resolved.clone());
        resolved
    }
}

/// Walk upward from `current_dir()` until `module` exists as a relative path.
///
/// This is the uncached, I/O-heavy fallback — prefer
/// [`StdlibPathCache::get_or_resolve`] instead.
pub fn find_stdlib_path_uncached(relative: &str) -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}
