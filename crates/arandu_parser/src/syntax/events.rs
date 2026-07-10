//! Parse events for building a green tree from recursive-descent (F1 event sink).
//!
//! The RD parser emits [`ParseEvent`]s; [`build_green_from_events`] turns them into
//! a rowan green tree in the same pass as AST construction.

use super::kind::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder};

/// Events produced by the RD parser while consuming tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseEvent {
    /// Start a composite node (must be paired with [`Self::Finish`]).
    Start(SyntaxKind),
    /// A leaf token covering `source[start..end]` (end may equal start for ASI `;`).
    Token {
        kind: SyntaxKind,
        start: u32,
        end: u32,
    },
    /// Finish the current composite node.
    Finish,
}

/// Build a green tree from a balanced event stream.
///
/// Inserts [`SyntaxKind::WHITESPACE`] for gaps between token starts so the green
/// text covers the full `source` (lexer does not emit whitespace tokens).
///
/// Returns `None` if Start/Finish are unbalanced or the builder cannot finish
/// (caller should fall back to heuristic green).
#[must_use]
pub fn build_green_from_events(source: &str, events: &[ParseEvent]) -> Option<GreenNode> {
    if events.is_empty() {
        return None;
    }
    let mut builder = GreenNodeBuilder::new();
    let mut depth = 0i32;
    let mut cursor = 0u32;
    let src_len = source.len() as u32;

    for ev in events {
        match *ev {
            ParseEvent::Start(kind) => {
                builder.start_node(kind.into());
                depth += 1;
            }
            ParseEvent::Token { kind, start, end } => {
                let start = start.min(src_len);
                let end = end.min(src_len).max(start);
                // Gap whitespace (spaces, newlines, comments are real tokens).
                if start > cursor {
                    let s = cursor as usize;
                    let e = start as usize;
                    builder.token(SyntaxKind::WHITESPACE.into(), &source[s..e]);
                }
                if end > start {
                    builder.token(kind.into(), &source[start as usize..end as usize]);
                    cursor = end;
                } else {
                    // Zero-width (ASI `;`): keep cursor so following gap stays correct.
                    cursor = cursor.max(start);
                }
            }
            ParseEvent::Finish => {
                if depth == 0 {
                    return None;
                }
                // Trailing whitespace before closing the root.
                if depth == 1 && cursor < src_len {
                    builder.token(
                        SyntaxKind::WHITESPACE.into(),
                        &source[cursor as usize..source.len()],
                    );
                    cursor = src_len;
                }
                builder.finish_node();
                depth -= 1;
            }
        }
    }
    if depth != 0 {
        return None;
    }
    // finish() requires exactly one root node on the stack.
    Some(builder.finish())
}

/// True if Start/Finish counts match (quick sanity check).
#[must_use]
pub fn events_balanced(events: &[ParseEvent]) -> bool {
    let mut depth = 0i32;
    for ev in events {
        match ev {
            ParseEvent::Start(_) => depth += 1,
            ParseEvent::Finish => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            ParseEvent::Token { .. } => {}
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_source_file_events() {
        let src = "x";
        let events = [
            ParseEvent::Start(SyntaxKind::SOURCE_FILE),
            ParseEvent::Token {
                kind: SyntaxKind::IDENT,
                start: 0,
                end: 1,
            },
            ParseEvent::Finish,
        ];
        let green = build_green_from_events(src, &events).expect("green");
        assert_eq!(green.to_string(), "x");
    }

    #[test]
    fn gaps_become_whitespace() {
        let src = "a b";
        let events = [
            ParseEvent::Start(SyntaxKind::SOURCE_FILE),
            ParseEvent::Token {
                kind: SyntaxKind::IDENT,
                start: 0,
                end: 1,
            },
            ParseEvent::Token {
                kind: SyntaxKind::IDENT,
                start: 2,
                end: 3,
            },
            ParseEvent::Finish,
        ];
        let green = build_green_from_events(src, &events).expect("green");
        assert_eq!(green.to_string(), "a b");
    }
}
