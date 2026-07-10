//! Arandu source formatter (F3a MVP).
//!
//! Pure text/CST hygiene — does **not** depend on Salsa or LSP.
//! On unparseable input, returns the original source unchanged.

use arandu_parser::{lower_syntax_to_program, parse_syntax};

/// UTF-8 byte range edit (for LSP full-document replace helpers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

/// Format source. MVP rules:
/// - Normalize line endings to `\n`
/// - Strip trailing whitespace per line
/// - Collapse 3+ blank lines to a single blank line
/// - Ensure exactly one trailing newline
/// - If the file does not parse cleanly, still apply whitespace rules (safe)
/// - If parse recovers with only lex noise, prefer formatted text as above
#[must_use]
pub fn format_source(source: &str) -> String {
    let normalized = normalize_whitespace(source);
    // Validate that we didn't destroy structure: if original parsed and new doesn't,
    // fall back to original (minus we still want whitespace fix if both parse or both fail).
    let orig_ok = parses_clean(source);
    let new_ok = parses_clean(&normalized);
    if orig_ok && !new_ok {
        return ensure_trailing_newline(source.replace("\r\n", "\n").replace('\r', "\n"));
    }
    normalized
}

/// Full-document replace if formatting changes the buffer.
#[must_use]
pub fn format_edits(source: &str) -> Vec<TextEdit> {
    let formatted = format_source(source);
    if formatted == source {
        return Vec::new();
    }
    vec![TextEdit {
        start: 0,
        end: source.len() as u32,
        new_text: formatted,
    }]
}

fn parses_clean(source: &str) -> bool {
    let tree = parse_syntax(source);
    lower_syntax_to_program(&tree, 0).is_ok() && tree.lex_diagnostics().is_empty()
}

fn ensure_trailing_newline(s: String) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s
    } else {
        let mut o = s;
        o.push('\n');
        o
    }
}

fn normalize_whitespace(source: &str) -> String {
    let unified = source.replace("\r\n", "\n").replace('\r', "\n");
    // `split_inclusive` keeps terminators; handle last line without double `\n`.
    let lines: Vec<&str> = if unified.is_empty() {
        Vec::new()
    } else {
        unified
            .strip_suffix('\n')
            .unwrap_or(&unified)
            .split('\n')
            .collect()
    };
    let mut out = String::with_capacity(unified.len() + 1);
    let mut blank_run = 0u32;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    if out.is_empty() {
        out.push('\n');
    }
    // Strip leading blank lines (keep at most none at start).
    while out.starts_with('\n') && out.len() > 1 {
        out.remove(0);
    }
    out
}

/// Quick-fix style actions (F3b MVP) from parse/type diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAction {
    pub title: &'static str,
    pub edits: Vec<TextEdit>,
}

/// Suggest inserting `;` when a diagnostic looks like a missing statement terminator.
#[must_use]
pub fn actions_for_expected_semicolon(start: u32, end: u32, message: &str) -> Option<CodeAction> {
    let msg = message.to_ascii_lowercase();
    if !(msg.contains("semicolon") || msg.contains("statement terminator") || msg.contains("semi"))
    {
        return None;
    }
    Some(CodeAction {
        title: "Insert `;`",
        edits: vec![TextEdit {
            start,
            end: start.min(end),
            new_text: ";".into(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_ws_and_extra_blanks() {
        let src = "func main(): int { return 1 }\n  \n\n\n";
        let out = format_source(src);
        assert!(out.ends_with('\n'));
        assert!(!out.contains("  \n"));
        assert!(!out.contains("\n\n\n"));
        assert!(parses_clean(&out));
    }

    #[test]
    fn format_edits_empty_when_stable() {
        let src = "func main(): int { return 1 }\n";
        let edits = format_edits(src);
        assert!(edits.is_empty() || edits[0].new_text == src);
    }

    #[test]
    fn semicolon_action() {
        let a = actions_for_expected_semicolon(10, 11, "expected SEMICOLON").unwrap();
        assert_eq!(a.edits[0].new_text, ";");
    }
}
