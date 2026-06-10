use arandu_parser::ast_pool::{AstPool, ExprId, ExprKind};
use arandu_parser::{
    Block, CatchHandler, Condition, DeferBody, ForClause, LambdaBody, MatchArmBody, ResultType,
    SimpleStmt, Stmt, TopLevelDecl, TypeExpr,
};

use super::super::TypeChecker;
use arandu_lexer::Span;

const ANY_ERROR_MESSAGE: &str =
    "any is only allowed in variadic parameters, extern declarations, and compiler builtins";

pub(crate) fn contains_any(ty: &TypeExpr) -> Option<Span> {
    match ty {
        TypeExpr::Primitive { span, name } => {
            if name == "any" {
                Some(*span)
            } else {
                None
            }
        }
        TypeExpr::Named { args, .. } => {
            for arg in args {
                if let Some(span) = contains_any(arg) {
                    return Some(span);
                }
            }
            None
        }
        TypeExpr::Nullable { inner, .. }
        | TypeExpr::Pointer { inner, .. }
        | TypeExpr::Slice { inner, .. }
        | TypeExpr::Array { elem: inner, .. }
        | TypeExpr::Group { inner, .. } => contains_any(inner),
        TypeExpr::Func { params, result, .. } => {
            for param in params {
                if let Some(span) = contains_any(param) {
                    return Some(span);
                }
            }
            if let Some(res) = result {
                match &**res {
                    ResultType::Single { ty, .. } => {
                        if let Some(span) = contains_any(ty) {
                            return Some(span);
                        }
                    }
                    ResultType::Multi { types, .. } => {
                        for ty in types {
                            if let Some(span) = contains_any(ty) {
                                return Some(span);
                            }
                        }
                    }
                }
            }
            None
        }
    }
}

#[cold]
#[inline(never)]
fn report_any_error(checker: &mut TypeChecker<'_>, span: Span) {
    checker.diagnostics.push(crate::Diagnostic::error(
        crate::DiagCode::T014InvalidVariadicType,
        ANY_ERROR_MESSAGE,
        span,
    ));
}

fn validate_type_no_any(checker: &mut TypeChecker<'_>, ty: &TypeExpr) {
    if let Some(span) = contains_any(ty) {
        report_any_error(checker, span);
    }
}

fn validate_result_type_no_any(checker: &mut TypeChecker<'_>, result: &ResultType) {
    match result {
        ResultType::Single { ty, .. } => validate_type_no_any(checker, ty),
        ResultType::Multi { types, .. } => {
            for ty in types {
                validate_type_no_any(checker, ty);
            }
        }
    }
}

