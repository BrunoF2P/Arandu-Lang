use crate::diagnostics::{Diagnostic, Severity};
use crate::hir::{HirFieldPattern, HirMatchArm, HirMatchArmBody, HirPattern, HirPlaceSuffix, *};
use crate::ops::SetOp;
use crate::passes::lowering::{require_def_symbol, require_type_symbol, require_value_symbol};
use crate::passes::type_checker::types::ArType;
use crate::{NodeKey, TypeCheckResult};
use arandu_lexer::Span;
use arandu_parser::Pattern;
use arandu_parser::{
    Block, CatchHandler, Condition, DeferBody, Expr, ForClause, LambdaBody, MatchArm, MatchArmBody,
    Place, PlaceSuffix, Program, SimpleStmt, Stmt, TopLevelDecl,
};

pub fn lower_to_hir(
    type_check: &TypeCheckResult,
    program: &Program,
) -> Result<HirProgram, Vec<Diagnostic>> {
    if type_check
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return Err(type_check.diagnostics.clone());
    }

    let mut decls = Vec::new();
    for decl in &program.decls {
        decls.push(lower_decl(type_check, decl).map_err(|e| vec![e])?);
    }
    let module = program.module.as_ref().map(|m| m.path.join("."));
    Ok(HirProgram {
        span: program.span,
        module,
        decls,
    })
}

