use arandu_parser::Stmt;
use arandu_parser::ast_pool::AstPool;
use arandu_parser::ast_pool::ExprKind;

use super::super::TypeChecker;
use super::super::constraints::ConstraintOrigin;
use super::super::types::{ArType, Primitive};
use super::block::check_block;
use super::condition::check_condition;
use super::place::synth_place;

pub fn check_stmt(checker: &mut TypeChecker<'_>, _pool: &AstPool, stmt: &Stmt) {
    match stmt {
        Stmt::VarDecl {
            span: _,
            bindings,
            value,
        } => {
            let val_ty = super::super::synth::synth_expr(checker, *value);

            if bindings.len() > 1 {
                let val_tys = if let Some((ok, err)) = super::super::types::result_ok_err(&val_ty) {
                    vec![ok, err]
                } else {
                    match &val_ty {
                        ArType::Tuple(tys) => tys.clone(),
                        ArType::Error => vec![ArType::Error; bindings.len()],
                        other => vec![other.clone(); bindings.len()],
                    }
                };

                for (i, binding) in bindings.iter().enumerate() {
                    let binding_key = crate::NodeKey::from(binding.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&binding_key).copied()
                    {
                        let elem_ty = val_tys.get(i).cloned().unwrap_or(ArType::Error);
                        let mut bind_ty = elem_ty.clone();

                        if let Some(ty_expr) = &binding.ty {
                            let expected = super::super::types::lower_type_expr(
                                ty_expr,
                                &checker.symbols,
                                checker.type_scope(),
                                &checker.resolved,
                            );

                            if !elem_ty.is_literal()
                                && elem_ty.clone().default_literal()
                                    != expected.clone().default_literal()
                                && expected.is_numeric()
                                && elem_ty.is_numeric()
                            {
                                checker.add_constraint(
                                    expected.clone(),
                                    elem_ty.clone(),
                                    ConstraintOrigin::ImplicitWidening {
                                        source_span: checker.pool.expr_span(*value),
                                        target_span: binding.span,
                                    },
                                );
                            } else if !super::super::types::unify(&expected, &elem_ty) {
                                checker.add_constraint(
                                    expected.clone(),
                                    elem_ty.clone(),
                                    ConstraintOrigin::Assignment {
                                        lhs_span: binding.span,
                                        rhs_span: checker.pool.expr_span(*value),
                                    },
                                );
                            }
                            bind_ty = expected;
                        }

                        checker.ctx.bind(symbol_id, bind_ty.clone());
                        checker.record_decl_type(symbol_id, bind_ty);
                    }
                }
            } else if let Some(binding) = bindings.first() {
                let binding_key = crate::NodeKey::from(binding.span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&binding_key).copied() {
                    let mut bind_ty = val_ty.clone();
                    if matches!(checker.pool.expr(*value), ExprKind::Nil)
                        && let Some(ty_expr) = &binding.ty
                    {
                        let expected = super::super::types::lower_type_expr(
                            ty_expr,
                            &checker.symbols,
                            checker.type_scope(),
                            &checker.resolved,
                        );
                        if !expected.is_error() {
                            checker.type_info.record_expr_type(*value, expected.clone());
                            bind_ty = expected;
                        }
                    }

                    let annotated_as_result = binding.ty.as_ref().is_some_and(|ty_expr| {
                        let expected = super::super::types::lower_type_expr(
                            ty_expr,
                            &checker.symbols,
                            checker.type_scope(),
                            &checker.resolved,
                        );
                        super::super::types::result_ok_err(&expected).is_some()
                    });
                    if super::super::types::result_ok_err(&val_ty).is_some() && !annotated_as_result
                    {
                        checker.diagnostics.push(crate::Diagnostic::warning(
                            crate::DiagCode::W006UnhandledResult,
                            "Result value must be handled with `?` or `value, err = f()`",
                            checker.pool.expr_span(*value),
                        ));
                    }

                    if let Some(ty_expr) = &binding.ty {
                        let expected = super::super::types::lower_type_expr(
                            ty_expr,
                            &checker.symbols,
                            checker.type_scope(),
                            &checker.resolved,
                        );

                        if !val_ty.is_literal()
                            && val_ty.clone().default_literal()
                                != expected.clone().default_literal()
                            && expected.is_numeric()
                            && val_ty.is_numeric()
                        {
                            checker.add_constraint(
                                expected.clone(),
                                val_ty.clone(),
                                ConstraintOrigin::ImplicitWidening {
                                    source_span: checker.pool.expr_span(*value),
                                    target_span: binding.span,
                                },
                            );
                        } else if !super::super::types::unify(&expected, &bind_ty) {
                            checker.add_constraint(
                                expected.clone(),
                                bind_ty.clone(),
                                ConstraintOrigin::Assignment {
                                    lhs_span: binding.span,
                                    rhs_span: checker.pool.expr_span(*value),
                                },
                            );
                        }
                        bind_ty = expected;
                    }

                    checker.ctx.bind(symbol_id, bind_ty.clone());
                    checker.record_decl_type(symbol_id, bind_ty);
                }
            }
        }
        Stmt::Set {
            span: _,
            places,
            op: _,
            value,
        } => {
            for place in places {
                validate_mutability(checker, place);
            }
            let val_ty = super::super::synth::synth_expr(checker, *value);
            if places.len() > 1 {
                let val_tys = if let Some((ok, err)) = super::super::types::result_ok_err(&val_ty) {
                    vec![ok, err]
                } else {
                    match &val_ty {
                        ArType::Tuple(tys) => tys.clone(),
                        ArType::Error => vec![ArType::Error; places.len()],
                        other => vec![other.clone(); places.len()],
                    }
                };
                for (i, place) in places.iter().enumerate() {
                    let expected_ty = synth_place(checker, place);
                    let elem_ty = val_tys.get(i).cloned().unwrap_or(ArType::Error);
                    if !super::super::types::unify(&expected_ty, &elem_ty) {
                        checker.add_constraint(
                            expected_ty.clone(),
                            elem_ty.clone(),
                            ConstraintOrigin::SetTarget {
                                place_span: place.span,
                                value_span: checker.pool.expr_span(*value),
                            },
                        );
                    } else if !elem_ty.is_literal()
                        && elem_ty.clone().default_literal()
                            != expected_ty.clone().default_literal()
                        && expected_ty.is_numeric()
                        && elem_ty.is_numeric()
                    {
                        checker.add_constraint(
                            expected_ty,
                            elem_ty.clone(),
                            ConstraintOrigin::ImplicitWidening {
                                source_span: checker.pool.expr_span(*value),
                                target_span: place.span,
                            },
                        );
                    }
                }
            } else if let Some(place) = places.first() {
                let expected_ty = synth_place(checker, place);
                if super::super::types::result_ok_err(&val_ty).is_some()
                    && super::super::types::result_ok_err(&expected_ty).is_none()
                {
                    checker.diagnostics.push(crate::Diagnostic::warning(
                        crate::DiagCode::W006UnhandledResult,
                        "Result value must be handled with `?` or `value, err = f()`",
                        checker.pool.expr_span(*value),
                    ));
                }
                if !super::super::types::unify(&expected_ty, &val_ty) {
                    checker.add_constraint(
                        expected_ty.clone(),
                        val_ty.clone(),
                        ConstraintOrigin::SetTarget {
                            place_span: place.span,
                            value_span: checker.pool.expr_span(*value),
                        },
                    );
                } else if !val_ty.is_literal()
                    && val_ty.clone().default_literal() != expected_ty.clone().default_literal()
                    && expected_ty.is_numeric()
                    && val_ty.is_numeric()
                {
                    checker.add_constraint(
                        expected_ty,
                        val_ty.clone(),
                        ConstraintOrigin::ImplicitWidening {
                            source_span: checker.pool.expr_span(*value),
                            target_span: place.span,
                        },
                    );
                }
            }
        }
        Stmt::Return { span, values } => {
            let current_ret = checker
                .ctx
                .current_return()
                .cloned()
                .unwrap_or(ArType::Void);

            let val_ty = if values.is_empty() {
                ArType::Void
            } else if values.len() == 1 {
                super::super::synth::synth_expr(checker, values[0])
            } else {
                let tys = values
                    .iter()
                    .map(|v| super::super::synth::synth_expr(checker, *v))
                    .collect();
                ArType::Tuple(tys)
            };

            if !super::super::types::unify_return(&current_ret, &val_ty) {
                checker.add_constraint(
                    current_ret,
                    val_ty,
                    ConstraintOrigin::ReturnType {
                        return_span: *span,
                        declared_span: checker.ctx.current_return_decl_span().unwrap_or(*span),
                    },
                );
            } else if !val_ty.is_literal()
                && val_ty.clone().default_literal() != current_ret.clone().default_literal()
                && current_ret.is_numeric()
                && val_ty.is_numeric()
            {
                checker.add_constraint(
                    current_ret,
                    val_ty.clone(),
                    ConstraintOrigin::ImplicitWidening {
                        source_span: values.first().map_or(*span, |v| checker.pool.expr_span(*v)),
                        target_span: *span,
                    },
                );
            }
        }
        Stmt::Expr { expr, .. } => {
            super::super::synth::synth_expr(checker, **expr);
        }
        Stmt::If {
            span: _,
            condition,
            then_block,
            else_block,
        } => {
            check_condition(checker, condition);
            check_block(checker, _pool, then_block);
            if let Some(eb) = else_block {
                check_block(checker, _pool, eb);
            }
        }
        Stmt::While {
            span: _,
            condition,
            body,
        } => {
            check_condition(checker, condition);
            checker.ctx.enter_loop();
            check_block(checker, _pool, body);
            checker.ctx.exit_loop();
        }
        Stmt::For {
            span: _,
            clause,
            body,
        } => {
            match &**clause {
                arandu_parser::ForClause::In {
                    span: _,
                    bindings,
                    iterable,
                } => {
                    let iterable_ty = super::super::synth::synth_expr(checker, **iterable);
                    let elem_ty = match &iterable_ty {
                        ArType::Array(_, inner) | ArType::Slice(inner) => inner.as_ref().clone(),
                        ArType::Error => ArType::Error,
                        _ => {
                            checker.diagnostics.push(crate::Diagnostic::error(
                                crate::DiagCode::T005OperatorNotApplicable,
                                format!(
                                    "expected array or slice, found '{}'",
                                    iterable_ty.display(&checker.symbols)
                                ),
                                checker.pool.expr_span(**iterable),
                            ));
                            ArType::Error
                        }
                    };
                    if let Some(binding) = bindings.first() {
                        let binding_key = crate::NodeKey::from(binding.span);
                        if let Some(symbol_id) =
                            checker.resolved.definitions.get(&binding_key).copied()
                        {
                            checker.ctx.bind(symbol_id, elem_ty.clone());
                            checker.record_decl_type(symbol_id, elem_ty);
                        }
                    }
                }
                arandu_parser::ForClause::CStyle {
                    init,
                    condition,
                    step,
                    ..
                } => {
                    if let Some(init_stmt) = init {
                        match &**init_stmt {
                            arandu_parser::SimpleStmt::VarDecl {
                                span,
                                bindings,
                                value,
                            } => {
                                check_stmt(
                                    checker,
                                    _pool,
                                    &Stmt::VarDecl {
                                        span: *span,
                                        bindings: bindings.clone(),
                                        value: *value,
                                    },
                                );
                            }
                            arandu_parser::SimpleStmt::Set {
                                span,
                                places,
                                op,
                                value,
                            } => {
                                check_stmt(
                                    checker,
                                    _pool,
                                    &Stmt::Set {
                                        span: *span,
                                        places: places.clone(),
                                        op: op.clone(),
                                        value: *value,
                                    },
                                );
                            }
                            arandu_parser::SimpleStmt::Expr { expr, span: _ } => {
                                super::super::synth::synth_expr(checker, **expr);
                            }
                        }
                    }
                    if let Some(cond_expr) = condition {
                        let cond_ty = super::super::synth::synth_expr(checker, **cond_expr);
                        if !cond_ty.is_error()
                            && !super::super::types::unify(
                                &cond_ty,
                                &ArType::Primitive(Primitive::Bool),
                            )
                        {
                            checker.add_constraint(
                                ArType::Primitive(Primitive::Bool),
                                cond_ty,
                                ConstraintOrigin::Condition {
                                    span: checker.pool.expr_span(**cond_expr),
                                },
                            );
                        }
                    }
                    if let Some(step_stmt) = step {
                        match &**step_stmt {
                            arandu_parser::SimpleStmt::VarDecl {
                                span,
                                bindings,
                                value,
                            } => {
                                check_stmt(
                                    checker,
                                    _pool,
                                    &Stmt::VarDecl {
                                        span: *span,
                                        bindings: bindings.clone(),
                                        value: *value,
                                    },
                                );
                            }
                            arandu_parser::SimpleStmt::Set {
                                span,
                                places,
                                op,
                                value,
                            } => {
                                check_stmt(
                                    checker,
                                    _pool,
                                    &Stmt::Set {
                                        span: *span,
                                        places: places.clone(),
                                        op: op.clone(),
                                        value: *value,
                                    },
                                );
                            }
                            arandu_parser::SimpleStmt::Expr { expr, span: _ } => {
                                super::super::synth::synth_expr(checker, **expr);
                            }
                        }
                    }
                }
            }
            checker.ctx.enter_loop();
            check_block(checker, _pool, body);
            checker.ctx.exit_loop();
        }
        Stmt::Match { span: _, expr } => {
            super::super::synth::synth_expr(checker, *expr);
        }
        Stmt::Free { span, expr } => {
            let ty = super::super::synth::synth_expr(checker, *expr);
            if !ty.is_error() && !matches!(ty, ArType::Ptr(_)) {
                checker.diagnostics.push(
                    crate::Diagnostic::error(
                        crate::DiagCode::O011FreeRequiresPtr,
                        format!(
                            "`free` requires a pointer type (`ptr[T]`), found '{}'",
                            ty.display(&checker.symbols)
                        ),
                        *span,
                    )
                    .with_label(
                        checker.pool.expr_span(*expr),
                        format!("expression has type '{}'", ty.display(&checker.symbols)),
                    ),
                );
            }
        }
        Stmt::Defer { span: _, body } | Stmt::ErrDefer { span: _, body } => match body {
            arandu_parser::DeferBody::Block { block, .. } => {
                check_block(checker, _pool, block);
            }
            arandu_parser::DeferBody::Expr { expr, .. } => {
                super::super::synth::synth_expr(checker, **expr);
            }
        },
        Stmt::Break { span } if !checker.ctx.is_in_loop() => {
            checker.diagnostics.push(crate::Diagnostic::error(
                crate::DiagCode::N011BreakContinueOutsideLoop,
                "`break` is only allowed inside a loop",
                *span,
            ));
        }
        Stmt::Continue { span } if !checker.ctx.is_in_loop() => {
            checker.diagnostics.push(crate::Diagnostic::error(
                crate::DiagCode::N011BreakContinueOutsideLoop,
                "`continue` is only allowed inside a loop",
                *span,
            ));
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
        _ => {}
    }
}

fn validate_mutability(checker: &mut TypeChecker<'_>, place: &arandu_parser::Place) {
    if place.suffixes.is_empty() {
        let root_key = crate::NodeKey::from(place.span);
        if let Some(symbol_id) = checker.resolved.value_refs.get(&root_key) {
            let symbol = checker.symbols.get(*symbol_id);
            if (symbol.kind == crate::SymbolKind::Local || symbol.kind == crate::SymbolKind::Param)
                && !checker.resolved.mutable_symbols.contains(symbol_id)
            {
                let name = &symbol.name;
                let replacement = crate::Hint {
                    message: format!("consider declaring the variable as mutable: `let mut {} = ...;`", name),
                    replacement: Some(crate::CodeReplacement {
                        span: symbol.span,
                        new_text: format!("mut {name}"),
                    }),
                };
                let diag = crate::Diagnostic::error(
                    crate::DiagCode::T026CannotAssignImmutable,
                    format!("cannot assign twice to immutable variable '{name}'"),
                    place.span,
                )
                .with_label(place.span, "cannot assign to immutable variable")
                .with_label(symbol.span, "variable declared here as immutable")
                .with_hint_replacement(replacement);
                checker.diagnostics.push(diag);
            }
        }
    }
}

