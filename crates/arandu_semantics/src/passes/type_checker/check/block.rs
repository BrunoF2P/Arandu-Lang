use arandu_parser::{Block, Stmt};

use super::super::TypeChecker;
use super::super::types::ArType;
use super::stmt::check_stmt;

pub fn check_block(checker: &mut TypeChecker, block: &Block) -> ArType {
    let mut last_ty = ArType::Void;
    let len = block.statements.len();
    for (i, stmt) in block.statements.iter().enumerate() {
        if i == len - 1 {
            if let Stmt::Expr { expr, .. } = stmt {
                last_ty = super::super::synth::synth_expr(checker, expr);
            } else {
                check_stmt(checker, stmt);
                last_ty = ArType::Void;
            }
        } else {
            check_stmt(checker, stmt);
        }
    }
    last_ty
}
