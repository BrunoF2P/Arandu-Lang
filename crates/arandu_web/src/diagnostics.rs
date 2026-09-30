//! Web diagnostic types and position calculation for the Arandu Web Playground.

use arandu_base::LineIndex;
use serde::{Deserialize, Serialize};

/// Diagnostic format consumed by the web editor (Monaco / CodeMirror).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDiagnostic {
    /// 1-indexed line number.
    pub line: u32,
    /// 1-indexed column number.
    pub column: u32,
    /// 1-indexed exclusive end position, suitable for Monaco markers.
    #[serde(rename = "endLine")]
    pub end_line: u32,
    #[serde(rename = "endColumn")]
    pub end_column: u32,
    /// Byte length of the primary diagnostic span (kept for API compatibility).
    pub length: u32,
    /// Compiler source-file identity for associating related labels.
    #[serde(rename = "fileId")]
    pub file_id: u32,
    /// Severity level.
    pub severity: WebSeverity,
    /// Canonical diagnostic code (e.g. "T001", "P005").
    pub code: Option<String>,
    /// Primary error or warning message.
    pub message: String,
    /// Short annotation attached to the primary span, when available.
    #[serde(rename = "primaryLabel")]
    pub primary_label: Option<String>,
    /// Secondary source annotations, including spans in other source files.
    pub labels: Vec<WebLabel>,
    /// Supplementary notes and help text.
    pub notes: Vec<String>,
}

/// A secondary annotated source span. Coordinates are present when the label
/// belongs to the source currently displayed by the playground; byte offsets
/// and `fileId` are retained for labels from other files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebLabel {
    pub file_id: u32,
    pub start: u32,
    pub end: u32,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    pub message: String,
}

/// Diagnostic severity for Monaco Editor markers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WebSeverity {
    Error,
    Warning,
    Info,
}

/// Convert an internal compiler diagnostic into a web-friendly diagnostic using `LineIndex`.
#[must_use]
pub fn convert_diagnostic(
    diag: &arandu_middle::Diagnostic,
    line_index: &LineIndex,
) -> WebDiagnostic {
    let span = diag.span;
    let (line, column) = line_index.line_col(span.start);
    let (end_line, end_column) = line_index.line_col(span.end);
    let length = span.end.saturating_sub(span.start).max(1);

    let severity = match diag.severity {
        arandu_middle::Severity::Error => WebSeverity::Error,
        arandu_middle::Severity::Warning => WebSeverity::Warning,
        arandu_middle::Severity::Note | arandu_middle::Severity::Hint => WebSeverity::Info,
    };

    let code = Some(diag.code.as_str().to_string());
    let mut notes = diag.notes.clone();
    for hint in &diag.hints {
        notes.push(format!("Hint: {}", hint.message));
    }
    let labels = diag
        .labels
        .iter()
        .map(|label| {
            let (line, column, end_line, end_column) = if label.span.file_id == span.file_id {
                let (line, column) = line_index.line_col(label.span.start);
                let (end_line, end_column) = line_index.line_col(label.span.end);
                (Some(line), Some(column), Some(end_line), Some(end_column))
            } else {
                (None, None, None, None)
            };
            WebLabel {
                file_id: label.span.file_id,
                start: label.span.start,
                end: label.span.end,
                line,
                column,
                end_line,
                end_column,
                message: label.message.clone(),
            }
        })
        .collect();

    WebDiagnostic {
        line,
        column,
        end_line,
        end_column,
        length,
        file_id: span.file_id,
        severity,
        code,
        message: diag.message.clone(),
        primary_label: diag.primary_label.as_deref().cloned(),
        labels,
        notes,
    }
}

/// Calculate 1-indexed line and column from a byte offset in UTF-8 source.
#[must_use]
pub fn offset_to_line_col(source: &str, byte_offset: usize) -> (u32, u32) {
    let index = LineIndex::new(source);
    index.line_col(byte_offset as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_to_line_col_at_start() {
        assert_eq!(offset_to_line_col("hello", 0), (1, 1));
    }

    #[test]
    fn offset_to_line_col_multiline() {
        let text = "func main(): void {\n    return\n}";
        assert_eq!(offset_to_line_col(text, 20), (2, 1));
        assert_eq!(offset_to_line_col(text, 24), (2, 5));
    }

    #[test]
    fn conversion_preserves_secondary_labels_and_utf16_ranges() {
        let diagnostic = arandu_middle::Diagnostic::error(
            arandu_middle::DiagCode::P001UnexpectedToken,
            "Unexpected token.",
            arandu_middle::Span::new(7, 5, 7),
        )
        .with_primary_label("unexpected token")
        .with_label(arandu_middle::Span::new(7, 0, 2), "declared here")
        .with_label(arandu_middle::Span::new(9, 3, 8), "in another file");
        let converted = convert_diagnostic(&diagnostic, &LineIndex::new("😀 bad\n"));

        assert_eq!((converted.line, converted.column), (1, 4));
        assert_eq!((converted.end_line, converted.end_column), (1, 6));
        assert_eq!(converted.labels.len(), 2);
        assert_eq!(converted.primary_label.as_deref(), Some("unexpected token"));
        assert_eq!(converted.labels[0].line, Some(1));
        assert_eq!(converted.labels[0].column, Some(1));
        assert_eq!(converted.labels[0].message, "declared here");
        assert_eq!(converted.labels[1].file_id, 9);
        assert_eq!(converted.labels[1].line, None);

        let json = serde_json::to_value(&converted).expect("diagnostic should serialize");
        assert_eq!(json["endLine"], 1);
        assert_eq!(json["endColumn"], 6);
        assert_eq!(json["fileId"], 7);
        assert_eq!(json["labels"][0]["message"], "declared here");
        assert_eq!(json["primaryLabel"], "unexpected token");
        assert_eq!(json["labels"][1]["line"], serde_json::Value::Null);
    }
}
