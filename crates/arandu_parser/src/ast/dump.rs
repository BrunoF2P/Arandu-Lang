use arandu_lexer::Span;

use super::*;

pub fn dump_program(program: &Program) -> String {
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
        out.push(format!("  {}", dump_import(import)));
    }
    for decl in &program.decls {
        dump_top_level_decl(decl, &mut out);
    }
    out.join("\n")
}

fn dump_span(span: Span) -> String {
    format!(
        "@{}:{}-{}:{}",
        span.start_line, span.start_col, span.end_line, span.end_col
    )
}

fn dump_top_level_decl(decl: &TopLevelDecl, out: &mut Vec<String>) {
    match decl {
        TopLevelDecl::Const(decl) => dump_const(decl, out),
        TopLevelDecl::TypeAlias(decl) => dump_type_alias(decl, out),
        TopLevelDecl::Func(func) => dump_func(func, out),
        TopLevelDecl::Struct(decl) => dump_struct(decl, out),
        TopLevelDecl::Enum(decl) => dump_enum(decl, out),
        TopLevelDecl::Interface(decl) => dump_interface(decl, out),
        TopLevelDecl::Extern(decl) => dump_extern(decl, out),
        TopLevelDecl::Error(span) => out.push(format!("  DeclError {}", dump_span(*span))),
    }
}

fn dump_const(decl: &ConstDecl, out: &mut Vec<String>) {
    dump_attrs(&decl.attrs, out, 2);
    let ty = decl
        .ty
        .as_ref()
        .map(|ty| format!(" {}", dump_type(ty)))
        .unwrap_or_default();
    out.push(format!(
        "  Const {} {}{}{ty} = {}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_expr(&decl.value)
    ));
}

fn dump_type_alias(decl: &TypeAliasDecl, out: &mut Vec<String>) {
    dump_attrs(&decl.attrs, out, 2);
    out.push(format!(
        "  Type {} {}{}{} = {}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_type(&decl.ty)
    ));
}

fn dump_func(func: &FuncDecl, out: &mut Vec<String>) {
    dump_attrs(&func.attrs, out, 2);
    let params = func
        .params
        .iter()
        .map(dump_param)
        .collect::<Vec<_>>()
        .join(", ");
    let result = func
        .result
        .as_ref()
        .map(dump_result_type)
        .unwrap_or_else(|| "void".to_string());
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
    dump_block_body(&func.body, out, 4);
}

fn dump_struct(decl: &StructDecl, out: &mut Vec<String>) {
    dump_attrs(&decl.attrs, out, 2);
    out.push(format!(
        "  Struct {} {}{}{}{}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_where_clause(&decl.where_clause)
    ));
    for field in &decl.fields {
        dump_attrs(&field.attrs, out, 4);
        out.push(format!(
            "    Field {} {}{} {}",
            dump_span(field.span),
            dump_visibility(field.visibility),
            field.name,
            dump_type(&field.ty)
        ));
    }
}

