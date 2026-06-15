use super::ast_pool::AstPool;
use super::{
    Attribute, BinaryOp, BindingItem, Block, CatchHandler, Condition, ConstDecl, DeferBody,
    EnumDecl, EnumPayload, ExprId, ExprKind, ExternDecl, ForClause, FuncDecl, FuncName,
    FuncSignature, GenericParam, ImportDecl, InterfaceDecl, LambdaBody, MatchArm, MatchArmBody,
    Ownership, Param, Pattern, Place, PlaceSuffix, Program, ResultType, SetOp, SimpleStmt, Stmt,
    StringPart, StructDecl, TopLevelDecl, TypeAliasDecl, TypeExpr, TypeName, UnaryOp, Visibility,
    WhereItem,
};
use arandu_lexer::Span;
use std::fmt::Write;
use std::cell::RefCell;

thread_local! {
    static CURRENT_LINE_INDEX: RefCell<Option<arandu_base::line_index::LineIndex>> = const { RefCell::new(None) };
}

pub fn dump_program(program: &Program, line_index: &arandu_base::line_index::LineIndex) -> String {
    CURRENT_LINE_INDEX.with(|cell| {
        *cell.borrow_mut() = Some(line_index.clone());
    });

    let mut out = Vec::new();
    out.push(format!("Program {}", dump_span(program.span)));
    if let Some(module) = &program.module {
        out.push(format!(
            "  Module {} {}",
            dump_span(module.span),
            module.path.join(".")
        ));
    }
    for import in &program.imports {
        out.push(format!("  {}", dump_import(&program.pool, import)));
    }
    for decl in &program.decls {
        dump_top_level_decl(&program.pool, program.pool.decl(*decl), &mut out);
    }

    CURRENT_LINE_INDEX.with(|cell| {
        *cell.borrow_mut() = None;
    });

    out.join("\n")
}

fn dump_span(span: Span) -> String {
    CURRENT_LINE_INDEX.with(|cell| {
        if let Some(line_index) = &*cell.borrow() {
            let (start_line, start_col) = line_index.line_col(span.start);
            let (end_line, end_col) = line_index.line_col(span.end);
            format!(
                "@{}:{}-{}:{}",
                start_line, start_col, end_line, end_col
            )
        } else {
            format!("@{}:{}-{}:{}", 1, span.start, 1, span.end)
        }
    })
}

fn dump_top_level_decl(pool: &AstPool, decl: &TopLevelDecl, out: &mut Vec<String>) {
    match decl {
        TopLevelDecl::Const(decl) => dump_const(pool, decl, out),
        TopLevelDecl::TypeAlias(decl) => dump_type_alias(pool, decl, out),
        TopLevelDecl::Func(func) => dump_func(pool, func, out),
        TopLevelDecl::Struct(decl) => dump_struct(pool, decl, out),
        TopLevelDecl::Enum(decl) => dump_enum(pool, decl, out),
        TopLevelDecl::Interface(decl) => dump_interface(pool, decl, out),
        TopLevelDecl::Extern(decl) => dump_extern(pool, decl, out),
        TopLevelDecl::Error(span) => out.push(format!("  DeclError {}", dump_span(*span))),
    }
}

fn dump_const(pool: &AstPool, decl: &ConstDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &decl.attrs, out, 2);
    let ty = decl
        .ty
        .map(|ty| format!(" {}", dump_type(pool.type_expr(ty), pool)))
        .unwrap_or_default();
    out.push(format!(
        "  Const {} {}{}{ty} = {}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_expr(pool, decl.value)
    ));
}

fn dump_type_alias(pool: &AstPool, decl: &TypeAliasDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &decl.attrs, out, 2);
    out.push(format!(
        "  Type {} {}{}{} = {}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_type(pool.type_expr(decl.ty), pool)
    ));
}