fn validate_expr(checker: &mut TypeChecker<'_>, expr: ExprId) {
    match checker.pool.expr(expr) {
        ExprKind::Generic { callee, args } => {
            validate_expr(checker, *callee);
            let arg_ids = checker.pool.type_expr_list(*args).to_vec();
            for arg_id in arg_ids {
                validate_type_no_any(checker, checker.pool.type_expr(arg_id));
            }
        }
        ExprKind::Field { base, .. }
        | ExprKind::SafeField { base, .. }
        | ExprKind::Alloc { expr: base, .. }
        | ExprKind::Try { expr: base, .. }
        | ExprKind::Group { expr: base, .. }
        | ExprKind::Unary { expr: base, .. } => {
            validate_expr(checker, *base);
        }
        ExprKind::Cast { expr: base, ty, .. } => {
            validate_expr(checker, *base);
            validate_type_no_any(checker, checker.pool.type_expr(*ty));
        }
        ExprKind::Index { base, index, .. }
        | ExprKind::SafeIndex { base, index, .. }
        | ExprKind::NullCoalesce {
            left: base,
            right: index,
            ..
        }
        | ExprKind::Binary {
            left: base,
            right: index,
            ..
        } => {
            validate_expr(checker, *base);
            validate_expr(checker, *index);
        }
        ExprKind::Call {
            callee,
            args,
            trailing_block,
            ..
        } => {
            validate_expr(checker, *callee);
            let arg_ids = checker.pool.expr_list(*args).to_vec();
            for arg_id in arg_ids {
                validate_expr(checker, arg_id);
            }
            if let Some(block_id) = trailing_block {
                validate_block(checker, checker.pool, checker.pool.block(*block_id));
            }
        }
        ExprKind::StructLiteral { ty, fields, .. } => {
            validate_type_no_any(checker, checker.pool.type_expr(*ty));
            let field_ids = checker.pool.field_init_list(*fields).to_vec();
            for field_id in field_ids {
                validate_expr(checker, checker.pool.field_init(field_id).value);
            }
        }
        ExprKind::Array { items, .. } => {
            let item_ids = checker.pool.expr_list(*items).to_vec();
            for item_id in item_ids {
                validate_expr(checker, item_id);
            }
        }
        ExprKind::Lambda { params, body, .. } => {
            let param_ids = checker.pool.lambda_param_list(*params).to_vec();
            for param_id in param_ids {
                let param = checker.pool.lambda_param(param_id);
                if let Some(ty) = &param.ty {
                    validate_type_no_any(checker, ty);
                }
            }
            match body {
                LambdaBody::Expr {
                    expr: inner_expr, ..
                } => validate_expr(checker, *inner_expr),
                LambdaBody::Block { block, .. } => validate_block(checker, checker.pool, block),
            }
        }
        ExprKind::AsyncBlock { block, .. } | ExprKind::UnsafeBlock { block, .. } => {
            validate_block(checker, checker.pool, checker.pool.block(*block));
        }
        ExprKind::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            validate_condition(checker, condition);
            validate_block(checker, checker.pool, checker.pool.block(*then_block));
            validate_block(checker, checker.pool, checker.pool.block(*else_block));
        }
        ExprKind::Match { value, arms, .. } => {
            validate_expr(checker, *value);
            let arm_ids = checker.pool.match_arm_list(*arms).to_vec();
            for arm_id in arm_ids {
                let arm = checker.pool.match_arm(arm_id);
                if let Some(guard) = &arm.guard {
                    validate_expr(checker, *guard);
                }
                match &arm.body {
                    MatchArmBody::Expr {
                        expr: inner_expr, ..
                    } => validate_expr(checker, **inner_expr),
                    MatchArmBody::Block { block, .. } => {
                        validate_block(checker, checker.pool, block)
                    }
                }
            }
        }
        ExprKind::Catch {
            expr: base,
            handler,
            ..
        } => {
            validate_expr(checker, *base);
            match checker.pool.catch_handler(*handler) {
                CatchHandler::Expr {
                    expr: inner_expr, ..
                } => validate_expr(checker, *inner_expr),
                CatchHandler::Block { block, .. } => validate_block(checker, checker.pool, block),
            }
        }
        ExprKind::InterpolatedString { parts, .. } => {
            let part_ids = checker.pool.string_part_list(*parts).to_vec();
            for part_id in part_ids {
                if let arandu_parser::StringPart::Expr {
                    expr: inner_expr, ..
                } = checker.pool.string_part(part_id)
                {
                    validate_expr(checker, *inner_expr);
                }
            }
        }
        _ => {}
    }
}

fn validate_condition(checker: &mut TypeChecker<'_>, cond: &Condition) {
    match cond {
        Condition::Expr { expr, .. } => validate_expr(checker, **expr),
        Condition::Is { expr, .. } => validate_expr(checker, **expr),
    }
}

fn validate_simple_stmt(checker: &mut TypeChecker<'_>, stmt: &SimpleStmt) {
    match stmt {
        SimpleStmt::VarDecl {
            bindings, value, ..
        } => {
            for binding in bindings {
                if let Some(ty) = &binding.ty {
                    validate_type_no_any(checker, ty);
                }
            }
            validate_expr(checker, *value);
        }
        SimpleStmt::Set { value, .. } => {
            validate_expr(checker, *value);
        }
        SimpleStmt::Expr { expr, .. } => {
            validate_expr(checker, **expr);
        }
    }
}

