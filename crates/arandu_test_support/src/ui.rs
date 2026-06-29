use crate::assert_golden_text;

/// Asserts diagnostic UI output matches golden `.diag` file.
pub fn assert_diagnostic_golden(phase: &str, name: &str, actual_diag: &str) {
    assert_golden_text(phase, name, "diag", actual_diag);
}