fn dump_func(pool: &AstPool, func: &FuncDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &func.attrs, out, 2);
    let params = func
        .params
        .iter()
        .map(|param| dump_param(pool, param))
        .collect::<Vec<_>>()
        .join(", ");
    let result = func
        .result
        .as_ref()
        .map_or_else(|| "void".to_string(), |r| dump_result_type(r, pool));
    let mut modifiers = Vec::new();
    if func.visibility == Visibility::Public {
        modifiers.push("public");
    }
    if func.is_async {
        modifiers.push("async");
    }
    let modifiers = if modifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", modifiers.join(" "))
    };
    out.push(format!(
        "  Func {} {modifiers}{}{}({}) -> {}{}",
        dump_span(func.span),
        dump_func_name(&func.name),
        dump_generic_params(&func.generic_params),
        params,
        result,
        dump_where_clause(&func.where_clause)
    ));
    dump_block_body(pool, &func.body, out, 4);
}

fn dump_struct(pool: &AstPool, decl: &StructDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &decl.attrs, out, 2);
    out.push(format!(
        "  Struct {} {}{}{}{}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_where_clause(&decl.where_clause)
    ));
    for field in &decl.fields {
        dump_attrs(pool, &field.attrs, out, 4);
        out.push(format!(
            "    Field {} {}{} {}",
            dump_span(field.span),
            dump_visibility(field.visibility),
            field.name,
            dump_type(pool.type_expr(field.ty), pool)
        ));
    }
}

fn dump_enum(pool: &AstPool, decl: &EnumDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &decl.attrs, out, 2);
    out.push(format!(
        "  Enum {} {}{}{}{}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_where_clause(&decl.where_clause)
    ));
    for variant in &decl.variants {
        dump_attrs(pool, &variant.attrs, out, 4);
        let payload = match &variant.payload {
            None => String::new(),
            Some(EnumPayload::Tuple { types, .. }) => {
                let list = pool.type_expr_list(*types);
                let types_str = list.iter().map(|&ty| dump_type(pool.type_expr(ty), pool)).collect::<Vec<_>>().join(", ");
                format!("({types_str})")
            }
            Some(EnumPayload::Struct { fields, .. }) => {
                let fields = fields
                    .iter()
                    .map(|field| format!("{} {}", field.name, dump_type(pool.type_expr(field.ty), pool)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" {{ {fields} }}")
            }
        };
        out.push(format!(
            "    Variant {} {}{payload}",
            dump_span(variant.span),
            variant.name
        ));
    }
}

fn dump_interface(pool: &AstPool, decl: &InterfaceDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &decl.attrs, out, 2);
    out.push(format!(
        "  Interface {} {}{}{}{}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_where_clause(&decl.where_clause)
    ));
    for member in &decl.members {
        dump_signature(pool, member, out, 4);
    }
}

fn dump_extern(pool: &AstPool, decl: &ExternDecl, out: &mut Vec<String>) {
    dump_attrs(pool, &decl.attrs, out, 2);
    out.push(format!(
        "  Extern {} \"{}\"",
        dump_span(decl.span),
        decl.abi
    ));
    for member in &decl.members {
        dump_signature(pool, member, out, 4);
    }
}

fn dump_signature(pool: &AstPool, signature: &FuncSignature, out: &mut Vec<String>, indent: usize) {
    dump_attrs(pool, &signature.attrs, out, indent);
    let pad = " ".repeat(indent);
    let params = signature
        .params
        .iter()
        .map(|param| dump_param(pool, param))
        .collect::<Vec<_>>()
        .join(", ");
    let result = signature
        .result
        .as_ref()
        .map_or_else(|| "void".to_string(), |r| dump_result_type(r, pool));
    out.push(format!(
        "{pad}Signature {} {}{}({}) -> {}{}",
        dump_span(signature.span),
        signature.name,
        dump_generic_params(&signature.generic_params),
        params,
        result,
        dump_where_clause(&signature.where_clause)
    ));
}

fn dump_attrs(pool: &AstPool, attrs: &[Attribute], out: &mut Vec<String>, indent: usize) {
    let pad = " ".repeat(indent);
    for attr in attrs {
        if attr.args.is_empty() {
            out.push(format!("{pad}Attr {} {}", dump_span(attr.span), attr.name));
        } else {
            let args = attr
                .args
                .iter()
                .map(|arg| dump_expr(pool, *arg))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "{pad}Attr {} {}({args})",
                dump_span(attr.span),
                attr.name
            ));
        }
    }
}

fn dump_block_body(pool: &AstPool, block: &Block, out: &mut Vec<String>, indent: usize) {
    for stmt in &block.statements {
        dump_stmt(pool, *stmt, out, indent);
    }
}