fn dump_enum(decl: &EnumDecl, out: &mut Vec<String>) {
    dump_attrs(&decl.attrs, out, 2);
    out.push(format!(
        "  Enum {} {}{}{}{}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_where_clause(&decl.where_clause)
    ));
    for variant in &decl.variants {
        dump_attrs(&variant.attrs, out, 4);
        let payload = match &variant.payload {
            None => String::new(),
            Some(EnumPayload::Tuple { types, .. }) => {
                let types = types.iter().map(dump_type).collect::<Vec<_>>().join(", ");
                format!("({types})")
            }
            Some(EnumPayload::Struct { fields, .. }) => {
                let fields = fields
                    .iter()
                    .map(|field| format!("{} {}", field.name, dump_type(&field.ty)))
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

fn dump_interface(decl: &InterfaceDecl, out: &mut Vec<String>) {
    dump_attrs(&decl.attrs, out, 2);
    out.push(format!(
        "  Interface {} {}{}{}{}",
        dump_span(decl.span),
        dump_visibility(decl.visibility),
        decl.name,
        dump_generic_params(&decl.generic_params),
        dump_where_clause(&decl.where_clause)
    ));
    for member in &decl.members {
        dump_signature(member, out, 4);
    }
}

fn dump_extern(decl: &ExternDecl, out: &mut Vec<String>) {
    dump_attrs(&decl.attrs, out, 2);
    out.push(format!(
        "  Extern {} \"{}\"",
        dump_span(decl.span),
        decl.abi
    ));
    for member in &decl.members {
        dump_signature(member, out, 4);
    }
}

fn dump_signature(signature: &FuncSignature, out: &mut Vec<String>, indent: usize) {
    dump_attrs(&signature.attrs, out, indent);
    let pad = " ".repeat(indent);
    let params = signature
        .params
        .iter()
        .map(dump_param)
        .collect::<Vec<_>>()
        .join(", ");
    let result = signature
        .result
        .as_ref()
        .map(dump_result_type)
        .unwrap_or_else(|| "void".to_string());
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

fn dump_attrs(attrs: &[Attribute], out: &mut Vec<String>, indent: usize) {
    let pad = " ".repeat(indent);
    for attr in attrs {
        if attr.args.is_empty() {
            out.push(format!("{pad}Attr {} {}", dump_span(attr.span), attr.name));
        } else {
            let args = attr
                .args
                .iter()
                .map(dump_expr)
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

fn dump_block_body(block: &Block, out: &mut Vec<String>, indent: usize) {
    for stmt in &block.statements {
        dump_stmt(stmt, out, indent);
    }
}

fn dump_stmt(stmt: &Stmt, out: &mut Vec<String>, indent: usize) {
    let pad = " ".repeat(indent);
    match stmt {
        Stmt::VarDecl {
            span,
            bindings,
            value,
        } => {
            let bindings = bindings
                .iter()
                .map(dump_binding)
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "{pad}Var {} {bindings} = {}",
                dump_span(*span),
                dump_expr(value)
            ));
        }
        Stmt::Set {
            span,
            places,
            op,
            value,
        } => {
            let places = places.iter().map(dump_place).collect::<Vec<_>>().join(", ");
            out.push(format!(
                "{pad}Set {} {places} {} {}",
                dump_span(*span),
                dump_set_op(op),
                dump_expr(value)
            ));
        }
        Stmt::Return { span, values } => {
            let values = values.iter().map(dump_expr).collect::<Vec<_>>().join(", ");
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
                dump_expr(expr)
            ));
        }
        Stmt::Expr { span, expr } => {
            out.push(format!(
                "{pad}Expr {} {}",
                dump_span(*span),
                dump_expr(expr)
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
                dump_condition(condition)
            ));
            dump_block_body(then_block, out, indent + 2);
            if let Some(else_block) = else_block {
                out.push(format!("{pad}Else {}", dump_span(else_block.span)));
                dump_block_body(else_block, out, indent + 2);
            }
        }
        Stmt::For { span, clause, body } => {
            out.push(format!(
                "{pad}For {}{}",
                dump_span(*span),
                dump_for_clause(clause)
            ));
            dump_block_body(body, out, indent + 2);
        }
        Stmt::While {
            span,
            condition,
            body,
        } => {
            out.push(format!(
                "{pad}While {} {}",
                dump_span(*span),
                dump_condition(condition)
            ));
            dump_block_body(body, out, indent + 2);
        }
        Stmt::Match { span, expr } => {
            out.push(format!(
                "{pad}MatchStmt {} {}",
                dump_span(*span),
                dump_expr(expr)
            ));
        }
        Stmt::Defer { span, body } => dump_defer_body("Defer", *span, body, out, indent),
        Stmt::ErrDefer { span, body } => dump_defer_body("ErrDefer", *span, body, out, indent),
        Stmt::Unsafe { span, block } => {
            out.push(format!("{pad}Unsafe {}", dump_span(*span)));
            dump_block_body(block, out, indent + 2);
        }
        Stmt::Error(span) => out.push(format!("{pad}StmtError {}", dump_span(*span))),
    }
}

fn dump_condition(condition: &Condition) -> String {
    match condition {
        Condition::Expr { span, expr } => {
            format!("Condition {} {}", dump_span(*span), dump_expr(expr))
        }
        Condition::Is {
            span,
            expr,
            pattern,
        } => {
            format!(
                "Is {} ({}, {})",
                dump_span(*span),
                dump_expr(expr),
                dump_pattern(pattern)
            )
        }
    }
}

fn dump_for_clause(clause: &ForClause) -> String {
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
                dump_expr(iterable)
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
                .map(|stmt| dump_simple_stmt(stmt))
                .unwrap_or_else(|| "none".to_string()),
            condition
                .as_ref()
                .map(|expr| dump_expr(expr))
                .unwrap_or_else(|| "none".to_string()),
            step.as_ref()
                .map(|stmt| dump_simple_stmt(stmt))
                .unwrap_or_else(|| "none".to_string())
        ),
    }
}

fn dump_simple_stmt(stmt: &SimpleStmt) -> String {
    match stmt {
        SimpleStmt::VarDecl {
            span,
            bindings,
            value,
        } => {
            let bindings = bindings
                .iter()
                .map(dump_binding)
                .collect::<Vec<_>>()
                .join(", ");
            format!("Var {} {bindings} = {}", dump_span(*span), dump_expr(value))
        }
        SimpleStmt::Set {
            span,
            places,
            op,
            value,
        } => {
            let places = places.iter().map(dump_place).collect::<Vec<_>>().join(", ");
            format!(
                "Set {} {places} {} {}",
                dump_span(*span),
                dump_set_op(op),
                dump_expr(value)
            )
        }
        SimpleStmt::Expr { span, expr } => format!("Expr {} {}", dump_span(*span), dump_expr(expr)),
    }
}

fn dump_defer_body(
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
                dump_expr(expr)
            ));
        }
        DeferBody::Block { block, .. } => {
            out.push(format!(
                "{pad}{label} {} Block {}",
                dump_span(span),
                dump_span(block.span)
            ));
            dump_block_body(block, out, indent + 2);
        }
    }
}

