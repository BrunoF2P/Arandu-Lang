//! Lossless-ish CST via [`rowan`] (P5).
//!
//! Dual with the existing AST: [`parse_dual`] builds both. Green interning reuses
//! identical ITEM subtrees across reparses when item text is unchanged.

mod build;
mod kind;

pub use build::{
    SyntaxTree, apply_text_edit, parse_syntax, parse_syntax_with_item_spans, reparse_edit,
    text_range,
};
pub use kind::{AranduLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::{ParseError, Program, parse_with_file_id};

/// Dual parse: AST (existing path) + CST (rowan green tree with ITEM nodes).
pub fn parse_dual(source: &str) -> (Result<Program, ParseError>, SyntaxTree) {
    parse_dual_with_file_id(source, 0)
}

/// Dual parse with file id for spans.
pub fn parse_dual_with_file_id(
    source: &str,
    file_id: u32,
) -> (Result<Program, ParseError>, SyntaxTree) {
    let ast = parse_with_file_id(source, file_id);
    let spans: Vec<(u32, u32)> = match &ast {
        Ok(program) => program
            .decls
            .iter()
            .map(|id| {
                let decl = program.pool.decl(*id);
                let sp = decl.span();
                (sp.start, sp.end)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let tree = if spans.is_empty() {
        parse_syntax(source)
    } else {
        parse_syntax_with_item_spans(source, &spans)
    };
    (ast, tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_funcs(beta: i32) -> String {
        format!(
            "func alpha(): int {{\n    return 1\n}}\n\nfunc beta(): int {{\n    return {beta}\n}}\n"
        )
    }

    #[test]
    fn dual_builds_item_nodes() {
        let src = two_funcs(2);
        let (ast, tree) = parse_dual(&src);
        assert!(ast.is_ok(), "ast: {ast:?}");
        let items = tree.items();
        assert!(
            items.len() >= 2,
            "expected ≥2 ITEM nodes, got {}",
            items.len()
        );
        let texts = tree.item_texts();
        assert!(texts[0].contains("alpha"), "first item: {}", texts[0]);
        assert!(texts[1].contains("beta"), "second item: {}", texts[1]);
    }

    #[test]
    fn reparse_preserves_unchanged_item_text() {
        let src1 = two_funcs(2);
        let (_, t1) = parse_dual(&src1);
        let texts1 = t1.item_texts();
        assert!(texts1.len() >= 2);

        // Edit only beta's return constant (find "return 2").
        let needle = "return 2";
        let start = src1.find(needle).expect("needle") as u32;
        let end = start + needle.len() as u32;
        let (src2, _flat) = reparse_edit(&src1, start, end, "return 99", &[]);
        // Without spans, items may be flat — re-dual with AST:
        let (_, t2) = parse_dual(&src2);
        let texts2 = t2.item_texts();
        assert!(texts2.len() >= 2);
        // Alpha item text must be identical (green reuse candidate).
        assert_eq!(
            texts1[0], texts2[0],
            "alpha item text must survive beta-only edit"
        );
        assert_ne!(texts1[1], texts2[1], "beta item text must change");
        let _ = t2;
    }

    #[test]
    fn flat_syntax_covers_full_source() {
        let src = "func main(): int { return 1 }\n";
        let tree = parse_syntax(src);
        assert_eq!(tree.root().text().to_string(), src);
    }
}
