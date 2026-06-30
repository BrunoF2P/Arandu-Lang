use arandu_parser::ast_pool::AstPool;
use arandu_parser::{Block, Stmt};

use super::super::TypeChecker;
use super::super::types::ArType;
use super::stmt::check_stmt;

pub fn check_block(checker: &mut TypeChecker<'_>, pool: &AstPool, block: &Block) -> ArType {
    let mut last_ty = ArType::Void;
    let len = block.statements.len();
    for (i, stmt) in block.statements.iter().enumerate() {
        let stmt = pool.stmt(*stmt);
        if i == len - 1 {
            if let Stmt::Expr { expr, .. } = stmt {
                let last_ty_id = super::super::synth::synth_expr(checker, *expr);
                last_ty = checker.resolve(last_ty_id).clone();
            } else {
                check_stmt(checker, pool, stmt);
                last_ty = ArType::Void;
            }
        } else {
            check_stmt(checker, pool, stmt);
        }
    }
    last_ty
}