fn dump_binding(binding: &BindingItem) -> String {
    let mut out = format!("{} ", dump_span(binding.span));
    if binding.mutable {
        out.push_str("mut ");
    }
    out.push_str(&binding.name);
    if let Some(ty) = &binding.ty {
        out.push(' ');
        out.push_str(&dump_type(ty));
    }
    out
}

fn dump_place(place: &Place) -> String {
    let mut out = format!("{} {}", dump_span(place.span), place.root);
    for suffix in &place.suffixes {
        match suffix {
            PlaceSuffix::Field { name, .. } => {
                out.push('.');
                out.push_str(name);
            }
            PlaceSuffix::Index { expr, .. } => {
                out.push('[');
                out.push_str(&dump_expr(expr));
                out.push(']');
            }
        }
    }
    out
}

fn dump_import(import: &ImportDecl) -> String {
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

fn dump_param(param: &Param) -> String {
    let mut out = format!("{} ", dump_span(param.span));
    if let Some(ownership) = param.ownership {
        out.push_str(match ownership {
            Ownership::Own => "own ",
            Ownership::Mut => "mut ",
        });
    }
    out.push_str(&param.name);
    out.push(' ');
    out.push_str(&dump_type(&param.ty));
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

fn dump_result_type(result: &ResultType) -> String {
    match result {
        ResultType::Single { ty, .. } => dump_type(ty),
        ResultType::Multi { types, .. } => {
            let inner = types.iter().map(dump_type).collect::<Vec<_>>().join(", ");
            format!("({inner})")
        }
    }
}

fn dump_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Primitive { span, name } => format!("Type {} {name}", dump_span(*span)),
        TypeExpr::Named { span, name, args } => {
            let mut out = format!("Type {} {}", dump_span(*span), dump_type_name(name));
            if !args.is_empty() {
                let args = args.iter().map(dump_type).collect::<Vec<_>>().join(", ");
                out.push_str(&format!("<{args}>"));
            }
            out
        }
        TypeExpr::Nullable { span, inner } => {
            format!("Nullable {} {}", dump_span(*span), dump_type(inner))
        }
        TypeExpr::Pointer { span, inner } => {
            format!("Ptr {} [{}]", dump_span(*span), dump_type(inner))
        }
        TypeExpr::Slice { span, inner } => {
            format!("Slice {} {}", dump_span(*span), dump_type(inner))
        }
        TypeExpr::Array { span, size, elem } => {
            format!("ArrayType {} [{size}]{}", dump_span(*span), dump_type(elem))
        }
        TypeExpr::Func {
            span,
            params,
            result,
        } => {
            let params = params.iter().map(dump_type).collect::<Vec<_>>().join(", ");
            match result {
                Some(result) => {
                    format!(
                        "FuncType {} ({params}) {}",
                        dump_span(*span),
                        dump_result_type(result)
                    )
                }
                None => format!("FuncType {} ({params})", dump_span(*span)),
            }
        }
        TypeExpr::Group { span, inner } => {
            format!("GroupType {} ({})", dump_span(*span), dump_type(inner))
        }
    }
}

fn dump_type_name(name: &TypeName) -> String {
    format!("{} {}", dump_span(name.span), name.path.join("."))
}

