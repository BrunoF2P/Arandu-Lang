use arandu_base::source_registry::SourceRegistry;
use arandu_diagnostics::Diagnostic;

use crate::{assert_golden_text, read_golden_text};

/// Asserts diagnostic UI output matches golden `.diag` file for a given UI phase folder (e.g. "ui/semantics" or "ui/type_checker").
pub fn assert_diagnostic_golden(ui_subfolder: &str, name: &str, diagnostics: &[Diagnostic]) {
    let source_rel_path = format!("tests/{ui_subfolder}/{name}.aru");
    let source = read_golden_text(ui_subfolder, name, "aru");

    let mut registry = SourceRegistry::default();
    registry.register(&source_rel_path, &source);

    let prefix_char = if ui_subfolder.contains("type_checker") {
        Some('T')
    } else {
        None
    };

    let actual = diagnostics
        .iter()
        .filter(|d| {
            if let Some(c) = prefix_char {
                format!("{}", d.code).starts_with(c)
            } else {
                true
            }
        })
        .map(|d| format!("{}\n", d.format_for_cli(&registry)))
        .collect::<String>();

    assert_golden_text(ui_subfolder, name, "diag", &actual);
}