fn dump_stmt(pool: &AstPool, stmt: crate::ast_pool::StmtId, out: &mut Vec<String>, indent: usize) {
    let stmt = pool.stmt(stmt);
    let pad = " ".repeat(indent);
    match stmt {
        Stmt::VarDecl {
            span,
            bindings,
            value,
        } => {
            let bindings = bindings
                .iter()
                .map(|b| dump_binding(pool, b))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "{pad}Var {} {bindings} = {}",
                dump_span(*span),
                dump_expr(pool, *value)
            ));
        }
        Stmt::Set {
            span,
            places,
            op,
            value,
        } => {
            let places = places
                .iter()
                .map(|p| dump_place(pool, p))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "{pad}Set {} {places} {} {}",
                dump_span(*span),
                dump_set_op(op),
                dump_expr(pool, *value)
            ));
        }
        Stmt::Return { span, values } => {
            let values = values
                .iter()
                .map(|val| dump_expr(pool, *val))
                .collect::<Vec<_>>()
                .join(", ");
            if values.is_empty() {
                out.push(format!("{pad}Return {}", dump_span(*span)));
            } else {
                out.push(format!("{pad}Return {} {values}", dump_span(*span)));
            }
        }
        Stmt::Break { span } => out.push(format!("{pad}Break {}", dump_span(*span))),
        Stmt::Continue { span } => out.push(format!("{pad}Continue {}", dump_span(*span))),
        Stmt::Free { span, expr } => {
            out.push(format!(
                "{pad}Free {} {}",
                dump_span(*span),
                dump_expr(pool, *expr)
            ));
        }
        Stmt::Expr { span, expr } => {
            out.push(format!(
                "{pad}Expr {} {}",
                dump_span(*span),
                dump_expr(pool, *expr)
            ));
        }
        Stmt::If {
            span,
            condition,
            then_block,
            else_block,
        } => {
            out.push(format!(
                "{pad}If {} {}",
                dump_span(*span),
                dump_condition(pool, condition)
            ));
            dump_block_body(pool, then_block, out, indent + 2);
            if let Some(else_block) = else_block {
                out.push(format!("{pad}Else {}", dump_span(else_block.span)));
                dump_block_body(pool, else_block, out, indent + 2);
            }
        }
        Stmt::For { span, clause, body } => {
            out.push(format!(
                "{pad}For {}{}",
                dump_span(*span),
                dump_for_clause(pool, clause)
            ));
            dump_block_body(pool, body, out, indent + 2);
        }
        Stmt::While {
            span,
            condition,
            body,
        } => {
            out.push(format!(
                "{pad}While {} {}",
                dump_span(*span),
                dump_condition(pool, condition)
            ));
            dump_block_body(pool, body, out, indent + 2);
        }
        Stmt::Match { span, expr } => {
            out.push(format!(
                "{pad}MatchStmt {} {}",
                dump_span(*span),
                dump_expr(pool, *expr)
            ));
        }
        Stmt::Defer { span, body } => dump_defer_body(pool, "Defer", *span, body, out, indent),
        Stmt::ErrDefer { span, body } => {
            dump_defer_body(pool, "ErrDefer", *span, body, out, indent)
        }
        Stmt::Unsafe { span, block } => {
            out.push(format!("{pad}Unsafe {}", dump_span(*span)));
            dump_block_body(pool, block, out, indent + 2);
        }
        Stmt::Error(span) => out.push(format!("{pad}StmtError {}", dump_span(*span))),
    }
}

fn dump_condition(pool: &AstPool, condition: &Condition) -> String {
    match condition {
        Condition::Expr { span, expr } => {
            format!("Condition {} {}", dump_span(*span), dump_expr(pool, *expr))
        }
        Condition::Is {
            span,
            expr,
            pattern,
        } => {
            format!(
                "Is {} ({}, {})",
                dump_span(*span),
                dump_expr(pool, *expr),
                dump_pattern(pool, pool.pattern(*pattern))
            )
        }
    }
}

