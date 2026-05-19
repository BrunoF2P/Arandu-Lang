use arandu_parser::{
    Block, CatchHandler, Condition, DeferBody, Expr, ForClause, FuncDecl, LambdaBody, MatchArmBody,
    Program, ResultType, SimpleStmt, Stmt, TopLevelDecl, TypeExpr,
};

use super::TypeChecker;
use super::constraints::ConstraintOrigin;
use super::types::{ArType, Primitive};
use arandu_lexer::Span;

fn contains_any(ty: &TypeExpr) -> Option<Span> {
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

fn validate_expr(checker: &mut TypeChecker, expr: &Expr) {
    match expr {
        Expr::Generic { callee, args, .. } => {
            validate_expr(checker, callee);
            for arg in args {
                if let Some(span) = contains_any(arg) {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T014InvalidVariadicType,
                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                        span,
                    ));
                }
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
            if let Some(span) = contains_any(ty) {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T014InvalidVariadicType,
                    "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                    span,
                ));
            }
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
            if let Some(span) = contains_any(ty) {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T014InvalidVariadicType,
                    "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                    span,
                ));
            }
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
                if let Some(ty) = &param.ty
                    && let Some(span) = contains_any(ty)
                {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T014InvalidVariadicType,
                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                        span,
                    ));
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
                if let Some(ty) = &binding.ty
                    && let Some(span) = contains_any(ty)
                {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T014InvalidVariadicType,
                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                        span,
                    ));
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
                    if let Some(ty) = &binding.ty
                        && let Some(span) = contains_any(ty)
                    {
                        checker.diagnostics.push(crate::Diagnostic::error(
                            crate::DiagCode::T014InvalidVariadicType,
                            "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                            span,
                        ));
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

fn validate_top_level_any(checker: &mut TypeChecker, decl: &TopLevelDecl) {
    match decl {
        TopLevelDecl::Struct(struct_decl) => {
            for field in &struct_decl.fields {
                if let Some(span) = contains_any(&field.ty) {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T014InvalidVariadicType,
                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                        span,
                    ));
                }
            }
        }
        TopLevelDecl::Enum(enum_decl) => {
            for variant in &enum_decl.variants {
                if let Some(payload) = &variant.payload {
                    match payload {
                        arandu_parser::EnumPayload::Tuple { types, .. } => {
                            for ty in types {
                                if let Some(span) = contains_any(ty) {
                                    checker.diagnostics.push(crate::Diagnostic::error(
                                        crate::DiagCode::T014InvalidVariadicType,
                                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                                        span,
                                    ));
                                }
                            }
                        }
                        arandu_parser::EnumPayload::Struct { fields, .. } => {
                            for field in fields {
                                if let Some(span) = contains_any(&field.ty) {
                                    checker.diagnostics.push(crate::Diagnostic::error(
                                        crate::DiagCode::T014InvalidVariadicType,
                                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                                        span,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        TopLevelDecl::TypeAlias(alias_decl) => {
            if let Some(span) = contains_any(&alias_decl.ty) {
                checker.diagnostics.push(crate::Diagnostic::error(
                    crate::DiagCode::T014InvalidVariadicType,
                    "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                    span,
                ));
            }
        }
        TopLevelDecl::Func(func_decl) => {
            for param in &func_decl.params {
                if !param.is_variadic
                    && let Some(span) = contains_any(&param.ty)
                {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T014InvalidVariadicType,
                        "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                        span,
                    ));
                }
            }
            if let Some(result) = &func_decl.result {
                match result {
                    ResultType::Single { ty, .. } => {
                        if let Some(span) = contains_any(ty) {
                            checker.diagnostics.push(crate::Diagnostic::error(
                                crate::DiagCode::T014InvalidVariadicType,
                                "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                                span,
                            ));
                        }
                    }
                    ResultType::Multi { types, .. } => {
                        for ty in types {
                            if let Some(span) = contains_any(ty) {
                                checker.diagnostics.push(crate::Diagnostic::error(
                                    crate::DiagCode::T014InvalidVariadicType,
                                    "any is only allowed in variadic parameters, extern declarations, and compiler builtins",
                                    span,
                                ));
                            }
                        }
                    }
                }
            }
            validate_block(checker, &func_decl.body);
        }
        _ => {}
    }
}

pub fn check_program(checker: &mut TypeChecker, program: &Program) {
    // Populate prelude definitions
    for (module, members_with_types) in [
        (
            "io",
            vec![
                (
                    "println",
                    ArType::Func(
                        vec![ArType::Primitive(Primitive::Any)],
                        Box::new(ArType::Void),
                    ),
                ),
                (
                    "create",
                    ArType::Func(
                        vec![ArType::Primitive(Primitive::Str)],
                        Box::new(ArType::Tuple(vec![
                            ArType::Primitive(Primitive::Any),
                            ArType::Nullable(Box::new(ArType::Err)),
                        ])),
                    ),
                ),
                (
                    "remove",
                    ArType::Func(
                        vec![ArType::Primitive(Primitive::Str)],
                        Box::new(ArType::Nullable(Box::new(ArType::Err))),
                    ),
                ),
            ],
        ),
        (
            "err",
            vec![(
                "new",
                ArType::Func(
                    vec![ArType::Primitive(Primitive::Str)],
                    Box::new(ArType::Err),
                ),
            )],
        ),
    ] {
        for (member, ty) in members_with_types {
            if let Some(symbol_id) = checker.symbols.lookup_module_member(module, member) {
                checker.type_info.decl_types.insert(symbol_id, ty);
            }
        }
    }

    // Pass 1: Collect struct & enum type shapes
    for decl in &program.decls {
        if let TopLevelDecl::Struct(struct_decl) = decl {
            let mut fields = std::collections::HashMap::new();
            for field in &struct_decl.fields {
                let field_ty = super::types::lower_type_expr(
                    &field.ty,
                    &checker.symbols,
                    checker.symbols.global_scope(),
                    &checker.resolved,
                );
                fields.insert(field.name.clone(), field_ty);
            }
            let struct_key = crate::NodeKey::from(struct_decl.span);
            if let Some(symbol_id) = checker.resolved.definitions.get(&struct_key) {
                checker.type_info.struct_fields.insert(*symbol_id, fields);
            }
        } else if let TopLevelDecl::Enum(enum_decl) = decl {
            let enum_key = crate::NodeKey::from(enum_decl.span);
            if let Some(enum_symbol_id) = checker.resolved.definitions.get(&enum_key) {
                for variant in &enum_decl.variants {
                    let shape = match &variant.payload {
                        None => super::EnumPayloadShape::Unit,
                        Some(arandu_parser::EnumPayload::Tuple { types, .. }) => {
                            let tys = types
                                .iter()
                                .map(|ty_expr| {
                                    super::types::lower_type_expr(
                                        ty_expr,
                                        &checker.symbols,
                                        checker.symbols.global_scope(),
                                        &checker.resolved,
                                    )
                                })
                                .collect();
                            super::EnumPayloadShape::Tuple(tys)
                        }
                        _ => super::EnumPayloadShape::Unit,
                    };
                    let variant_key = crate::NodeKey::from(variant.span);
                    if let Some(variant_symbol_id) = checker.resolved.definitions.get(&variant_key)
                    {
                        checker
                            .type_info
                            .enum_variants
                            .insert(*variant_symbol_id, (*enum_symbol_id, shape.clone()));
                    }
                    if let Some(assoc_symbol_id) = checker
                        .symbols
                        .lookup_associated_member(&enum_decl.name, &variant.name)
                    {
                        checker
                            .type_info
                            .enum_variants
                            .insert(assoc_symbol_id, (*enum_symbol_id, shape));
                    }
                }
            }
        }
    }

    // Pass 2: Collect function and extern signature types
    for decl in &program.decls {
        match decl {
            TopLevelDecl::Func(func_decl) => {
                let ret_ty = if let Some(result) = &func_decl.result {
                    super::types::lower_result_type(
                        result,
                        &checker.symbols,
                        checker.symbols.global_scope(),
                        &checker.resolved,
                    )
                } else {
                    ArType::Void
                };

                let mut param_types = Vec::new();
                for param in &func_decl.params {
                    let param_ty = super::types::lower_type_expr(
                        &param.ty,
                        &checker.symbols,
                        checker.symbols.global_scope(),
                        &checker.resolved,
                    );
                    param_types.push(param_ty);
                }

                let name_span = match &func_decl.name {
                    arandu_parser::FuncName::Free { span, .. } => *span,
                    arandu_parser::FuncName::Method { span, .. } => *span,
                };
                let name_key = crate::NodeKey::from(name_span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&name_key) {
                    let func_ty = ArType::Func(param_types, Box::new(ret_ty));
                    checker.type_info.decl_types.insert(*symbol_id, func_ty);
                }
            }
            TopLevelDecl::Extern(extern_decl) => {
                for member in &extern_decl.members {
                    let ret_ty = if let Some(result) = &member.result {
                        super::types::lower_result_type(
                            result,
                            &checker.symbols,
                            checker.symbols.global_scope(),
                            &checker.resolved,
                        )
                    } else {
                        ArType::Void
                    };

                    let mut param_types = Vec::new();
                    for param in &member.params {
                        let param_ty = super::types::lower_type_expr(
                            &param.ty,
                            &checker.symbols,
                            checker.symbols.global_scope(),
                            &checker.resolved,
                        );
                        param_types.push(param_ty);
                    }

                    let name_key = crate::NodeKey::from(member.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&name_key) {
                        let func_ty = ArType::Func(param_types, Box::new(ret_ty));
                        checker.type_info.decl_types.insert(*symbol_id, func_ty);
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 3: Check bodies and constants, and validate any type usage
    for decl in &program.decls {
        validate_top_level_any(checker, decl);

        match decl {
            TopLevelDecl::Func(func_decl) => {
                check_func_body(checker, func_decl);
            }
            TopLevelDecl::Const(const_decl) => {
                let val_ty = super::synth::synth_expr(checker, &const_decl.value);
                let const_key = crate::NodeKey::from(const_decl.span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&const_key) {
                    checker.type_info.decl_types.insert(*symbol_id, val_ty);
                }
            }
            _ => {}
        }
    }
}

pub fn check_func_body(checker: &mut TypeChecker, decl: &FuncDecl) {
    let ret_ty = if let Some(result) = &decl.result {
        super::types::lower_result_type(
            result,
            &checker.symbols,
            checker.symbols.global_scope(),
            &checker.resolved,
        )
    } else {
        ArType::Void
    };

    for param in &decl.params {
        let param_ty = super::types::lower_type_expr(
            &param.ty,
            &checker.symbols,
            checker.symbols.global_scope(),
            &checker.resolved,
        );

        let param_key = crate::NodeKey::from(param.span);
        if let Some(symbol_id) = checker.resolved.definitions.get(&param_key) {
            checker.ctx.bind(*symbol_id, param_ty.clone());
            checker.type_info.decl_types.insert(*symbol_id, param_ty);
        }
    }

    checker.ctx.push_return(ret_ty);
    check_block(checker, &decl.body);
    checker.ctx.pop_return();
}

pub fn check_block(checker: &mut TypeChecker, block: &Block) -> ArType {
    let mut last_ty = ArType::Void;
    let len = block.statements.len();
    for (i, stmt) in block.statements.iter().enumerate() {
        if i == len - 1 {
            if let Stmt::Expr { expr, .. } = stmt {
                last_ty = super::synth::synth_expr(checker, expr);
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

fn synth_place(checker: &mut TypeChecker, place: &arandu_parser::Place) -> ArType {
    // 1. Get the type of the root variable
    let root_key = crate::NodeKey::from(place.span);
    let mut current_ty = if let Some(symbol_id) = checker.resolved.value_refs.get(&root_key) {
        if let Some(ty) = checker.ctx.lookup(*symbol_id) {
            ty.clone()
        } else if let Some(ty) = checker.type_info.decl_types.get(symbol_id) {
            ty.clone()
        } else {
            ArType::Error
        }
    } else {
        ArType::Error
    };

    // 2. Traverse suffixes
    for suffix in &place.suffixes {
        if current_ty.is_error() {
            break;
        }
        match suffix {
            arandu_parser::PlaceSuffix::Field { span, name } => {
                let (actual_base_ty, was_nullable) = match &current_ty {
                    ArType::Nullable(inner) => (inner.as_ref().clone(), true),
                    other => (other.clone(), false),
                };
                if was_nullable {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T006NotNullable,
                        format!(
                            "cannot access field '{}' on nullable type '{}'",
                            name,
                            current_ty.display(&checker.symbols)
                        ),
                        *span,
                    ));
                    current_ty = ArType::Error;
                    break;
                }
                let struct_id_opt = match &actual_base_ty {
                    ArType::Named(id, _) => Some(*id),
                    ArType::Ptr(inner) => match &**inner {
                        ArType::Named(id, _) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(struct_id) = struct_id_opt
                    && let Some(fields) = checker.type_info.struct_fields.get(&struct_id)
                    && let Some(field_ty) = fields.get(name)
                {
                    current_ty = field_ty.clone();
                } else {
                    checker.add_constraint(
                        actual_base_ty.clone(),
                        ArType::Error,
                        ConstraintOrigin::UndefinedField {
                            base_span: place.span,
                            field_span: *span,
                            field_name: name.clone(),
                        },
                    );
                    current_ty = ArType::Error;
                }
            }
            arandu_parser::PlaceSuffix::Index { span, expr } => {
                let index_ty = super::synth::synth_expr(checker, expr);
                let (actual_base_ty, was_nullable) = match &current_ty {
                    ArType::Nullable(inner) => (inner.as_ref().clone(), true),
                    other => (other.clone(), false),
                };
                if was_nullable {
                    checker.diagnostics.push(crate::Diagnostic::error(
                        crate::DiagCode::T006NotNullable,
                        format!(
                            "cannot index nullable type '{}'",
                            current_ty.display(&checker.symbols)
                        ),
                        *span,
                    ));
                    current_ty = ArType::Error;
                    break;
                }
                match &actual_base_ty {
                    ArType::Array(_, inner) | ArType::Slice(inner) => {
                        current_ty = inner.as_ref().clone();
                    }
                    _ => {
                        checker.add_constraint(
                            actual_base_ty.clone(),
                            ArType::Error,
                            ConstraintOrigin::InvalidIndex {
                                base_span: place.span,
                                index_span: expr.span(),
                                is_base_error: true,
                            },
                        );
                        current_ty = ArType::Error;
                    }
                }
                if !index_ty.is_error() && !index_ty.is_integer() {
                    checker.add_constraint(
                        ArType::Primitive(Primitive::Int),
                        index_ty,
                        ConstraintOrigin::InvalidIndex {
                            base_span: place.span,
                            index_span: expr.span(),
                            is_base_error: false,
                        },
                    );
                }
            }
        }
    }

    current_ty
}

pub fn check_stmt(checker: &mut TypeChecker, stmt: &Stmt) {
    match stmt {
        Stmt::VarDecl {
            span: _,
            bindings,
            value,
        } => {
            let val_ty = super::synth::synth_expr(checker, value);

            if bindings.len() > 1 {
                let val_tys = match &val_ty {
                    ArType::Tuple(tys) => tys.clone(),
                    ArType::Error => vec![ArType::Error; bindings.len()],
                    other => vec![other.clone(); bindings.len()],
                };

                for (i, binding) in bindings.iter().enumerate() {
                    let binding_key = crate::NodeKey::from(binding.span);
                    if let Some(symbol_id) = checker.resolved.definitions.get(&binding_key).copied()
                    {
                        let elem_ty = val_tys.get(i).cloned().unwrap_or(ArType::Error);
                        let mut bind_ty = elem_ty.clone();

                        if let Some(ty_expr) = &binding.ty {
                            let expected = super::types::lower_type_expr(
                                ty_expr,
                                &checker.symbols,
                                checker.symbols.global_scope(),
                                &checker.resolved,
                            );

                            if !elem_ty.is_literal()
                                && elem_ty != expected
                                && expected.is_numeric()
                                && elem_ty.is_numeric()
                            {
                                checker.add_constraint(
                                    expected.clone(),
                                    elem_ty.clone(),
                                    ConstraintOrigin::ImplicitWidening {
                                        source_span: value.span(),
                                        target_span: binding.span,
                                    },
                                );
                            } else if !super::types::unify(&expected, &elem_ty) {
                                checker.add_constraint(
                                    expected.clone(),
                                    elem_ty.clone(),
                                    ConstraintOrigin::Assignment {
                                        lhs_span: binding.span,
                                        rhs_span: value.span(),
                                    },
                                );
                            }
                            bind_ty = expected;
                        }

                        checker.ctx.bind(symbol_id, bind_ty.clone());
                        checker.type_info.decl_types.insert(symbol_id, bind_ty);
                    }
                }
            } else if let Some(binding) = bindings.first() {
                let binding_key = crate::NodeKey::from(binding.span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&binding_key).copied() {
                    let mut bind_ty = val_ty.clone();

                    if let Some(ty_expr) = &binding.ty {
                        let expected = super::types::lower_type_expr(
                            ty_expr,
                            &checker.symbols,
                            checker.symbols.global_scope(),
                            &checker.resolved,
                        );

                        if !val_ty.is_literal()
                            && val_ty != expected
                            && expected.is_numeric()
                            && val_ty.is_numeric()
                        {
                            checker.add_constraint(
                                expected.clone(),
                                val_ty.clone(),
                                ConstraintOrigin::ImplicitWidening {
                                    source_span: value.span(),
                                    target_span: binding.span,
                                },
                            );
                        } else if !super::types::unify(&expected, &val_ty) {
                            checker.add_constraint(
                                expected.clone(),
                                val_ty.clone(),
                                ConstraintOrigin::Assignment {
                                    lhs_span: binding.span,
                                    rhs_span: value.span(),
                                },
                            );
                        }
                        bind_ty = expected;
                    }

                    checker.ctx.bind(symbol_id, bind_ty.clone());
                    checker.type_info.decl_types.insert(symbol_id, bind_ty);
                }
            }
        }
        Stmt::Set {
            span: _,
            places,
            op: _,
            value,
        } => {
            let val_ty = super::synth::synth_expr(checker, value);
            if places.len() > 1 {
                let val_tys = match &val_ty {
                    ArType::Tuple(tys) => tys.clone(),
                    ArType::Error => vec![ArType::Error; places.len()],
                    other => vec![other.clone(); places.len()],
                };
                for (i, place) in places.iter().enumerate() {
                    let expected_ty = synth_place(checker, place);
                    let elem_ty = val_tys.get(i).cloned().unwrap_or(ArType::Error);
                    if !super::types::unify(&expected_ty, &elem_ty) {
                        checker.add_constraint(
                            expected_ty.clone(),
                            elem_ty.clone(),
                            ConstraintOrigin::SetTarget {
                                place_span: place.span,
                                value_span: value.span(),
                            },
                        );
                    } else if !elem_ty.is_literal()
                        && elem_ty != expected_ty
                        && expected_ty.is_numeric()
                        && elem_ty.is_numeric()
                    {
                        checker.add_constraint(
                            expected_ty,
                            elem_ty.clone(),
                            ConstraintOrigin::ImplicitWidening {
                                source_span: value.span(),
                                target_span: place.span,
                            },
                        );
                    }
                }
            } else if let Some(place) = places.first() {
                let expected_ty = synth_place(checker, place);
                if !super::types::unify(&expected_ty, &val_ty) {
                    checker.add_constraint(
                        expected_ty.clone(),
                        val_ty.clone(),
                        ConstraintOrigin::SetTarget {
                            place_span: place.span,
                            value_span: value.span(),
                        },
                    );
                } else if !val_ty.is_literal()
                    && val_ty != expected_ty
                    && expected_ty.is_numeric()
                    && val_ty.is_numeric()
                {
                    checker.add_constraint(
                        expected_ty,
                        val_ty.clone(),
                        ConstraintOrigin::ImplicitWidening {
                            source_span: value.span(),
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
                super::synth::synth_expr(checker, &values[0])
            } else {
                let tys = values
                    .iter()
                    .map(|v| super::synth::synth_expr(checker, v))
                    .collect();
                ArType::Tuple(tys)
            };

            if !super::types::unify(&current_ret, &val_ty) {
                checker.add_constraint(
                    current_ret,
                    val_ty,
                    ConstraintOrigin::ReturnType {
                        return_span: *span,
                        declared_span: *span, // should be func decl span
                    },
                );
            } else if !val_ty.is_literal()
                && val_ty != current_ret
                && current_ret.is_numeric()
                && val_ty.is_numeric()
            {
                checker.add_constraint(
                    current_ret,
                    val_ty.clone(),
                    ConstraintOrigin::ImplicitWidening {
                        source_span: values.first().map(|v| v.span()).unwrap_or(*span),
                        target_span: *span,
                    },
                );
            }
        }
        Stmt::Expr { expr, .. } => {
            super::synth::synth_expr(checker, expr);
        }
        Stmt::If {
            span: _,
            condition,
            then_block,
            else_block,
        } => {
            check_condition(checker, condition);
            check_block(checker, then_block);
            if let Some(eb) = else_block {
                check_block(checker, eb);
            }
        }
        Stmt::While {
            span: _,
            condition,
            body,
        } => {
            check_condition(checker, condition);
            checker.ctx.enter_loop();
            check_block(checker, body);
            checker.ctx.exit_loop();
        }
        Stmt::For {
            span: _,
            clause: _,
            body,
        } => {
            checker.ctx.enter_loop();
            check_block(checker, body);
            checker.ctx.exit_loop();
        }
        Stmt::Match { span: _, expr } => {
            // expr is always an Expr::Match. We just synth it and discard the result
            super::synth::synth_expr(checker, expr);
        }
        Stmt::Free { span: _, expr } => {
            let _ty = super::synth::synth_expr(checker, expr);
            // Assert ty is Ptr(T)
        }
        Stmt::Defer { span: _, body } | Stmt::ErrDefer { span: _, body } => match body {
            arandu_parser::DeferBody::Block { block, .. } => {
                check_block(checker, block);
            }
            arandu_parser::DeferBody::Expr { expr, .. } => {
                super::synth::synth_expr(checker, expr);
            }
        },
        Stmt::Break { span: _ } | Stmt::Continue { span: _ } if !checker.ctx.is_in_loop() => {
            // Must be inside a loop
        }
        Stmt::Break { .. } | Stmt::Continue { .. } => {}
        _ => {}
    }
}

pub fn check_condition(checker: &mut TypeChecker, condition: &arandu_parser::Condition) {
    match condition {
        arandu_parser::Condition::Expr { expr, span } => {
            let cond_ty = super::synth::synth_expr(checker, expr);
            if !cond_ty.is_error()
                && !super::types::unify(&cond_ty, &ArType::Primitive(Primitive::Bool))
            {
                checker.add_constraint(
                    ArType::Primitive(Primitive::Bool),
                    cond_ty,
                    ConstraintOrigin::Condition { span: *span },
                );
            }
        }
        arandu_parser::Condition::Is {
            expr,
            pattern,
            span: _,
        } => {
            let cond_ty = super::synth::synth_expr(checker, expr);
            super::synth::check_pattern(checker, pattern, &cond_ty);
        }
    }
}
