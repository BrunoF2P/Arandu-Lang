//! CST-first pipeline via [`rowan`] (P5 complete).
//!
//! 1. Lex → green tree with top-level [`SyntaxKind::ITEM`] nodes (heuristic).
//! 2. Lower CST text → AST [`Program`] for resolve/typeck.
//! 3. Subtree reparse via [`reparse_subtree`] when an edit stays inside one ITEM
//!    (re-lex only that ITEM; sibling green nodes reused via `replace_child`).

mod build;
mod kind;

pub use build::{
    SyntaxTree, apply_text_edit, find_top_level_item_spans, for_each_highlight_token,
    highlight_spans, lower_syntax_to_program, lower_syntax_to_program_recovering, parse_syntax,
    parse_syntax_with_item_spans, reparse_edit, reparse_subtree, text_range,
};
pub use kind::{AranduLanguage, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::{ParseError, ParseOutput, Program};

/// Single entry: CST-first then lower to AST (no independent dual parse).
pub fn parse_from_cst(source: &str) -> Result<Program, ParseError> {
    parse_from_cst_with_file_id(source, 0)
}

/// CST-first parse with file id for spans.
pub fn parse_from_cst_with_file_id(source: &str, file_id: u32) -> Result<Program, ParseError> {
    let tree = parse_syntax(source);
    lower_syntax_to_program(&tree, file_id)
}

/// Recovering CST-first pipeline.
#[must_use]
pub fn parse_from_cst_recovering(source: &str, file_id: u32) -> (SyntaxTree, ParseOutput) {
    let tree = parse_syntax(source);
    let output = lower_syntax_to_program_recovering(&tree, file_id);
    (tree, output)
}

/// CST + AST together (same path as [`parse_from_cst`]; name kept for call sites).
pub fn parse_dual(source: &str) -> (Result<Program, ParseError>, SyntaxTree) {
    let tree = parse_syntax(source);
    let ast = lower_syntax_to_program(&tree, 0);
    (ast, tree)
}

pub fn parse_dual_with_file_id(
    source: &str,
    file_id: u32,
) -> (Result<Program, ParseError>, SyntaxTree) {
    let tree = parse_syntax(source);
    let ast = lower_syntax_to_program(&tree, file_id);
    (ast, tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Multiline bodies so lexer ASI inserts `;` before `}` (newline-driven).
    fn two_funcs(beta: i32) -> String {
        format!(
            "func alpha(): int {{\n    return 1\n}}\n\nfunc beta(): int {{\n    return {beta}\n}}\n"
        )
    }

    fn main_func() -> &'static str {
        "func main(): int {\n    return 1\n}\n"
    }

    #[test]
    fn cst_first_builds_item_nodes_without_ast() {
        let src = two_funcs(2);
        let tree = parse_syntax(&src);
        let items = tree.items();
        assert!(
            items.len() >= 2,
            "expected ≥2 ITEM nodes from heuristic, got {}",
            items.len()
        );
        let texts = tree.item_texts();
        assert!(texts[0].contains("alpha"), "first: {}", texts[0]);
        assert!(texts.iter().any(|t| t.contains("beta")), "beta missing");
    }

    #[test]
    fn lower_from_cst_matches_program() {
        let src = two_funcs(2);
        let tree = parse_syntax(&src);
        let prog = lower_syntax_to_program(&tree, 0).expect("lower");
        assert!(prog.decls.len() >= 2);
    }

    #[test]
    fn lower_flat_main_ok() {
        let src = main_func();
        let tree = parse_syntax(src);
        let prog = lower_syntax_to_program(&tree, 100).expect("lower");
        assert!(!prog.decls.is_empty());
        assert!(crate::parse(src).is_ok());
    }

    #[test]
    fn subtree_reparse_preserves_sibling_item_text() {
        let src1 = two_funcs(2);
        let t1 = parse_syntax(&src1);
        let texts1 = t1.item_texts();
        assert!(texts1.len() >= 2);

        let needle = "return 2";
        let start = src1.find(needle).expect("needle") as u32;
        let end = start + needle.len() as u32;
        let (src2, t2) = reparse_subtree(&t1, start, end, "return 99");
        assert!(src2.contains("return 99"));
        let texts2 = t2.item_texts();
        assert!(texts2.len() >= 2);
        assert_eq!(
            texts1[0].trim(),
            texts2[0].trim(),
            "alpha ITEM text must survive beta-only subtree reparse"
        );
        assert_ne!(texts1[1].trim(), texts2[1].trim());
    }

    #[test]
    fn subtree_reparse_reuses_sibling_green_identity() {
        let src1 = two_funcs(2);
        let t1 = parse_syntax(&src1);
        let alpha1 = t1.items()[0].green().into_owned();

        let needle = "return 2";
        let start = src1.find(needle).expect("needle") as u32;
        let end = start + needle.len() as u32;
        let (_src2, t2) = reparse_subtree(&t1, start, end, "return 99");
        let alpha2 = t2.items()[0].green().into_owned();

        // replace_child clones Arc for untouched children → same green identity.
        let p1: *const rowan::GreenNodeData = &*alpha1;
        let p2: *const rowan::GreenNodeData = &*alpha2;
        assert_eq!(
            p1, p2,
            "unedited ITEM green must be reused (pointer identity)"
        );
    }

    #[test]
    fn flat_syntax_covers_full_source() {
        let src = main_func();
        let tree = parse_syntax(src);
        assert_eq!(tree.root().text().to_string(), src);
    }

    #[test]
    fn highlight_spans_mark_keywords() {
        let tree = parse_syntax(main_func());
        let spans = highlight_spans(&tree);
        assert!(
            spans.iter().any(|(_, _, c)| *c == "keyword"),
            "expected keyword highlights: {spans:?}"
        );
    }
}