fn dump_for_clause(pool: &AstPool, clause: &ForClause) -> String {
    match clause {
        ForClause::In {
            span,
            bindings,
            iterable,
        } => {
            let bindings = bindings
                .iter()
                .map(|binding| {
                    if binding.mutable {
                        format!("mut {}", binding.name)
                    } else {
                        binding.name.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                " In {} {bindings} in {}",
                dump_span(*span),
                dump_expr(pool, *iterable)
            )
        }
        ForClause::CStyle {
            span,
            init,
            condition,
            step,
        } => format!(
            " C {} init {}; condition {}; step {}",
            dump_span(*span),
            init.as_ref()
                .map_or_else(|| "none".to_string(), |stmt| dump_simple_stmt(pool, stmt)),
            condition
                .as_ref()
                .map_or_else(|| "none".to_string(), |expr| dump_expr(pool, *expr)),
            step.as_ref()
                .map_or_else(|| "none".to_string(), |stmt| dump_simple_stmt(pool, stmt))
        ),
    }
}

fn dump_simple_stmt(pool: &AstPool, stmt: &SimpleStmt) -> String {
    match stmt {
        SimpleStmt::VarDecl {
            span,
            bindings,
            value,
        } => {
            let bindings = bindings
                .iter()
                .map(|b| dump_binding(pool, b))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Var {} {bindings} = {}",
                dump_span(*span),
                dump_expr(pool, *value)
            )
        }
        SimpleStmt::Set {
            span,
            places,
            op,
            value,
        } => {
            let places = places
                .iter()
                .map(|p| dump_place(pool, p))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Set {} {places} {} {}",
                dump_span(*span),
                dump_set_op(op),
                dump_expr(pool, *value)
            )
        }
        SimpleStmt::Expr { span, expr } => {
            format!("Expr {} {}", dump_span(*span), dump_expr(pool, *expr))
        }
    }
}

fn dump_defer_body(
    pool: &AstPool,
    label: &str,
    span: Span,
    body: &DeferBody,
    out: &mut Vec<String>,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    match body {
        DeferBody::Expr { expr, .. } => {
            out.push(format!(
                "{pad}{label} {} Expr({})",
                dump_span(span),
                dump_expr(pool, *expr)
            ));
        }
        DeferBody::Block { block, .. } => {
            out.push(format!(
                "{pad}{label} {} Block {}",
                dump_span(span),
                dump_span(block.span)
            ));
            dump_block_body(pool, block, out, indent + 2);
        }
    }
}

fn dump_binding(pool: &AstPool, binding: &BindingItem) -> String {
    let mut out = format!("{} ", dump_span(binding.span));
    if binding.mutable {
        out.push_str("mut ");
    }
    out.push_str(&binding.name);
    if let Some(ty) = &binding.ty {
        out.push(' ');
        out.push_str(&dump_type(pool.type_expr(*ty), pool));
    }
    out
}

fn dump_place(pool: &AstPool, place: &Place) -> String {
    let mut out = format!("{} {}", dump_span(place.span), place.root);
    for suffix in &place.suffixes {
        match suffix {
            PlaceSuffix::Field { name, .. } => {
                out.push('.');
                out.push_str(name);
            }
            PlaceSuffix::Index { expr, .. } => {
                out.push('[');
                out.push_str(&dump_expr(pool, *expr));
                out.push(']');
            }
        }
    }
    out
}

