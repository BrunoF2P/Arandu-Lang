/// A map of line start byte offsets in a source file.
/// Used to perform binary search lookups of line and column numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    pub line_starts: Vec<u32>,
}

impl LineIndex {
    /// Constructs a `LineIndex` from the given source file contents.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (offset, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push((offset + 1) as u32);
            }
        }
        Self { line_starts }
    }

    /// Converts a byte offset into 1-based (line, column) numbers.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = self
            .line_starts
            .partition_point(|&s| s <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line] + 1;
        ((line + 1) as u32, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_lookups() {
        let source = "abc\ndef\nghi";
        let index = LineIndex::new(source);

        // First line: "abc\n" (offsets 0..=3)
        assert_eq!(index.line_col(0), (1, 1));
        assert_eq!(index.line_col(1), (1, 2));
        assert_eq!(index.line_col(2), (1, 3));
        assert_eq!(index.line_col(3), (1, 4));

        // Second line: "def\n" (offsets 4..=7)
        assert_eq!(index.line_col(4), (2, 1));
        assert_eq!(index.line_col(7), (2, 4));

        // Third line: "ghi" (offsets 8..=10)
        assert_eq!(index.line_col(8), (3, 1));
        assert_eq!(index.line_col(10), (3, 3));

        // Out of bounds: should saturate/cap nicely
        assert_eq!(index.line_col(20), (3, 13));
    }
}
