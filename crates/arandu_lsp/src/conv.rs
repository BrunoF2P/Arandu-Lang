//! Byte offset ↔ LSP Position using [`arandu_base::LineIndex`].
//!
//! LSP `character` is treated as **UTF-8 byte offset within the line** for now
//! (correct for ASCII / most Arandu sources). Full UTF-16 mapping can come later.

use arandu_base::{LineIndex, Span};
use lsp_types::{Position, Range};

/// 0-based LSP position from exclusive/inclusive byte offset into `text`.
#[must_use]
pub fn offset_to_position(index: &LineIndex, offset: u32) -> Position {
    let (line1, col1) = index.line_col(offset);
    Position {
        line: line1.saturating_sub(1),
        character: col1.saturating_sub(1),
    }
}

/// Byte offset from LSP position (clamped to text length).
#[must_use]
pub fn position_to_offset(index: &LineIndex, pos: Position, text: &str) -> u32 {
    let len = text.len() as u32;
    let line = pos.line as usize;
    let line_start = match index.line_starts.get(line) {
        Some(&s) => s,
        None => return len,
    };
    let line_end = index.line_starts.get(line + 1).copied().unwrap_or(len);
    // Exclude trailing `\n` from line end for character clamp.
    let mut end = line_end;
    if end > line_start {
        let last = text.as_bytes().get((end - 1) as usize).copied();
        if last == Some(b'\n') {
            end -= 1;
        }
    }
    let col = pos.character;
    let off = line_start.saturating_add(col);
    off.min(end).min(len)
}

/// Convert a compiler [`Span`] (byte offsets) to an LSP [`Range`].
/// End is exclusive in our spans.
#[must_use]
pub fn span_to_range(index: &LineIndex, span: Span) -> Range {
    let start = offset_to_position(index, span.start);
    let end_off = if span.end > span.start {
        span.end
    } else {
        span.start
    };
    let end = offset_to_position(index, end_off);
    Range { start, end }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arandu_base::LineIndex;

    #[test]
    fn roundtrip_ascii() {
        let text = "abc\ndef\n";
        let idx = LineIndex::new(text);
        assert_eq!(
            offset_to_position(&idx, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            offset_to_position(&idx, 4),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            position_to_offset(
                &idx,
                Position {
                    line: 1,
                    character: 1
                },
                text
            ),
            5
        );
        let r = span_to_range(&idx, Span::new(0, 4, 7)); // "def"
        assert_eq!(r.start.line, 1);
        assert_eq!(r.start.character, 0);
        assert_eq!(r.end.line, 1);
        assert_eq!(r.end.character, 3);
    }
}
