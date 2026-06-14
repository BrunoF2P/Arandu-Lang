/// A compact, 12-byte representation of a source code span.
/// Stores `file_id`, `start`, and `end` byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Creates a new `Span` with the given file identifier, start byte offset, and end byte offset.
    #[must_use]
    pub const fn new(file_id: u32, start: u32, end: u32) -> Self {
        Self { file_id, start, end }
    }
}
