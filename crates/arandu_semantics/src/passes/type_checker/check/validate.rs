use arandu_parser::{
    Block, CatchHandler, Condition, DeferBody, Expr, ForClause, LambdaBody, MatchArmBody,
    ResultType, SimpleStmt, Stmt, TopLevelDecl, TypeExpr,
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

fn report_any_error(checker: &mut TypeChecker, span: Span) {
    checker.diagnostics.push(crate::Diagnostic::error(
        crate::DiagCode::T014InvalidVariadicType,
        ANY_ERROR_MESSAGE,
        span,
    ));
}

fn validate_type_no_any(checker: &mut TypeChecker, ty: &TypeExpr) {
    if let Some(span) = contains_any(ty) {
        report_any_error(checker, span);
    }
}

fn validate_result_type_no_any(checker: &mut TypeChecker, result: &ResultType) {
    match result {
        ResultType::Single { ty, .. } => validate_type_no_any(checker, ty),
        ResultType::Multi { types, .. } => {
            for ty in types {
                validate_type_no_any(checker, ty);
            }
        }
    }
}

fn validate_expr(checker: &mut TypeChecker, expr: &Expr) {
    match expr {
        Expr::Generic { callee, args, .. } => {
            validate_expr(checker, callee);
            for arg in args {
                validate_type_no_any(checker, arg);
            }
        }
        Expr::Field { base, .. }
        | Expr::SafeField { base, .. }
        | Expr::Alloc { expr: base, .. }
        | Expr::Try { expr: base, .. }
        | Expr::Group { expr: base, .. }
        | Expr::Unary { expr: base, .. } => {
            validate_expr(checker, base);
        }
        Expr::Cast { expr: base, ty, .. } => {
            validate_expr(checker, base);
            validate_type_no_any(checker, ty);
        }
        Expr::Index { base, index, .. }
        | Expr::SafeIndex { base, index, .. }
        | Expr::NullCoalesce {
            left: base,
            right: index,
            ..
        }
        | Expr::Binary {
            left: base,
            right: index,
            ..
        } => {
            validate_expr(checker, base);
            validate_expr(checker, index);
        }
        Expr::Call {
            callee,
            args,
            trailing_block,
            ..
        } => {
            validate_expr(checker, callee);
            for arg in args {
                validate_expr(checker, arg);
            }
            if let Some(block) = trailing_block {
                validate_block(checker, block);
            }
        }
        Expr::StructLiteral { ty, fields, .. } => {
            validate_type_no_any(checker, ty);
            for field in fields {
                validate_expr(checker, &field.value);
            }
        }
        Expr::Array { items, .. } => {
            for item in items {
                validate_expr(checker, item);
            }
        }
        Expr::Lambda { params, body, .. } => {
            for param in params {
                if let Some(ty) = &param.ty {
                    validate_type_no_any(checker, ty);
                }
            }
            match body {
                LambdaBody::Expr { expr, .. } => validate_expr(checker, expr),
                LambdaBody::Block { block, .. } => validate_block(checker, block),
            }
        }
        Expr::AsyncBlock { block, .. } | Expr::UnsafeBlock { block, .. } => {
            validate_block(checker, block);
        }
        Expr::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            validate_condition(checker, condition);
            validate_block(checker, then_block);
            validate_block(checker, else_block);
        }
        Expr::Match { value, arms, .. } => {
            validate_expr(checker, value);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    validate_expr(checker, guard);
                }
                match &arm.body {
                    MatchArmBody::Expr { expr, .. } => validate_expr(checker, expr),
                    MatchArmBody::Block { block, .. } => validate_block(checker, block),
                }
            }
        }
        Expr::Catch {
            expr: base,
            handler,
            ..
        } => {
            validate_expr(checker, base);
            match handler {
                CatchHandler::Expr { expr, .. } => validate_expr(checker, expr),
                CatchHandler::Block { block, .. } => validate_block(checker, block),
            }
        }
        Expr::InterpolatedString { parts, .. } => {
            for part in parts {
                if let arandu_parser::StringPart::Expr { expr, .. } = part {
                    validate_expr(checker, expr);
                }
            }
        }
        _ => {}
    }
}

fn validate_condition(checker: &mut TypeChecker, cond: &Condition) {
    match cond {
        Condition::Expr { expr, .. } => validate_expr(checker, expr),
        Condition::Is { expr, .. } => validate_expr(checker, expr),
    }
}

fn validate_simple_stmt(checker: &mut TypeChecker, stmt: &SimpleStmt) {
    match stmt {
        SimpleStmt::VarDecl {
            bindings, value, ..
        } => {
            for binding in bindings {
                if let Some(ty) = &binding.ty {
                    validate_type_no_any(checker, ty);
                }
            }
            validate_expr(checker, value);
        }
        SimpleStmt::Set { value, .. } => {
            validate_expr(checker, value);
        }
        SimpleStmt::Expr { expr, .. } => {
            validate_expr(checker, expr);
        }
    }
}

fn validate_block(checker: &mut TypeChecker, block: &Block) {
    for stmt in &block.statements {
        match stmt {
            Stmt::VarDecl {
                bindings, value, ..
            } => {
                for binding in bindings {
                    if let Some(ty) = &binding.ty {
                        validate_type_no_any(checker, ty);
                    }
                }
                validate_expr(checker, value);
            }
            Stmt::Set { value, .. } => {
                validate_expr(checker, value);
            }
            Stmt::Return { values, .. } => {
                for val in values {
                    validate_expr(checker, val);
                }
            }
            Stmt::Free { expr, .. } => {
                validate_expr(checker, expr);
            }
            Stmt::Expr { expr, .. } => {
                validate_expr(checker, expr);
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                validate_condition(checker, condition);
                validate_block(checker, then_block);
                if let Some(block) = else_block {
                    validate_block(checker, block);
                }
            }
            Stmt::For { clause, body, .. } => {
                match &**clause {
                    ForClause::In { iterable, .. } => {
                        validate_expr(checker, iterable);
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
                            validate_expr(checker, cond_expr);
                        }
                        if let Some(step_stmt) = step {
                            validate_simple_stmt(checker, step_stmt);
                        }
                    }
                }
                validate_block(checker, body);
            }
            Stmt::While {
                condition, body, ..
            } => {
                validate_condition(checker, condition);
                validate_block(checker, body);
            }
            Stmt::Match { expr, .. } => {
                validate_expr(checker, expr);
            }
            Stmt::Defer { body, .. } | Stmt::ErrDefer { body, .. } => match body {
                DeferBody::Expr { expr, .. } => validate_expr(checker, expr),
                DeferBody::Block { block, .. } => validate_block(checker, block),
            },
            Stmt::Unsafe { block, .. } => {
                validate_block(checker, block);
            }
            _ => {}
        }
    }
}

pub(crate) fn validate_top_level_any(checker: &mut TypeChecker, decl: &TopLevelDecl) {
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
            validate_block(checker, &func_decl.body);
        }
        _ => {}
    }
}
