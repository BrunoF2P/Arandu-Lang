use rustc_hash::FxHashMap;
use std::fmt;
use std::marker::PhantomData;

/// A type-safe, compact StringId.
/// Wrapped around `u32` with a `PhantomData<*const str>` marker.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StringId {
    index: u32,
    _marker: PhantomData<*const str>,
}

impl StringId {
    /// Creates a new `StringId` from a raw index.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }

    /// Returns the raw `u32` index.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.index
    }

    /// Converts the `StringId` to `usize` for index mappings.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.index as usize
    }

    /// Constructs a `StringId` from `usize`.
    #[must_use]
    pub const fn from_usize(idx: usize) -> Self {
        Self::new(idx as u32)
    }
}

impl fmt::Debug for StringId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StringId({})", self.index)
    }
}

/// A Small String Optimization (SSO) string.
/// If the string is <= 23 bytes, it is stored inline without any heap allocation.
/// Otherwise, it falls back to a heap-allocated `String`.
#[derive(Clone)]
pub enum SsoString {
    Inline { len: u8, data: [u8; 23] },
    Heap(String),
}

impl std::hash::Hash for SsoString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq for SsoString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SsoString {}

impl std::borrow::Borrow<str> for SsoString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl SsoString {
    /// Creates a new `SsoString` from a string slice.
    #[must_use]
    pub fn new(s: &str) -> Self {
        let len = s.len();
        if len <= 23 {
            let mut data = [0u8; 23];
            data[..len].copy_from_slice(s.as_bytes());
            Self::Inline {
                len: len as u8,
                data,
            }
        } else {
            Self::Heap(s.to_string())
        }
    }

    /// Accesses the underlying string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Inline { len, data } => {
                let len = *len as usize;
                // Safe because we copy from a valid &str in new()
                unsafe { std::str::from_utf8_unchecked(&data[..len]) }
            }
            Self::Heap(s) => s.as_str(),
        }
    }
}

impl fmt::Debug for SsoString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for SsoString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl AsRef<str> for SsoString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<str> for SsoString {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<SsoString> for str {
    fn eq(&self, other: &SsoString) -> bool {
        self == other.as_str()
    }
}

/// A deduplicating String Interner employing `SsoString` (Small String Optimization)
/// to keep the memory footprint low and comparison times at O(1).
#[derive(Debug, Clone)]
pub struct StringPool {
    map: FxHashMap<SsoString, StringId>,
    strings: Vec<SsoString>,
}

impl StringPool {
    /// Creates an empty `StringPool`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            strings: Vec::new(),
        }
    }

    /// Interns a string slice, returning its canonical `StringId`.
    pub fn intern(&mut self, s: &str) -> StringId {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let sso = SsoString::new(s);
        let id = StringId::from_usize(self.strings.len());
        self.map.insert(sso.clone(), id);
        self.strings.push(sso);
        id
    }

    /// Resolves a `StringId` back to its string slice.
    #[must_use]
    pub fn resolve(&self, id: StringId) -> &str {
        self.strings[id.as_usize()].as_str()
    }

    /// Number of unique strings interned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Returns `true` if the pool is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

impl Default for StringPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sso_inline_and_heap() {
        let s1 = SsoString::new("short");
        assert!(matches!(s1, SsoString::Inline { .. }));
        assert_eq!(s1.as_str(), "short");

        let s2 = SsoString::new("this string is definitely longer than twenty-three bytes");
        assert!(matches!(s2, SsoString::Heap(_)));
        assert_eq!(
            s2.as_str(),
            "this string is definitely longer than twenty-three bytes"
        );
    }

    #[test]
    fn test_string_pool_intern_resolve() {
        let mut pool = StringPool::new();
        let id1 = pool.intern("hello");
        let id2 = pool.intern("world");
        let id3 = pool.intern("hello");

        assert_eq!(id1, id3);
        assert_ne!(id1, id2);

        assert_eq!(pool.resolve(id1), "hello");
        assert_eq!(pool.resolve(id2), "world");
    }
}