fn dump_expr(expr: &Expr) -> String {
    match expr {
        Expr::Path { span, path } => format!("Path {}({})", dump_span(*span), path.join(".")),
        Expr::TypePath {
            span,
            type_name,
            member,
        } => {
            format!(
                "TypePath {}({}.{})",
                dump_span(*span),
                dump_type_name(type_name),
                member
            )
        }
        Expr::Generic { span, callee, args } => {
            let args = args.iter().map(dump_type).collect::<Vec<_>>().join(", ");
            format!(
                "Generic {}({}, <{args}>)",
                dump_span(*span),
                dump_expr(callee)
            )
        }
        Expr::Field { span, base, field } => {
            format!("Field {}({}, {field})", dump_span(*span), dump_expr(base))
        }
        Expr::SafeField { span, base, field } => {
            format!(
                "SafeField {}({}, {field})",
                dump_span(*span),
                dump_expr(base)
            )
        }
        Expr::Index { span, base, index } => {
            format!(
                "Index {}({}, {})",
                dump_span(*span),
                dump_expr(base),
                dump_expr(index)
            )
        }
        Expr::SafeIndex { span, base, index } => {
            format!(
                "SafeIndex {}({}, {})",
                dump_span(*span),
                dump_expr(base),
                dump_expr(index)
            )
        }
        Expr::Try { span, expr } => format!("Try {}({})", dump_span(*span), dump_expr(expr)),
        Expr::Call {
            span,
            callee,
            args,
            trailing_block,
        } => dump_call(*span, callee, args, trailing_block),
        Expr::StructLiteral { span, ty, fields } => {
            let fields = fields
                .iter()
                .map(|field| {
                    format!(
                        "{} {}: {}",
                        dump_span(field.span),
                        field.name,
                        dump_expr(&field.value)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "StructLiteral {}({}, [{fields}])",
                dump_span(*span),
                dump_type(ty)
            )
        }
        Expr::Array { span, items } => {
            let items = items.iter().map(dump_expr).collect::<Vec<_>>().join(", ");
            format!("Array {}([{items}])", dump_span(*span))
        }
        Expr::Lambda { span, params, body } => {
            let params = params
                .iter()
                .map(|param| match &param.ty {
                    Some(ty) => {
                        format!("{} {} {}", dump_span(param.span), param.name, dump_type(ty))
                    }
                    None => format!("{} {}", dump_span(param.span), param.name),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "Lambda {}([{params}], {})",
                dump_span(*span),
                dump_lambda_body(body)
            )
        }
        Expr::Alloc { span, expr } => format!("Alloc {}({})", dump_span(*span), dump_expr(expr)),
        Expr::AsyncBlock { span, block } => dump_inline_block("AsyncBlock", *span, block),
        Expr::UnsafeBlock { span, block } => dump_inline_block("UnsafeBlock", *span, block),
        Expr::If {
            span,
            condition,
            then_block,
            else_block,
        } => format!(
            "IfExpr {}({}, {}, {})",
            dump_span(*span),
            dump_condition(condition),
            dump_block_inline(then_block),
            dump_block_inline(else_block)
        ),
        Expr::Match { span, value, arms } => {
            let arms = arms
                .iter()
                .map(dump_match_arm)
                .collect::<Vec<_>>()
                .join(", ");
            format!("Match {}({}, [{arms}])", dump_span(*span), dump_expr(value))
        }
        Expr::Catch {
            span,
            expr,
            handler,
        } => format!(
            "Catch {}({}, {})",
            dump_span(*span),
            dump_expr(expr),
            dump_catch_handler(handler)
        ),
        Expr::NullCoalesce { span, left, right } => {
            format!(
                "NullCoalesce {}({}, {})",
                dump_span(*span),
                dump_expr(left),
                dump_expr(right)
            )
        }
        Expr::Cast { span, expr, ty } => {
            format!(
                "Cast {}({}, {})",
                dump_span(*span),
                dump_expr(expr),
                dump_type(ty)
            )
        }
        Expr::Group { span, expr } => format!("Group {}({})", dump_span(*span), dump_expr(expr)),
        Expr::Unary { span, op, expr } => {
            format!(
                "Unary {}({}, {})",
                dump_span(*span),
                dump_unary(*op),
                dump_expr(expr)
            )
        }
        Expr::Binary {
            span,
            op,
            left,
            right,
        } => {
            format!(
                "Binary {}({}, {}, {})",
                dump_span(*span),
                dump_binary(*op),
                dump_expr(left),
                dump_expr(right)
            )
        }
        Expr::Int { span, value } => format!("Int {}({value})", dump_span(*span)),
        Expr::Float { span, value } => format!("Float {}({value})", dump_span(*span)),
        Expr::Bool { span, value } => format!("Bool {}({value})", dump_span(*span)),
        Expr::Char { span, value } => format!("Char {}('{value}')", dump_span(*span)),
        Expr::InterpolatedString { span, parts } => dump_interpolated_string(*span, parts),
        Expr::Nil { span } => format!("Nil {}", dump_span(*span)),
        Expr::Error(span) => format!("ExprError {}", dump_span(*span)),
    }
}

fn dump_call(span: Span, callee: &Expr, args: &[Expr], trailing_block: &Option<Block>) -> String {
    let args = args.iter().map(dump_expr).collect::<Vec<_>>().join(", ");
    match trailing_block {
        Some(block) => format!(
            "Call {}({}, [{args}], {})",
            dump_span(span),
            dump_expr(callee),
            dump_block_inline(block)
        ),
        None => format!("Call {}({}, [{args}])", dump_span(span), dump_expr(callee)),
    }
}

fn dump_lambda_body(body: &LambdaBody) -> String {
    match body {
        LambdaBody::Expr { expr, .. } => format!("Expr({})", dump_expr(expr)),
        LambdaBody::Block { block, .. } => dump_block_inline(block),
    }
}

fn dump_catch_handler(handler: &CatchHandler) -> String {
    match handler {
        CatchHandler::Expr { expr, .. } => format!("Expr({})", dump_expr(expr)),
        CatchHandler::Block { error, block, .. } => {
            format!("Handler({error}, {})", dump_block_inline(block))
        }
    }
}

fn dump_inline_block(label: &str, span: Span, block: &Block) -> String {
    format!("{label} {} {}", dump_span(span), dump_block_inline(block))
}

fn dump_block_inline(block: &Block) -> String {
    let stmts = block
        .statements
        .iter()
        .map(dump_stmt_inline)
        .collect::<Vec<_>>()
        .join("; ");
    format!("Block {}[{stmts}]", dump_span(block.span))
}

fn dump_stmt_inline(stmt: &Stmt) -> String {
    let mut out = Vec::new();
    dump_stmt(stmt, &mut out, 0);
    out.join(" ")
}

fn dump_match_arm(arm: &MatchArm) -> String {
    format!(
        "Arm {} {}{} => {}",
        dump_span(arm.span),
        dump_pattern(&arm.pattern),
        arm.guard
            .as_ref()
            .map(|guard| format!(" if {}", dump_expr(guard)))
            .unwrap_or_default(),
        dump_match_arm_body(&arm.body)
    )
}

fn dump_match_arm_body(body: &MatchArmBody) -> String {
    match body {
        MatchArmBody::Expr { expr, .. } => dump_expr(expr),
        MatchArmBody::Block { block, .. } => dump_block_inline(block),
    }
}

fn dump_pattern(pattern: &Pattern) -> String {
    match pattern {
        Pattern::Wildcard { span } => format!("Wildcard {}", dump_span(*span)),
        Pattern::Bind { span, name } => format!("Bind {}({name})", dump_span(*span)),
        Pattern::Literal { span, expr } => {
            format!("Literal {}({})", dump_span(*span), dump_expr(expr))
        }
        Pattern::Enum {
            span,
            type_name,
            variant,
            payload,
        } => {
            let payload = payload
                .iter()
                .map(dump_pattern)
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "EnumPattern {}({}.{}, [{payload}])",
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
            let payload = payload
                .iter()
                .map(dump_pattern)
                .collect::<Vec<_>>()
                .join(", ");
            format!("TypePattern {}({name}, [{payload}])", dump_span(*span))
        }
        Pattern::Struct {
            span,
            type_name,
            fields,
        } => {
            let fields = fields
                .iter()
                .map(|field| match &field.pattern {
                    Some(pattern) => {
                        format!(
                            "{} {}: {}",
                            dump_span(field.span),
                            field.name,
                            dump_pattern(pattern)
                        )
                    }
                    None => format!("{} {}", dump_span(field.span), field.name),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "StructPattern {}({}, [{fields}])",
                dump_span(*span),
                dump_type_name(type_name)
            )
        }
        Pattern::Tuple { span, items } => {
            let items = items
                .iter()
                .map(dump_pattern)
                .collect::<Vec<_>>()
                .join(", ");
            format!("TuplePattern {}([{items}])", dump_span(*span))
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
                dump_expr(start),
                dump_expr(end)
            )
        }
    }
}

fn dump_interpolated_string(span: Span, parts: &[StringPart]) -> String {
    if let [StringPart::Text { text, .. }] = parts {
        return format!("String {}(\"{text}\")", dump_span(span));
    }

    let parts = parts
        .iter()
        .map(|part| match part {
            StringPart::Text { span, text } => format!("Text {}(\"{text}\")", dump_span(*span)),
            StringPart::Expr { span, expr } => {
                format!("Expr {}({})", dump_span(*span), dump_expr(expr))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("InterpolatedString {}([{parts}])", dump_span(span))
}

fn dump_generic_params(params: &[GenericParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let params = params
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
    format!("<{params}>")
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