fn validate_block(checker: &mut TypeChecker<'_>, pool: &AstPool, block: &Block) {
    for stmt in &block.statements {
        let stmt = pool.stmt(*stmt);
        match stmt {
            Stmt::VarDecl {
                bindings, value, ..
            } => {
                for binding in bindings {
                    if let Some(ty) = &binding.ty {
                        validate_type_no_any(checker, ty);
                    }
                }
                validate_expr(checker, *value);
            }
            Stmt::Set { value, .. } => {
                validate_expr(checker, *value);
            }
            Stmt::Return { values, .. } => {
                for val in values {
                    validate_expr(checker, *val);
                }
            }
            Stmt::Free { expr, .. } => {
                validate_expr(checker, *expr);
            }
            Stmt::Expr { expr, .. } => {
                validate_expr(checker, **expr);
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                validate_condition(checker, condition);
                validate_block(checker, pool, then_block);
                if let Some(block) = else_block {
                    validate_block(checker, pool, block);
                }
            }
            Stmt::For { clause, body, .. } => {
                match &**clause {
                    ForClause::In { iterable, .. } => {
                        validate_expr(checker, **iterable);
                    }
                    ForClause::CStyle {
                        init,
                        condition,
                        step,
                        ..
                    } => {
                        if let Some(init_stmt) = init {
                            validate_simple_stmt(checker, init_stmt);
                        }
                        if let Some(cond_expr) = condition {
                            validate_expr(checker, **cond_expr);
                        }
                        if let Some(step_stmt) = step {
                            validate_simple_stmt(checker, step_stmt);
                        }
                    }
                }
                validate_block(checker, pool, body);
            }
            Stmt::While {
                condition, body, ..
            } => {
                validate_condition(checker, condition);
                validate_block(checker, pool, body);
            }
            Stmt::Match { expr, .. } => {
                validate_expr(checker, *expr);
            }
            Stmt::Defer { body, .. } | Stmt::ErrDefer { body, .. } => match body {
                DeferBody::Expr { expr, .. } => validate_expr(checker, **expr),
                DeferBody::Block { block, .. } => validate_block(checker, pool, block),
            },
            Stmt::Unsafe { block, .. } => {
                validate_block(checker, pool, block);
            }
            _ => {}
        }
    }
}

pub(crate) fn validate_top_level_any(checker: &mut TypeChecker<'_>, decl: &TopLevelDecl) {
    match decl {
        TopLevelDecl::Struct(struct_decl) => {
            for field in &struct_decl.fields {
                validate_type_no_any(checker, &field.ty);
            }
        }
        TopLevelDecl::Enum(enum_decl) => {
            for variant in &enum_decl.variants {
                if let Some(payload) = &variant.payload {
                    match payload {
                        arandu_parser::EnumPayload::Tuple { types, .. } => {
                            for ty in types {
                                validate_type_no_any(checker, ty);
                            }
                        }
                        arandu_parser::EnumPayload::Struct { fields, .. } => {
                            for field in fields {
                                validate_type_no_any(checker, &field.ty);
                            }
                        }
                    }
                }
            }
        }
        TopLevelDecl::TypeAlias(alias_decl) => {
            validate_type_no_any(checker, &alias_decl.ty);
        }
        TopLevelDecl::Func(func_decl) => {
            for param in &func_decl.params {
                if !param.is_variadic {
                    validate_type_no_any(checker, &param.ty);
                }
            }
            if let Some(result) = &func_decl.result {
                validate_result_type_no_any(checker, result);
            }
            validate_block(checker, checker.pool, &func_decl.body);
        }
        _ => {}
    }
}