fn dump_import(_pool: &AstPool, import: &ImportDecl) -> String {
    match import {
        ImportDecl::Module { span, path } => {
            format!("Import {} {}", dump_span(*span), path.join("."))
        }
        ImportDecl::Named { span, items, from } => {
            let items = items
                .iter()
                .map(|item| match &item.alias {
                    Some(alias) => format!("{} {} as {alias}", dump_span(item.span), item.name),
                    None => format!("{} {}", dump_span(item.span), item.name),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Import {} {{ {items} }} from {}",
                dump_span(*span),
                from.join(".")
            )
        }
    }
}

fn dump_param(pool: &AstPool, param: &Param) -> String {
    let mut out = format!("{} ", dump_span(param.span));
    if let Some(ownership) = param.ownership {
        out.push_str(match ownership {
            Ownership::Own => "own ",
            Ownership::Mut => "mut ",
            Ownership::Shared => "shared ",
        });
    }
    out.push_str(&param.name);
    out.push(' ');
    out.push_str(&dump_type(pool.type_expr(param.ty), pool));
    if param.is_variadic {
        out.push_str("...");
    }
    out
}

fn dump_func_name(name: &FuncName) -> String {
    match name {
        FuncName::Free { name, .. } => name.clone(),
        FuncName::Method { receiver, name, .. } => {
            format!("{}.{}", dump_type_name(receiver), name)
        }
    }
}

fn dump_result_type(result: &ResultType, pool: &AstPool) -> String {
    match result {
        ResultType::Single { ty, .. } => dump_type(pool.type_expr(*ty), pool),
        ResultType::Multi { types, .. } => {
            let list = pool.type_expr_list(*types);
            let inner = list.iter().map(|&t| dump_type(pool.type_expr(t), pool)).collect::<Vec<_>>().join(", ");
            format!("({inner})")
        }
    }
}

fn dump_type(ty: &TypeExpr, pool: &AstPool) -> String {
    match ty {
        TypeExpr::Primitive { span, name } => format!("Type {} {name}", dump_span(*span)),
        TypeExpr::Named { span, name, args } => {
            let mut out = format!("Type {} {}", dump_span(*span), dump_type_name(name));
            let arg_list = pool.type_expr_list(*args);
            if !arg_list.is_empty() {
                let args_str = arg_list.iter().map(|&arg| dump_type(pool.type_expr(arg), pool)).collect::<Vec<_>>().join(", ");
                let _ = write!(out, "<{args_str}>");
            }
            out
        }
        TypeExpr::Nullable { span, inner } => {
            format!("Nullable {} {}", dump_span(*span), dump_type(pool.type_expr(*inner), pool))
        }
        TypeExpr::Pointer { span, inner } => {
            format!("Ptr {} [{}]", dump_span(*span), dump_type(pool.type_expr(*inner), pool))
        }
        TypeExpr::Slice { span, inner } => {
            format!("Slice {} {}", dump_span(*span), dump_type(pool.type_expr(*inner), pool))
        }
        TypeExpr::Array { span, size, elem } => {
            format!("ArrayType {} [{size}]{}", dump_span(*span), dump_type(pool.type_expr(*elem), pool))
        }
        TypeExpr::Func {
            span,
            params,
            result,
        } => {
            let param_list = pool.type_expr_list(*params);
            let params_str = param_list.iter().map(|&p| dump_type(pool.type_expr(p), pool)).collect::<Vec<_>>().join(", ");
            match result {
                Some(result) => {
                    format!(
                        "FuncType {} ({params_str}) {}",
                        dump_span(*span),
                        dump_result_type(result, pool)
                    )
                }
                None => format!("FuncType {} ({params_str})", dump_span(*span)),
            }
        }
        TypeExpr::Group { span, inner } => {
            format!("GroupType {} ({})", dump_span(*span), dump_type(pool.type_expr(*inner), pool))
        }
    }
}

fn dump_type_name(name: &TypeName) -> String {
    format!("{} {}", dump_span(name.span), name.path.join("."))
}

fn dump_expr(pool: &AstPool, expr: ExprId) -> String {
    let span = pool.expr_span(expr);
    match pool.expr(expr) {
        ExprKind::Path { path } => format!("Path {}({})", dump_span(span), path.join(".")),
        ExprKind::TypePath { type_name, member } => {
            format!(
                "TypePath {}({}.{})",
                dump_span(span),
                dump_type_name(type_name),
                member
            )
        }
        ExprKind::Generic { callee, args } => {
            let type_expr_ids = pool.type_expr_list(*args);
            let args_str = type_expr_ids
                .iter()
                .map(|id| dump_type(pool.type_expr(*id), pool))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Generic {}({}, <{args_str}>)",
                dump_span(span),
                dump_expr(pool, *callee)
            )
        }
        ExprKind::Field { base, field } => {
            format!(
                "Field {}({}, {field})",
                dump_span(span),
                dump_expr(pool, *base)
            )
        }
        ExprKind::SafeField { base, field } => {
            format!(
                "SafeField {}({}, {field})",
                dump_span(span),
                dump_expr(pool, *base)
            )
        }
        ExprKind::Index { base, index } => {
            format!(
                "Index {}({}, {})",
                dump_span(span),
                dump_expr(pool, *base),
                dump_expr(pool, *index)
            )
        }
        ExprKind::SafeIndex { base, index } => {
            format!(
                "SafeIndex {}({}, {})",
                dump_span(span),
                dump_expr(pool, *base),
                dump_expr(pool, *index)
            )
        }
        ExprKind::Try { expr } => format!("Try {}({})", dump_span(span), dump_expr(pool, *expr)),
        ExprKind::Call {
            callee,
            args,
            trailing_block,
        } => {
            let arg_ids = pool.expr_list(*args);
            let block = trailing_block.map(|block_id| pool.block(block_id));
            dump_call(pool, span, *callee, arg_ids, block)
        }
        ExprKind::StructLiteral { ty, fields } => {
            let field_init_ids = pool.field_init_list(*fields);
            let fields_str = field_init_ids
                .iter()
                .map(|id| {
                    let field = pool.field_init(*id);
                    format!(
                        "{} {}: {}",
                        dump_span(field.span),
                        field.name,
                        dump_expr(pool, field.value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "StructLiteral {}({}, [{fields_str}])",
                dump_span(span),
                dump_type(pool.type_expr(*ty), pool)
            )
        }
        ExprKind::Array { items } => {
            let item_ids = pool.expr_list(*items);
            let items_str = item_ids
                .iter()
                .map(|item| dump_expr(pool, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("Array {}([{items_str}])", dump_span(span))
        }
        ExprKind::Lambda { params, body } => {
            let param_ids = pool.lambda_param_list(*params);
            let params_str = param_ids
                .iter()
                .map(|id| {
                    let param = pool.lambda_param(*id);
                    match &param.ty {
                        Some(ty) => {
                            format!("{} {} {}", dump_span(param.span), param.name, dump_type(pool.type_expr(*ty), pool))
                        }
                        None => format!("{} {}", dump_span(param.span), param.name),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Lambda {}([{params_str}], {})",
                dump_span(span),
                dump_lambda_body(pool, body)
            )
        }
        ExprKind::Alloc { expr } => {
            format!("Alloc {}({})", dump_span(span), dump_expr(pool, *expr))
        }
        ExprKind::AsyncBlock { block } => {
            dump_inline_block(pool, "AsyncBlock", span, pool.block(*block))
        }
        ExprKind::UnsafeBlock { block } => {
            dump_inline_block(pool, "UnsafeBlock", span, pool.block(*block))
        }
        ExprKind::If {
            condition,
            then_block,
            else_block,
        } => {
            format!(
                "IfExpr {}({}, {}, {})",
                dump_span(span),
                dump_condition(pool, condition),
                dump_block_inline(pool, pool.block(*then_block)),
                dump_block_inline(pool, pool.block(*else_block))
            )
        }
        ExprKind::Match { value, arms } => {
            let arm_ids = pool.match_arm_list(*arms);
            let arms_str = arm_ids
                .iter()
                .map(|id| dump_match_arm(pool, pool.match_arm(*id)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Match {}({}, [{arms_str}])",
                dump_span(span),
                dump_expr(pool, *value)
            )
        }
        ExprKind::Catch { expr, handler } => {
            format!(
                "Catch {}({}, {})",
                dump_span(span),
                dump_expr(pool, *expr),
                dump_catch_handler(pool, pool.catch_handler(*handler))
            )
        }
        ExprKind::NullCoalesce { left, right } => {
            format!(
                "NullCoalesce {}({}, {})",
                dump_span(span),
                dump_expr(pool, *left),
                dump_expr(pool, *right)
            )
        }
        ExprKind::Cast { expr, ty } => {
            format!(
                "Cast {}({}, {})",
                dump_span(span),
                dump_expr(pool, *expr),
                dump_type(pool.type_expr(*ty), pool)
            )
        }
        ExprKind::Group { expr } => {
            format!("Group {}({})", dump_span(span), dump_expr(pool, *expr))
        }
        ExprKind::Unary { op, expr } => {
            format!(
                "Unary {}({}, {})",
                dump_span(span),
                dump_unary(*op),
                dump_expr(pool, *expr)
            )
        }
        ExprKind::Binary { op, left, right } => {
            format!(
                "Binary {}({}, {}, {})",
                dump_span(span),
                dump_binary(*op),
                dump_expr(pool, *left),
                dump_expr(pool, *right)
            )
        }
        ExprKind::Int { value } => format!("Int {}({value})", dump_span(span)),
        ExprKind::Float { value } => format!("Float {}({value})", dump_span(span)),
        ExprKind::Bool { value } => format!("Bool {}({value})", dump_span(span)),
        ExprKind::Char { value } => format!("Char {}('{value}')", dump_span(span)),
        ExprKind::InterpolatedString { parts } => {
            let part_ids = pool.string_part_list(*parts);
            let parts_resolved: Vec<StringPart> = part_ids
                .iter()
                .map(|id| pool.string_part(*id).clone())
                .collect();
            dump_interpolated_string(pool, span, &parts_resolved)
        }
        ExprKind::Nil => format!("Nil {}", dump_span(span)),
        ExprKind::Error => format!("ExprError {}", dump_span(span)),
    }
}

fn dump_call(
    pool: &AstPool,
    span: Span,
    callee: ExprId,
    args: &[ExprId],
    trailing_block: Option<&Block>,
) -> String {
    let args_str = args
        .iter()
        .map(|arg| dump_expr(pool, *arg))
        .collect::<Vec<_>>()
        .join(", ");
    match trailing_block {
        Some(block) => format!(
            "Call {}({}, [{args_str}], {})",
            dump_span(span),
            dump_expr(pool, callee),
            dump_block_inline(pool, block)
        ),
        None => format!(
            "Call {}({}, [{args_str}])",
            dump_span(span),
            dump_expr(pool, callee)
        ),
    }
}

fn dump_lambda_body(pool: &AstPool, body: &LambdaBody) -> String {
    match body {
        LambdaBody::Expr { expr, .. } => format!("Expr({})", dump_expr(pool, *expr)),
        LambdaBody::Block { block, .. } => dump_block_inline(pool, block),
    }
}

fn dump_catch_handler(pool: &AstPool, handler: &CatchHandler) -> String {
    match handler {
        CatchHandler::Expr { expr, .. } => format!("Expr({})", dump_expr(pool, *expr)),
        CatchHandler::Block { error, block, .. } => {
            format!("Handler({error}, {})", dump_block_inline(pool, block))
        }
    }
}

fn dump_inline_block(pool: &AstPool, label: &str, span: Span, block: &Block) -> String {
    format!(
        "{label} {} {}",
        dump_span(span),
        dump_block_inline(pool, block)
    )
}

fn dump_block_inline(pool: &AstPool, block: &Block) -> String {
    let stmts = block
        .statements
        .iter()
        .map(|stmt| dump_stmt_inline(pool, *stmt))
        .collect::<Vec<_>>()
        .join("; ");
    format!("Block {}[{stmts}]", dump_span(block.span))
}

fn dump_stmt_inline(pool: &AstPool, stmt: crate::ast_pool::StmtId) -> String {
    let mut out = Vec::new();
    dump_stmt(pool, stmt, &mut out, 0);
    out.join(" ")
}

fn dump_match_arm(pool: &AstPool, arm: &MatchArm) -> String {
    format!(
        "Arm {} {}{} => {}",
        dump_span(arm.span),
        dump_pattern(pool, pool.pattern(arm.pattern)),
        arm.guard
            .as_ref()
            .map(|guard| format!(" if {}", dump_expr(pool, *guard)))
            .unwrap_or_default(),
        dump_match_arm_body(pool, &arm.body)
    )
}

fn dump_match_arm_body(pool: &AstPool, body: &MatchArmBody) -> String {
    match body {
        MatchArmBody::Expr { expr, .. } => dump_expr(pool, *expr),
        MatchArmBody::Block { block, .. } => dump_block_inline(pool, block),
    }
}

fn dump_pattern(pool: &AstPool, pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard { span } => format!("Wildcard {}", dump_span(*span)),
        Pattern::Bind { span, name } => format!("Bind {}({name})", dump_span(*span)),
        Pattern::Literal { span, expr } => {
            format!("Literal {}({})", dump_span(*span), dump_expr(pool, *expr))
        }
        Pattern::Enum {
            span,
            type_name,
            variant,
            payload,
        } => {
            let payload_str = pool.pattern_list(*payload)
                .iter()
                .map(|&p| dump_pattern(pool, pool.pattern(p)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "EnumPattern {}({}.{}, [{payload_str}])",
                dump_span(*span),
                dump_type_name(type_name),
                variant
            )
        }
        Pattern::TypeTuple {
            span,
            name,
            payload,
        } => {
            let payload_str = pool.pattern_list(*payload)
                .iter()
                .map(|&p| dump_pattern(pool, pool.pattern(p)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("TypePattern {}({name}, [{payload_str}])", dump_span(*span))
        }
        Pattern::Struct {
            span,
            type_name,
            fields,
        } => {
            let fields_str = pool.field_pattern_list(*fields)
                .iter()
                .map(|&field_id| {
                    let field = pool.field_pattern(field_id);
                    match &field.pattern {
                        Some(pat_id) => {
                            format!(
                                "{} {}: {}",
                                dump_span(field.span),
                                field.name,
                                dump_pattern(pool, pool.pattern(*pat_id))
                            )
                        }
                        None => format!("{} {}", dump_span(field.span), field.name),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "StructPattern {}({}, [{fields_str}])",
                dump_span(*span),
                dump_type_name(type_name)
            )
        }
        Pattern::Tuple { span, items } => {
            let items_str = pool.pattern_list(*items)
                .iter()
                .map(|&item| dump_pattern(pool, pool.pattern(item)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("TuplePattern {}([{items_str}])", dump_span(*span))
        }
        Pattern::Range {
            span,
            start,
            inclusive,
            end,
        } => {
            let op = if *inclusive { "..=" } else { ".." };
            format!(
                "RangePattern {}({}, {op}, {})",
                dump_span(*span),
                dump_expr(pool, *start),
                dump_expr(pool, *end)
            )
        }
    }
}

fn dump_interpolated_string(pool: &AstPool, span: Span, parts: &[StringPart]) -> String {
    if let [StringPart::Text { text, .. }] = parts {
        return format!("String {}(\"{text}\")", dump_span(span));
    }

    let parts_str = parts
        .iter()
        .map(|part| match part {
            StringPart::Text { span, text } => format!("Text {}(\"{text}\")", dump_span(*span)),
            StringPart::Expr { span, expr } => {
                format!("Expr {}({})", dump_span(*span), dump_expr(pool, *expr))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("InterpolatedString {}([{parts_str}])", dump_span(span))
}

fn dump_generic_params(params: &[GenericParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let params_str = params
        .iter()
        .map(|param| {
            if param.constraints.is_empty() {
                format!("{} {}", dump_span(param.span), param.name)
            } else {
                let constraints = param
                    .constraints
                    .iter()
                    .map(dump_type_name)
                    .collect::<Vec<_>>()
                    .join(" + ");
                format!("{} {}: {constraints}", dump_span(param.span), param.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{params_str}>")
}

fn dump_where_clause(where_clause: &[WhereItem]) -> String {
    if where_clause.is_empty() {
        return String::new();
    }
    let items = where_clause
        .iter()
        .map(|item| {
            let constraints = item
                .constraints
                .iter()
                .map(dump_type_name)
                .collect::<Vec<_>>()
                .join(" + ");
            format!("{} {}: {constraints}", dump_span(item.span), item.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(" where {items}")
}

fn dump_visibility(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Private => "",
        Visibility::Public => "public ",
    }
}

fn dump_set_op(op: &SetOp) -> &'static str {
    match op {
        SetOp::Assign => "=",
        SetOp::AddAssign => "+=",
        SetOp::SubAssign => "-=",
        SetOp::MulAssign => "*=",
        SetOp::DivAssign => "/=",
        SetOp::ModAssign => "%=",
        SetOp::BitAndAssign => "&=",
        SetOp::BitOrAssign => "|=",
        SetOp::BitXorAssign => "^=",
        SetOp::ShiftLeftAssign => "<<=",
        SetOp::ShiftRightAssign => ">>=",
    }
}

fn dump_unary(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
        UnaryOp::Await => "await",
    }
}

fn dump_binary(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "||",
        BinaryOp::And => "&&",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Gt => ">",
        BinaryOp::LtEqual => "<=",
        BinaryOp::GtEqual => ">=",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::BitAnd => "&",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::NullCoalesce => "??",
        BinaryOp::RangeExclusive => "..",
        BinaryOp::RangeInclusive => "..=",
    }
}
