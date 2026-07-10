//! Token-based green hand-lower: build AST without recursive-descent.
//!
//! Designed for the CST-first pipeline: walk green `*_ITEM` / `STMT` / `BLOCK`
//! nodes and lower their token spans with a small cursor. Any construct that
//! cannot be recognized returns [`None`] so the green walk can fall back to RD.

mod cursor;
mod decl;
mod expr;
mod pattern;
mod stmt;
mod ty;

pub use cursor::{Cursor, HandCtx, token_span, token_text, tokens_in_range};
pub use decl::{
    try_hand_lower_func_item, try_hand_lower_import, try_hand_lower_module,
    try_hand_lower_top_level,
};
pub use expr::{try_hand_lower_expr, try_hand_lower_expr_all};
pub use stmt::{try_hand_lower_block, try_hand_lower_stmt};
pub use ty::try_hand_lower_type;

use super::SyntaxTree;
use super::kind::SyntaxKind;
use crate::ast::ast_pool::AstPool;

/// Count how many green STMTs under `tree` hand-lower without RD.
#[must_use]
pub fn count_hand_lowerable_stmts(tree: &SyntaxTree) -> (usize, usize) {
    let mut total = 0usize;
    let mut hand = 0usize;
    let mut pool = AstPool::new();
    for item in tree.items() {
        for n in item.descendants() {
            if n.kind() != SyntaxKind::STMT {
                continue;
            }
            total += 1;
            if try_hand_lower_stmt(&mut pool, tree.text(), tree.tokens(), &n, 0).is_some() {
                hand += 1;
            }
        }
    }
    (total, hand)
}

/// Count top-level `FUNC_ITEM`s that fully hand-lower (signature + body).
#[must_use]
pub fn count_hand_lowerable_funcs(tree: &SyntaxTree) -> (usize, usize) {
    let mut total = 0usize;
    let mut hand = 0usize;
    let mut pool = AstPool::new();
    for item in tree.items() {
        if item.kind() != SyntaxKind::FUNC_ITEM {
            continue;
        }
        total += 1;
        if try_hand_lower_func_item(&mut pool, tree.text(), tree.tokens(), &item, 0).is_some() {
            hand += 1;
        }
    }
    (total, hand)
}

/// Count typed top-level decls (excluding module/import) that hand-lower.
#[must_use]
pub fn count_hand_lowerable_decls(tree: &SyntaxTree) -> (usize, usize) {
    let mut total = 0usize;
    let mut hand = 0usize;
    let mut pool = AstPool::new();
    for item in tree.items() {
        if matches!(
            item.kind(),
            SyntaxKind::MODULE_ITEM | SyntaxKind::IMPORT_ITEM
        ) {
            continue;
        }
        if !item.kind().is_top_level_item() {
            continue;
        }
        total += 1;
        if try_hand_lower_top_level(&mut pool, tree.text(), tree.tokens(), &item, 0).is_some() {
            hand += 1;
        }
    }
    (total, hand)
}
