//! F1b — lower helpers over structured green trees.
//!
//! Full AST construction still uses recursive-descent on the CST token stream
//! ([`super::lower_syntax_to_program`]). This module walks green structure for
//! validation, metrics, and future per-node lower.

use super::SyntaxTree;
use super::kind::{SyntaxKind, SyntaxNode};

/// Summary of structured green content (no heap AST).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GreenStructure {
    pub top_level_items: usize,
    pub func_items: usize,
    pub struct_items: usize,
    pub blocks: usize,
    pub typed_items: usize,
}

/// Walk the CST and count structured nodes (F1a readiness).
#[must_use]
pub fn inspect_green_structure(tree: &SyntaxTree) -> GreenStructure {
    let mut s = GreenStructure::default();
    for item in tree.items() {
        s.top_level_items += 1;
        if item.kind() != SyntaxKind::ITEM {
            s.typed_items += 1;
        }
        match item.kind() {
            SyntaxKind::FUNC_ITEM => s.func_items += 1,
            SyntaxKind::STRUCT_ITEM => s.struct_items += 1,
            _ => {}
        }
        s.blocks += item
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::BLOCK)
            .count();
    }
    s
}

/// True when every top-level child is a typed `*_ITEM` (not generic `ITEM`).
#[must_use]
pub fn is_fully_typed_toplevel(tree: &SyntaxTree) -> bool {
    let items = tree.items();
    !items.is_empty() && items.iter().all(|n| n.kind() != SyntaxKind::ITEM)
}

/// First `FUNC_ITEM` node, if any.
#[must_use]
pub fn first_func_item(tree: &SyntaxTree) -> Option<SyntaxNode> {
    tree.items()
        .into_iter()
        .find(|n| n.kind() == SyntaxKind::FUNC_ITEM)
}

/// Body `BLOCK` of a function item node.
#[must_use]
pub fn func_body_block(func: &SyntaxNode) -> Option<SyntaxNode> {
    func.children().find(|n| n.kind() == SyntaxKind::BLOCK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parse_syntax;

    #[test]
    fn structured_func_has_block() {
        let tree = parse_syntax("func main(): int {\n    return 1\n}\n");
        let s = inspect_green_structure(&tree);
        assert_eq!(s.func_items, 1);
        assert_eq!(s.blocks, 1);
        assert!(is_fully_typed_toplevel(&tree));
        let f = first_func_item(&tree).expect("func");
        assert!(func_body_block(&f).is_some());
    }

    #[test]
    fn two_funcs_two_blocks() {
        let src = "func a(): int { return 1 }\nfunc b(): int { return 2 }\n";
        let tree = parse_syntax(src);
        let s = inspect_green_structure(&tree);
        assert_eq!(s.func_items, 2);
        assert_eq!(s.blocks, 2);
    }

    #[test]
    fn struct_item_has_block() {
        let tree = parse_syntax("struct Point {\n    x: int\n}\n");
        let s = inspect_green_structure(&tree);
        assert_eq!(s.struct_items, 1);
        assert!(s.blocks >= 1);
    }
}