fn lower_decl(type_check: &TypeCheckResult, decl: &TopLevelDecl) -> Result<HirDecl, Diagnostic> {
    match decl {
        TopLevelDecl::Const(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let ty = type_check
                .type_info
                .decl_types
                .get(&symbol)
                .cloned()
                .unwrap_or(ArType::Error);
            Ok(HirDecl::Const(HirConst {
                symbol,
                ty,
                value: lower_expr(type_check, &d.value)?,
                span: d.span,
            }))
        }
        TopLevelDecl::TypeAlias(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let target = type_check
                .type_info
                .decl_types
                .get(&symbol)
                .cloned()
                .unwrap_or(ArType::Error);
            Ok(HirDecl::TypeAlias(HirTypeAlias {
                symbol,
                target,
                span: d.span,
            }))
        }
        TopLevelDecl::Func(d) => {
            let name_span = match &d.name {
                arandu_parser::FuncName::Free { span, .. } => *span,
                arandu_parser::FuncName::Method { span, .. } => *span,
            };
            let symbol = require_def_symbol(&type_check.resolved, name_span)?;
            let decl_ty = type_check
                .type_info
                .decl_types
                .get(&symbol)
                .cloned()
                .unwrap_or(ArType::Error);
            let return_type = match decl_ty {
                ArType::Func(_, ret) => *ret,
                other => other,
            };
            let mut params = Vec::new();
            for p in &d.params {
                let p_symbol = require_def_symbol(&type_check.resolved, p.span)?;
                let p_ty = type_check
                    .type_info
                    .decl_types
                    .get(&p_symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                params.push(HirParam {
                    symbol: p_symbol,
                    ty: p_ty,
                    span: p.span,
                });
            }
            let body = Some(lower_block(type_check, &d.body)?);
            Ok(HirDecl::Func(HirFunc {
                symbol,
                params,
                return_type,
                body,
                span: d.span,
            }))
        }
        TopLevelDecl::Struct(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let mut fields = Vec::new();
            if let Some(struct_fields_map) = type_check.type_info.struct_fields.get(&symbol) {
                for f in &d.fields {
                    let field_symbol = require_def_symbol(&type_check.resolved, f.span)?;
                    let field_ty = struct_fields_map
                        .get(&f.name)
                        .cloned()
                        .unwrap_or(ArType::Error);
                    fields.push(HirStructField {
                        symbol: field_symbol,
                        ty: field_ty,
                        span: f.span,
                    });
                }
            }
            Ok(HirDecl::Struct(HirStruct {
                symbol,
                fields,
                span: d.span,
            }))
        }
        TopLevelDecl::Enum(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            let mut variants = Vec::new();
            for v in &d.variants {
                let v_symbol = require_def_symbol(&type_check.resolved, v.span)?;
                let payload =
                    type_check
                        .type_info
                        .enum_variants
                        .get(&v_symbol)
                        .and_then(|(_, shape)| match shape {
                            crate::passes::type_checker::EnumPayloadShape::Unit => None,
                            crate::passes::type_checker::EnumPayloadShape::Tuple(tys) => {
                                if tys.is_empty() {
                                    None
                                } else if tys.len() == 1 {
                                    Some(tys[0].clone())
                                } else {
                                    Some(ArType::Tuple(tys.clone()))
                                }
                            }
                        });
                variants.push(HirEnumVariant {
                    symbol: v_symbol,
                    payload,
                    span: v.span,
                });
            }
            Ok(HirDecl::Enum(HirEnum {
                symbol,
                variants,
                span: d.span,
            }))
        }
        TopLevelDecl::Interface(d) => {
            let symbol = require_def_symbol(&type_check.resolved, d.span)?;
            Ok(HirDecl::Interface(HirInterface {
                symbol,
                span: d.span,
            }))
        }
        TopLevelDecl::Extern(d) => {
            let mut members = Vec::new();
            for m in &d.members {
                let symbol = require_def_symbol(&type_check.resolved, m.span)?;
                let m_ty = type_check
                    .type_info
                    .decl_types
                    .get(&symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                let return_type = match m_ty {
                    ArType::Func(_, ret) => *ret,
                    other => other,
                };
                let mut params = Vec::new();
                for p in &m.params {
                    let p_symbol = require_def_symbol(&type_check.resolved, p.span)?;
                    let p_ty = type_check
                        .type_info
                        .decl_types
                        .get(&p_symbol)
                        .cloned()
                        .unwrap_or(ArType::Error);
                    params.push(HirParam {
                        symbol: p_symbol,
                        ty: p_ty,
                        span: p.span,
                    });
                }
                members.push(HirFuncSignature {
                    symbol,
                    params,
                    return_type,
                    span: m.span,
                });
            }
            Ok(HirDecl::Extern(HirExtern {
                abi: d.abi.clone(),
                members,
                span: d.span,
            }))
        }
        TopLevelDecl::Error(_) => unreachable!("syntax error in HIR lowering"),
    }
}

fn lower_pattern(
    type_check: &TypeCheckResult,
    pattern: &Pattern,
) -> Result<HirPattern, Diagnostic> {
    match pattern {
        Pattern::Wildcard { span } => Ok(HirPattern::Wildcard { span: *span }),
        Pattern::Bind { span, name } => Ok(HirPattern::Bind {
            span: *span,
            name: name.clone(),
        }),
        Pattern::Literal { span, expr } => Ok(HirPattern::Literal {
            span: *span,
            expr: Box::new(lower_expr(type_check, expr)?),
        }),
        Pattern::Enum {
            span,
            type_name,
            variant,
            payload,
        } => {
            let type_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let variant_symbol = type_check
                .resolved
                .definitions
                .get(&NodeKey::from(*span))
                .copied();
            let mut hir_payload = Vec::new();
            for p in payload {
                hir_payload.push(lower_pattern(type_check, p)?);
            }
            Ok(HirPattern::Enum {
                span: *span,
                type_symbol,
                variant: variant.clone(),
                variant_symbol,
                payload: hir_payload,
            })
        }
        Pattern::TypeTuple {
            span,
            name,
            payload,
        } => {
            let mut hir_payload = Vec::new();
            for p in payload {
                hir_payload.push(lower_pattern(type_check, p)?);
            }
            Ok(HirPattern::TypeTuple {
                span: *span,
                name: name.clone(),
                payload: hir_payload,
            })
        }
        Pattern::Struct {
            span,
            type_name,
            fields,
        } => {
            let struct_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let mut hir_fields = Vec::new();
            for f in fields {
                hir_fields.push(HirFieldPattern {
                    span: f.span,
                    name: f.name.clone(),
                    pattern: match f.pattern.as_ref() {
                        Some(p) => Some(Box::new(lower_pattern(type_check, p)?)),
                        None => None,
                    },
                });
            }
            Ok(HirPattern::Struct {
                span: *span,
                struct_symbol,
                fields: hir_fields,
            })
        }
        Pattern::Tuple { span, items } => {
            let mut hir_items = Vec::new();
            for p in items {
                hir_items.push(lower_pattern(type_check, p)?);
            }
            Ok(HirPattern::Tuple {
                span: *span,
                items: hir_items,
            })
        }
        Pattern::Range {
            span,
            start,
            inclusive,
            end,
        } => Ok(HirPattern::Range {
            span: *span,
            start: Box::new(lower_expr(type_check, start)?),
            inclusive: *inclusive,
            end: Box::new(lower_expr(type_check, end)?),
        }),
    }
}

fn lower_match_arms(
    type_check: &TypeCheckResult,
    arms: &[MatchArm],
) -> Result<Vec<HirMatchArm>, Diagnostic> {
    let mut hir_arms = Vec::new();
    for arm in arms {
        let guard = arm
            .guard
            .as_ref()
            .map(|g| lower_expr(type_check, g))
            .transpose()?;
        let body = match &arm.body {
            MatchArmBody::Expr { expr, .. } => {
                HirMatchArmBody::Expr(Box::new(lower_expr(type_check, expr)?))
            }
            MatchArmBody::Block { block, .. } => {
                HirMatchArmBody::Block(lower_block(type_check, block)?)
            }
        };
        hir_arms.push(HirMatchArm {
            span: arm.span,
            pattern: lower_pattern(type_check, &arm.pattern)?,
            guard,
            body,
        });
    }
    Ok(hir_arms)
}

fn lower_block(type_check: &TypeCheckResult, block: &Block) -> Result<HirBlock, Diagnostic> {
    let mut statements = Vec::new();
    for s in &block.statements {
        statements.push(lower_stmt(type_check, s)?);
    }
    Ok(HirBlock {
        statements,
        span: block.span,
    })
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::VarDecl { span, .. }
        | Stmt::Set { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::Break { span }
        | Stmt::Continue { span }
        | Stmt::Free { span, .. }
        | Stmt::Expr { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::Match { span, .. }
        | Stmt::Defer { span, .. }
        | Stmt::ErrDefer { span, .. }
        | Stmt::Unsafe { span, .. }
        | Stmt::Error(span) => *span,
    }
}

fn lower_stmt(type_check: &TypeCheckResult, stmt: &Stmt) -> Result<HirStmt, Diagnostic> {
    let kind = match stmt {
        Stmt::VarDecl {
            bindings, value, ..
        } => {
            let mut hir_bindings = Vec::new();
            for b in bindings {
                let symbol = require_def_symbol(&type_check.resolved, b.span)?;
                let ty = type_check
                    .type_info
                    .decl_types
                    .get(&symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_bindings.push(HirBindingItem {
                    symbol,
                    ty,
                    span: b.span,
                });
            }
            HirStmtKind::VarDecl {
                bindings: hir_bindings,
                value: lower_expr(type_check, value)?,
            }
        }
        Stmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, _> =
                places.iter().map(|p| lower_place(type_check, p)).collect();
            HirStmtKind::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: lower_expr(type_check, value)?,
            }
        }
        Stmt::Return { values, .. } => {
            let hir_values: Result<Vec<_>, _> =
                values.iter().map(|v| lower_expr(type_check, v)).collect();
            HirStmtKind::Return {
                values: hir_values?,
            }
        }
        Stmt::Break { .. } => HirStmtKind::Break,
        Stmt::Continue { .. } => HirStmtKind::Continue,
        Stmt::Free { expr, .. } => HirStmtKind::Free(lower_expr(type_check, expr)?),
        Stmt::Expr { expr, .. } => HirStmtKind::Expr(lower_expr(type_check, expr)?),
        Stmt::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirStmtKind::If {
            condition: lower_condition(type_check, condition)?,
            then_block: lower_block(type_check, then_block)?,
            else_block: else_block
                .as_ref()
                .map(|b| lower_block(type_check, b))
                .transpose()?,
        },
        Stmt::For { clause, body, .. } => HirStmtKind::For {
            clause: lower_for_clause(type_check, clause)?,
            body: lower_block(type_check, body)?,
        },
        Stmt::While {
            condition, body, ..
        } => HirStmtKind::While {
            condition: lower_condition(type_check, condition)?,
            body: lower_block(type_check, body)?,
        },
        Stmt::Match { expr, .. } => match expr {
            Expr::Match { value, arms, .. } => HirStmtKind::Match {
                value: lower_expr(type_check, value)?,
                arms: lower_match_arms(type_check, arms)?,
            },
            other => HirStmtKind::Expr(lower_expr(type_check, other)?),
        },
        Stmt::Defer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => HirBlock {
                    statements: vec![HirStmt {
                        kind: HirStmtKind::Expr(lower_expr(type_check, expr)?),
                        span: *span,
                    }],
                    span: *span,
                },
                DeferBody::Block { block, .. } => lower_block(type_check, block)?,
            };
            HirStmtKind::Defer(block)
        }
        Stmt::ErrDefer { body, .. } => {
            let block = match body {
                DeferBody::Expr { span, expr } => HirBlock {
                    statements: vec![HirStmt {
                        kind: HirStmtKind::Expr(lower_expr(type_check, expr)?),
                        span: *span,
                    }],
                    span: *span,
                },
                DeferBody::Block { block, .. } => lower_block(type_check, block)?,
            };
            HirStmtKind::ErrDefer(block)
        }
        Stmt::Unsafe { block, .. } => HirStmtKind::Unsafe(lower_block(type_check, block)?),
        Stmt::Error(_) => unreachable!("syntax error in HIR lowering"),
    };
    Ok(HirStmt {
        kind,
        span: stmt_span(stmt),
    })
}

fn get_resolved_value_ref(type_check: &TypeCheckResult, span: arandu_lexer::Span) -> Option<crate::symbol_table::SymbolId> {
    type_check.resolved.value_refs.get(&NodeKey::from(span)).copied()
}

fn lower_expr(type_check: &TypeCheckResult, expr: &Expr) -> Result<HirExpr, Diagnostic> {
    let span = expr.span();
    let key = NodeKey::from(span);
    let ty = type_check
        .type_info
        .expr_types
        .get(&key)
        .cloned()
        .unwrap_or(ArType::Error);

    let kind = match expr {
        Expr::Path { .. } => {
            let symbol = require_value_symbol(&type_check.resolved, span)?;
            HirExprKind::Path { symbol }
        }
        Expr::TypePath {
            type_name,
            member: _,
            ..
        } => {
            let type_symbol = require_type_symbol(&type_check.resolved, type_name.span)?;
            let member_symbol = require_value_symbol(&type_check.resolved, span)?;
            HirExprKind::TypePath {
                type_symbol,
                member_symbol,
            }
        }
        Expr::Generic { callee, args, .. } => {
            let hir_callee = lower_expr(type_check, callee)?;
            let mut hir_args = Vec::new();
            for arg in args {
                hir_args.push(crate::passes::type_checker::types::lower_type_expr(
                    arg,
                    &type_check.symbols,
                    crate::ScopeId(0),
                    &type_check.resolved,
                ));
            }
            HirExprKind::Generic {
                callee: Box::new(hir_callee),
                args: hir_args,
            }
        }
        Expr::Field { base, field, .. } => {
            if let Some(symbol) = get_resolved_value_ref(type_check, span) {
                HirExprKind::Path { symbol }
            } else {
                HirExprKind::Field {
                    base: Box::new(lower_expr(type_check, base)?),
                    field: field.clone(),
                }
            }
        }
        Expr::SafeField { base, field, .. } => {
            if let Some(symbol) = get_resolved_value_ref(type_check, span) {
                HirExprKind::Path { symbol }
            } else {
                HirExprKind::SafeField {
                    base: Box::new(lower_expr(type_check, base)?),
                    field: field.clone(),
                }
            }
        }
        Expr::Index { base, index, .. } => HirExprKind::Index {
            base: Box::new(lower_expr(type_check, base)?),
            index: Box::new(lower_expr(type_check, index)?),
        },
        Expr::SafeIndex { base, index, .. } => HirExprKind::SafeIndex {
            base: Box::new(lower_expr(type_check, base)?),
            index: Box::new(lower_expr(type_check, index)?),
        },
        Expr::Try { expr, .. } => HirExprKind::Try {
            expr: Box::new(lower_expr(type_check, expr)?),
        },
        Expr::Call {
            callee,
            args,
            trailing_block,
            ..
        } => {
            let hir_callee = lower_expr(type_check, callee)?;
            let hir_args: Result<Vec<_>, _> =
                args.iter().map(|a| lower_expr(type_check, a)).collect();
            let hir_trailing = trailing_block
                .as_ref()
                .map(|b| lower_block(type_check, b))
                .transpose()?;
            HirExprKind::Call {
                callee: Box::new(hir_callee),
                args: hir_args?,
                trailing_block: hir_trailing,
            }
        }
        Expr::StructLiteral { fields, .. } => {
            let struct_symbol = match &ty {
                ArType::Named(id, _) => *id,
                _ => {
                    return Err(Diagnostic::error(
                        crate::diagnostics::DiagCode::L001LoweringUnresolvedSymbol,
                        "cannot lower struct literal: type is not a named struct",
                        span,
                    ));
                }
            };
            let hir_fields: Result<Vec<_>, _> = fields
                .iter()
                .map(|f| {
                    Ok(HirFieldInit {
                        span: f.span,
                        name: f.name.clone(),
                        value: lower_expr(type_check, &f.value)?,
                    })
                })
                .collect();
            HirExprKind::StructLiteral {
                struct_symbol,
                fields: hir_fields?,
            }
        }
        Expr::Array { items, .. } => {
            let hir_items: Result<Vec<_>, _> =
                items.iter().map(|i| lower_expr(type_check, i)).collect();
            HirExprKind::Array { items: hir_items? }
        }
        Expr::Lambda { params, body, .. } => {
            let mut hir_params = Vec::new();
            for p in params {
                let symbol = require_def_symbol(&type_check.resolved, p.span)?;
                let p_ty = type_check
                    .type_info
                    .decl_types
                    .get(&symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_params.push(HirLambdaParam {
                    span: p.span,
                    symbol,
                    ty: p_ty,
                });
            }
            let hir_body = match body {
                LambdaBody::Expr { expr, .. } => {
                    HirLambdaBody::Expr(Box::new(lower_expr(type_check, expr)?))
                }
                LambdaBody::Block { block, .. } => {
                    HirLambdaBody::Block(lower_block(type_check, block)?)
                }
            };
            HirExprKind::Lambda {
                params: hir_params,
                body: hir_body,
            }
        }
        Expr::Alloc { expr, .. } => HirExprKind::Alloc {
            expr: Box::new(lower_expr(type_check, expr)?),
        },
        Expr::AsyncBlock { block, .. } => HirExprKind::AsyncBlock {
            block: lower_block(type_check, block)?,
        },
        Expr::UnsafeBlock { block, .. } => HirExprKind::UnsafeBlock {
            block: lower_block(type_check, block)?,
        },
        Expr::If {
            condition,
            then_block,
            else_block,
            ..
        } => HirExprKind::If {
            condition: Box::new(lower_condition(type_check, condition)?),
            then_block: lower_block(type_check, then_block)?,
            else_block: lower_block(type_check, else_block)?,
        },
        Expr::Match { value, arms, .. } => HirExprKind::Match {
            value: Box::new(lower_expr(type_check, value)?),
            arms: lower_match_arms(type_check, arms)?,
        },
        Expr::Catch { expr, handler, .. } => {
            let hir_expr = lower_expr(type_check, expr)?;
            let hir_handler = match handler {
                CatchHandler::Expr { expr, .. } => {
                    HirCatchHandler::Expr(Box::new(lower_expr(type_check, expr)?))
                }
                CatchHandler::Block { span, error, block } => {
                    let error_symbol = type_check
                        .resolved
                        .definitions
                        .get(&NodeKey::from(*span))
                        .copied();
                    HirCatchHandler::Block {
                        error_symbol,
                        error_name: Some(error.clone()),
                        block: lower_block(type_check, block)?,
                    }
                }
            };
            HirExprKind::Catch {
                expr: Box::new(hir_expr),
                handler: hir_handler,
            }
        }
        Expr::NullCoalesce { left, right, .. } => HirExprKind::NullCoalesce {
            left: Box::new(lower_expr(type_check, left)?),
            right: Box::new(lower_expr(type_check, right)?),
        },
        Expr::Cast {
            expr, ty: cast_ty, ..
        } => {
            let target_ty = crate::passes::type_checker::types::lower_type_expr(
                cast_ty,
                &type_check.symbols,
                crate::ScopeId(0),
                &type_check.resolved,
            );
            HirExprKind::Cast {
                expr: Box::new(lower_expr(type_check, expr)?),
                target_ty,
            }
        }
        Expr::Group { expr, .. } => {
            return lower_expr(type_check, expr);
        }
        Expr::Unary { op, expr, .. } => HirExprKind::Unary {
            op: (*op).into(),
            expr: Box::new(lower_expr(type_check, expr)?),
        },
        Expr::Binary {
            op, left, right, ..
        } => HirExprKind::Binary {
            op: (*op).into(),
            left: Box::new(lower_expr(type_check, left)?),
            right: Box::new(lower_expr(type_check, right)?),
        },
        Expr::Int { value, .. } => HirExprKind::Int(value.clone()),
        Expr::Float { value, .. } => HirExprKind::Float(value.clone()),
        Expr::Bool { value, .. } => HirExprKind::Bool(*value),
        Expr::Char { value, .. } => HirExprKind::Char(value.clone()),
        Expr::InterpolatedString { .. } => HirExprKind::Str("interpolated".to_string()),
        Expr::Nil { .. } => HirExprKind::Nil,
        Expr::Error(_) => unreachable!("syntax error in HIR lowering"),
    };

    Ok(HirExpr { kind, ty, span })
}

fn lower_place(type_check: &TypeCheckResult, place: &Place) -> Result<HirPlace, Diagnostic> {
    let root_key = NodeKey::from(place.span);
    let root_symbol = type_check
        .resolved
        .value_refs
        .get(&root_key)
        .copied()
        .or_else(|| type_check.resolved.definitions.get(&root_key).copied())
        .ok_or_else(|| {
            Diagnostic::error(
                crate::diagnostics::DiagCode::L001LoweringUnresolvedSymbol,
                "cannot lower place: root symbol not resolved",
                place.span,
            )
        })?;

    let mut current_ty = if let Some(ty) = type_check.type_info.decl_types.get(&root_symbol) {
        ty.clone()
    } else {
        ArType::Error
    };

    let mut suffixes = Vec::new();
    for suffix in &place.suffixes {
        if current_ty.is_error() {
            match suffix {
                PlaceSuffix::Field { span, name } => {
                    suffixes.push(HirPlaceSuffix::Field {
                        span: *span,
                        name: name.clone(),
                        ty: ArType::Error,
                    });
                }
                PlaceSuffix::Index { span, expr } => {
                    suffixes.push(HirPlaceSuffix::Index {
                        span: *span,
                        expr: lower_expr(type_check, expr)?,
                        ty: ArType::Error,
                    });
                }
            }
            continue;
        }

        match suffix {
            PlaceSuffix::Field { span, name } => {
                let actual_base_ty = match &current_ty {
                    ArType::Nullable(inner) => inner.as_ref().clone(),
                    other => other.clone(),
                };
                let struct_id_opt = match &actual_base_ty {
                    ArType::Named(id, _) => Some(*id),
                    ArType::Ptr(inner) => match &**inner {
                        ArType::Named(id, _) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                let field_ty = if let Some(struct_id) = struct_id_opt
                    && let Some(fields) = type_check.type_info.struct_fields.get(&struct_id)
                    && let Some(ty) = fields.get(name)
                {
                    ty.clone()
                } else {
                    ArType::Error
                };
                current_ty = field_ty.clone();
                suffixes.push(HirPlaceSuffix::Field {
                    span: *span,
                    name: name.clone(),
                    ty: field_ty,
                });
            }
            PlaceSuffix::Index { span, expr } => {
                let actual_base_ty = match &current_ty {
                    ArType::Nullable(inner) => inner.as_ref().clone(),
                    other => other.clone(),
                };
                let elem_ty = match &actual_base_ty {
                    ArType::Array(_, inner) | ArType::Slice(inner) => inner.as_ref().clone(),
                    _ => ArType::Error,
                };
                current_ty = elem_ty.clone();
                suffixes.push(HirPlaceSuffix::Index {
                    span: *span,
                    expr: lower_expr(type_check, expr)?,
                    ty: elem_ty,
                });
            }
        }
    }

    Ok(HirPlace {
        root_symbol,
        suffixes: suffixes.into(),
        ty: current_ty,
        span: place.span,
    })
}

fn lower_condition(
    type_check: &TypeCheckResult,
    cond: &Condition,
) -> Result<HirCondition, Diagnostic> {
    match cond {
        Condition::Expr { expr, .. } => Ok(HirCondition::Expr(lower_expr(type_check, expr)?)),
        Condition::Is { expr, pattern, .. } => Ok(HirCondition::Is {
            expr: lower_expr(type_check, expr)?,
            pattern: lower_pattern(type_check, pattern)?,
        }),
    }
}

fn lower_for_clause(
    type_check: &TypeCheckResult,
    clause: &ForClause,
) -> Result<HirForClause, Diagnostic> {
    match clause {
        ForClause::In {
            span,
            bindings,
            iterable,
        } => {
            let mut hir_bindings = Vec::new();
            for b in bindings {
                let symbol = require_def_symbol(&type_check.resolved, b.span)?;
                let ty = type_check
                    .type_info
                    .decl_types
                    .get(&symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_bindings.push(HirForBinding {
                    symbol,
                    ty,
                    span: b.span,
                });
            }
            Ok(HirForClause::In {
                span: *span,
                bindings: hir_bindings,
                iterable: lower_expr(type_check, iterable)?,
            })
        }
        ForClause::CStyle {
            span,
            init,
            condition,
            step,
        } => Ok(HirForClause::CStyle {
            span: *span,
            init: init
                .as_ref()
                .map(|s| lower_simple_stmt(type_check, s))
                .transpose()?,
            condition: condition
                .as_ref()
                .map(|e| lower_expr(type_check, e))
                .transpose()?,
            step: step
                .as_ref()
                .map(|s| lower_simple_stmt(type_check, s))
                .transpose()?,
        }),
    }
}

fn lower_simple_stmt(
    type_check: &TypeCheckResult,
    stmt: &SimpleStmt,
) -> Result<HirSimpleStmt, Diagnostic> {
    match stmt {
        SimpleStmt::VarDecl {
            bindings, value, ..
        } => {
            let mut hir_bindings = Vec::new();
            for b in bindings {
                let symbol = require_def_symbol(&type_check.resolved, b.span)?;
                let ty = type_check
                    .type_info
                    .decl_types
                    .get(&symbol)
                    .cloned()
                    .unwrap_or(ArType::Error);
                hir_bindings.push(HirBindingItem {
                    symbol,
                    ty,
                    span: b.span,
                });
            }
            Ok(HirSimpleStmt::VarDecl {
                bindings: hir_bindings,
                value: lower_expr(type_check, value)?,
            })
        }
        SimpleStmt::Set {
            places, op, value, ..
        } => {
            let hir_places: Result<Vec<_>, _> =
                places.iter().map(|p| lower_place(type_check, p)).collect();
            Ok(HirSimpleStmt::Set {
                places: hir_places?,
                op: SetOp::from(op.clone()),
                value: lower_expr(type_check, value)?,
            })
        }
        SimpleStmt::Expr { expr, .. } => Ok(HirSimpleStmt::Expr(lower_expr(type_check, expr)?)),
    }
}
