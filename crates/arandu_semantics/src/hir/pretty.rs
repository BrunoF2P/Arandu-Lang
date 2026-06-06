use super::{
    BinaryOp, HirBlock, HirCatchHandler, HirCondition, HirConst, HirDecl, HirEnum, HirExpr,
    HirExprKind, HirExtern, HirForClause, HirFunc, HirInterface, HirLambdaBody, HirMatchArmBody,
    HirParam, HirPlace, HirPlaceSuffix, HirProgram, HirSimpleStmt, HirStmt, HirStmtKind, HirStruct,
    HirTypeAlias, ReceiverKind, SetOp, SymbolTable, UnaryOp, symbol_name,
};

// ── Pretty Printer Implementation ───────────────────────────────────

pub struct HirPrettyCtx<'a> {
    pub pool: &'a crate::hir::HirPool,
    pub symbols: &'a SymbolTable,
    pub show_spans: bool,
}

fn format_hir_param(p: &HirParam, ctx: &HirPrettyCtx<'_>) -> String {
    let name = symbol_name(ctx.symbols, p.symbol);
    let ty = p.ty.display(ctx.symbols);
    if p.is_receiver {
        let prefix = match p.receiver_kind {
            Some(ReceiverKind::Shared) => "shared ",
            Some(ReceiverKind::Mut) => "mut ",
            Some(ReceiverKind::Own) => "own ",
            None => "",
        };
        format!("{prefix}{name}: {ty}")
    } else {
        format!("{name}: {ty}")
    }
}

pub fn print_program(program: &HirProgram, ctx: &HirPrettyCtx<'_>) -> String {
    let mut out = String::new();
    out.push_str("Program\n");
    if let Some(ref m) = program.module {
        out.push_str(&format!("  Module {m}\n"));
    }

    let mut first = true;
    for decl in &program.decls {
        if !first {
            out.push('\n');
        }
        first = false;
        decl.pretty_print_to(&mut out, 1, ctx);
    }
    out
}

impl HirDecl {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        match self {
            HirDecl::Const(c) => c.pretty_print_to(out, indent, ctx),
            HirDecl::TypeAlias(t) => t.pretty_print_to(out, indent, ctx),
            HirDecl::Func(f) => f.pretty_print_to(out, indent, ctx),
            HirDecl::Struct(s) => s.pretty_print_to(out, indent, ctx),
            HirDecl::Enum(e) => e.pretty_print_to(out, indent, ctx),
            HirDecl::Interface(i) => i.pretty_print_to(out, indent, ctx),
            HirDecl::Extern(ex) => ex.pretty_print_to(out, indent, ctx),
        }
    }
}

impl HirConst {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        out.push_str(&format!(
            "{}Const {}: {} =\n",
            ind,
            symbol_name(ctx.symbols, self.symbol),
            self.ty.display(ctx.symbols)
        ));
        self.value.pretty_print_to(out, indent + 1, ctx);
    }
}

impl HirTypeAlias {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        out.push_str(&format!(
            "{}TypeAlias {} = {}\n",
            ind,
            symbol_name(ctx.symbols, self.symbol),
            self.target.display(ctx.symbols)
        ));
    }
}

impl HirFunc {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        let params_str: Vec<String> = self
            .params
            .iter()
            .map(|p| format_hir_param(p, ctx))
            .collect();
        let return_ty_str = self.return_type.display(ctx.symbols);
        out.push_str(&format!(
            "{}Func {}({}) -> {}\n",
            ind,
            symbol_name(ctx.symbols, self.symbol),
            params_str.join(", "),
            return_ty_str
        ));
        if let Some(body_id) = self.body {
            ctx.pool.block(body_id).pretty_print_to(out, indent + 1, ctx);
        }
    }
}

impl HirStruct {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        out.push_str(&format!(
            "{}Struct {}\n",
            ind,
            symbol_name(ctx.symbols, self.symbol)
        ));
        let field_ind = "  ".repeat(indent + 1);
        for f in &self.fields {
            out.push_str(&format!(
                "{}{}: {}\n",
                field_ind,
                symbol_name(ctx.symbols, f.symbol),
                f.ty.display(ctx.symbols)
            ));
        }
    }
}

impl HirEnum {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        out.push_str(&format!(
            "{}Enum {}\n",
            ind,
            symbol_name(ctx.symbols, self.symbol)
        ));
        let variant_ind = "  ".repeat(indent + 1);
        for v in &self.variants {
            if let Some(ref payload) = v.payload {
                out.push_str(&format!(
                    "{}{}({})\n",
                    variant_ind,
                    symbol_name(ctx.symbols, v.symbol),
                    payload.display(ctx.symbols)
                ));
            } else {
                out.push_str(&format!(
                    "{}{}\n",
                    variant_ind,
                    symbol_name(ctx.symbols, v.symbol)
                ));
            }
        }
    }
}

impl HirInterface {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        out.push_str(&format!(
            "{}Interface {}\n",
            ind,
            symbol_name(ctx.symbols, self.symbol)
        ));
    }
}

impl HirExtern {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        out.push_str(&format!("{}Extern \"{}\"\n", ind, self.abi));
        let member_ind = "  ".repeat(indent + 1);
        for m in &self.members {
            let params_str: Vec<String> =
                m.params.iter().map(|p| format_hir_param(p, ctx)).collect();
            out.push_str(&format!(
                "{}Func {}({}) -> {}\n",
                member_ind,
                symbol_name(ctx.symbols, m.symbol),
                params_str.join(", "),
                m.return_type.display(ctx.symbols)
            ));
        }
    }
}

impl HirBlock {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        for stmt_id in &self.statements {
            ctx.pool.stmt(*stmt_id).pretty_print_to(out, indent, ctx);
        }
    }
}

impl HirStmt {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        match &self.kind {
            HirStmtKind::VarDecl { bindings, value } => {
                if bindings.len() == 1 {
                    let b = &bindings[0];
                    out.push_str(&format!(
                        "{}Var {}: {} =\n",
                        ind,
                        symbol_name(ctx.symbols, b.symbol),
                        b.ty.display(ctx.symbols)
                    ));
                } else {
                    let b_strs: Vec<String> = bindings
                        .iter()
                        .map(|b| {
                            format!(
                                "{}: {}",
                                symbol_name(ctx.symbols, b.symbol),
                                b.ty.display(ctx.symbols)
                            )
                        })
                        .collect();
                    out.push_str(&format!("{}Var ({}) =\n", ind, b_strs.join(", ")));
                }
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::Set { places, op, value } => {
                let place_strs: Vec<String> = places.iter().map(|p| p.pretty_print(ctx)).collect();
                out.push_str(&format!(
                    "{}Set ({}) {}\n",
                    ind,
                    place_strs.join(", "),
                    set_op_str(op)
                ));
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::Return { values } => {
                if values.is_empty() {
                    out.push_str(&format!("{ind}Return\n"));
                } else {
                    out.push_str(&format!("{ind}Return\n"));
                    for v in values {
                        v.pretty_print_to(out, indent + 1, ctx);
                    }
                }
            }
            HirStmtKind::Break => {
                out.push_str(&format!("{ind}Break\n"));
            }
            HirStmtKind::Continue => {
                out.push_str(&format!("{ind}Continue\n"));
            }
            HirStmtKind::Free(expr) => {
                out.push_str(&format!("{ind}Free\n"));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::Expr(expr) => {
                out.push_str(&format!("{ind}Expr\n"));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                out.push_str(&format!("{ind}If\n"));
                condition.pretty_print_to(out, indent + 1, ctx);
                let block_ind = "  ".repeat(indent + 1);
                out.push_str(&format!("{block_ind}Then\n"));
                ctx.pool.block(*then_block).pretty_print_to(out, indent + 2, ctx);
                if let Some(else_blk) = else_block {
                    out.push_str(&format!("{block_ind}Else\n"));
                    ctx.pool.block(*else_blk).pretty_print_to(out, indent + 2, ctx);
                }
            }
            HirStmtKind::While { condition, body } => {
                out.push_str(&format!("{ind}While\n"));
                condition.pretty_print_to(out, indent + 1, ctx);
                ctx.pool.block(*body).pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::For { clause, body } => {
                out.push_str(&format!("{ind}For\n"));
                clause.pretty_print_to(out, indent + 1, ctx);
                ctx.pool.block(*body).pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::Match { value, arms } => {
                out.push_str(&format!("{ind}Match\n"));
                value.pretty_print_to(out, indent + 1, ctx);
                let arm_ind = "  ".repeat(indent + 1);
                for arm in arms {
                    let guard_str = if let Some(ref g) = arm.guard {
                        format!(" if {}", g.pretty_print_inline(ctx))
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        "{}Arm({:?}{}):\n",
                        arm_ind, arm.pattern, guard_str
                    ));
                    match &arm.body {
                        HirMatchArmBody::Expr(expr) => {
                            expr.pretty_print_to(out, indent + 2, ctx);
                        }
                        HirMatchArmBody::Block(block) => {
                            ctx.pool.block(*block).pretty_print_to(out, indent + 2, ctx);
                        }
                    }
                }
            }
            HirStmtKind::Defer(block) => {
                out.push_str(&format!("{ind}Defer\n"));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::ErrDefer(block) => {
                out.push_str(&format!("{ind}ErrDefer\n"));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::Unsafe(block) => {
                out.push_str(&format!("{ind}Unsafe\n"));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
        }
    }
}

impl HirCondition {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        match self {
            HirCondition::Expr(expr) => {
                expr.pretty_print_to(out, indent, ctx);
            }
            HirCondition::Is { expr, pattern } => {
                out.push_str(&format!("{ind}Is\n"));
                expr.pretty_print_to(out, indent + 1, ctx);
                let pat_ind = "  ".repeat(indent + 1);
                out.push_str(&format!("{pat_ind}Pattern: {pattern:?}\n"));
            }
        }
    }
}

impl HirForClause {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        match self {
            HirForClause::In {
                bindings, iterable, ..
            } => {
                let b_strs: Vec<String> = bindings
                    .iter()
                    .map(|b| {
                        format!(
                            "{}: {}",
                            super::symbol_name(ctx.symbols, b.symbol),
                            b.ty.display(ctx.symbols)
                        )
                    })
                    .collect();
                out.push_str(&format!("{}In ({})\n", ind, b_strs.join(", ")));
                iterable.pretty_print_to(out, indent + 1, ctx);
            }
            HirForClause::CStyle {
                init,
                condition,
                step,
                ..
            } => {
                out.push_str(&format!("{ind}CStyle\n"));
                let sub_ind = "  ".repeat(indent + 1);
                if let Some(init_stmt) = init {
                    out.push_str(&format!("{sub_ind}Init\n"));
                    init_stmt.pretty_print_to(out, indent + 2, ctx);
                }
                if let Some(cond_expr) = condition {
                    out.push_str(&format!("{sub_ind}Condition\n"));
                    cond_expr.pretty_print_to(out, indent + 2, ctx);
                }
                if let Some(step_stmt) = step {
                    out.push_str(&format!("{sub_ind}Step\n"));
                    step_stmt.pretty_print_to(out, indent + 2, ctx);
                }
            }
        }
    }
}

impl HirSimpleStmt {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        match self {
            HirSimpleStmt::VarDecl { bindings, value } => {
                if bindings.len() == 1 {
                    let b = &bindings[0];
                    out.push_str(&format!(
                        "{}Var {}: {} =\n",
                        ind,
                        symbol_name(ctx.symbols, b.symbol),
                        b.ty.display(ctx.symbols)
                    ));
                } else {
                    let b_strs: Vec<String> = bindings
                        .iter()
                        .map(|b| {
                            format!(
                                "{}: {}",
                                symbol_name(ctx.symbols, b.symbol),
                                b.ty.display(ctx.symbols)
                            )
                        })
                        .collect();
                    out.push_str(&format!("{}Var ({}) =\n", ind, b_strs.join(", ")));
                }
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirSimpleStmt::Set { places, op, value } => {
                let place_strs: Vec<String> = places.iter().map(|p| p.pretty_print(ctx)).collect();
                out.push_str(&format!(
                    "{}Set ({}) {}\n",
                    ind,
                    place_strs.join(", "),
                    set_op_str(op)
                ));
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirSimpleStmt::Expr(expr) => {
                expr.pretty_print_to(out, indent, ctx);
            }
        }
    }
}

impl HirPlace {
    fn pretty_print(&self, ctx: &HirPrettyCtx<'_>) -> String {
        let mut out = symbol_name(ctx.symbols, self.root_symbol).to_string();
        for suffix in &self.suffixes {
            match suffix {
                HirPlaceSuffix::Field { name, .. } => {
                    out.push_str(&format!(".{name}"));
                }
                HirPlaceSuffix::Index { expr, .. } => {
                    out.push_str(&format!("[{}]", expr.pretty_print_inline(ctx)));
                }
            }
        }
        out
    }
}

impl HirExpr {
    fn pretty_print_inline(&self, ctx: &HirPrettyCtx<'_>) -> String {
        match &self.kind {
            HirExprKind::Int(v) => v.clone(),
            HirExprKind::Float(v) => v.clone(),
            HirExprKind::Bool(v) => v.to_string(),
            HirExprKind::Char(v) => format!("'{v}'"),
            HirExprKind::Str(v) => format!("\"{v}\""),
            HirExprKind::Nil => "nil".to_string(),
            HirExprKind::Path { symbol } => ctx.symbols.get(*symbol).name.clone(),
            HirExprKind::Binary { op, left, right } => {
                format!(
                    "{} {} {}",
                    left.pretty_print_inline(ctx),
                    op_str(op),
                    right.pretty_print_inline(ctx)
                )
            }
            HirExprKind::Unary { op, expr } => {
                format!("{}{}", unary_op_str(op), expr.pretty_print_inline(ctx))
            }
            HirExprKind::Index { base, index } => {
                format!(
                    "{}[{}]",
                    base.pretty_print_inline(ctx),
                    index.pretty_print_inline(ctx)
                )
            }
            HirExprKind::Field { base, field } => {
                format!("{}.{}", base.pretty_print_inline(ctx), field)
            }
            _ => "<expr>".to_string(),
        }
    }

    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        match &self.kind {
            HirExprKind::Path { symbol } => {
                let kind = ctx.symbols.get(*symbol).kind;
                let name = &ctx.symbols.get(*symbol).name;
                let prefix = if kind == crate::SymbolKind::Local || kind == crate::SymbolKind::Param
                {
                    "LocalRef"
                } else {
                    "Path"
                };
                out.push_str(&format!(
                    "{}{}({}): {}\n",
                    ind,
                    prefix,
                    name,
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::TypePath { member_symbol, .. } => {
                out.push_str(&format!(
                    "{}TypePath({}): {}\n",
                    ind,
                    symbol_name(ctx.symbols, *member_symbol),
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::Generic { callee, args } => {
                let args_strs: Vec<String> = args.iter().map(|a| a.display(ctx.symbols)).collect();
                out.push_str(&format!(
                    "{}Generic<{}>: {}\n",
                    ind,
                    args_strs.join(", "),
                    self.ty.display(ctx.symbols)
                ));
                callee.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Field { base, field } => {
                out.push_str(&format!(
                    "{}Field({}): {}\n",
                    ind,
                    field,
                    self.ty.display(ctx.symbols)
                ));
                base.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::SafeField { base, field } => {
                out.push_str(&format!(
                    "{}SafeField({}): {}\n",
                    ind,
                    field,
                    self.ty.display(ctx.symbols)
                ));
                base.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Index { base, index } => {
                out.push_str(&format!("{}Index: {}\n", ind, self.ty.display(ctx.symbols)));
                base.pretty_print_to(out, indent + 1, ctx);
                index.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::SafeIndex { base, index } => {
                out.push_str(&format!(
                    "{}SafeIndex: {}\n",
                    ind,
                    self.ty.display(ctx.symbols)
                ));
                base.pretty_print_to(out, indent + 1, ctx);
                index.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Try { expr } => {
                out.push_str(&format!("{}Try: {}\n", ind, self.ty.display(ctx.symbols)));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Call {
                callee,
                args,
                trailing_block,
            } => {
                out.push_str(&format!("{}Call: {}\n", ind, self.ty.display(ctx.symbols)));
                callee.pretty_print_to(out, indent + 1, ctx);
                for a in args {
                    a.pretty_print_to(out, indent + 1, ctx);
                }
                if let Some(block) = trailing_block {
                    let sub_ind = "  ".repeat(indent + 1);
                    out.push_str(&format!("{sub_ind}TrailingBlock\n"));
                    ctx.pool.block(*block).pretty_print_to(out, indent + 2, ctx);
                }
            }
            HirExprKind::ResultCtor { variant, value } => {
                let name = match variant {
                    crate::hir::ResultCtorVariant::Ok => "Result.Ok",
                    crate::hir::ResultCtorVariant::Err => "Result.Err",
                    crate::hir::ResultCtorVariant::Some => "Option.Some",
                };
                out.push_str(&format!(
                    "{}{}: {}\n",
                    ind,
                    name,
                    self.ty.display(ctx.symbols)
                ));
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::StructLiteral {
                struct_symbol,
                fields,
            } => {
                let name = &ctx.symbols.get(*struct_symbol).name;
                out.push_str(&format!(
                    "{}StructLiteral({}): {}\n",
                    ind,
                    name,
                    self.ty.display(ctx.symbols)
                ));
                let field_ind = "  ".repeat(indent + 1);
                for f in fields {
                    out.push_str(&format!("{}{}:\n", field_ind, f.name));
                    f.value.pretty_print_to(out, indent + 2, ctx);
                }
            }
            HirExprKind::Array { items } => {
                out.push_str(&format!("{}Array: {}\n", ind, self.ty.display(ctx.symbols)));
                for item in items {
                    item.pretty_print_to(out, indent + 1, ctx);
                }
            }
            HirExprKind::Lambda { params, body } => {
                let params_str: Vec<String> = params
                    .iter()
                    .map(|p| {
                        format!(
                            "{}: {}",
                            symbol_name(ctx.symbols, p.symbol),
                            p.ty.display(ctx.symbols)
                        )
                    })
                    .collect();
                out.push_str(&format!(
                    "{}Lambda({}): {}\n",
                    ind,
                    params_str.join(", "),
                    self.ty.display(ctx.symbols)
                ));
                match body {
                    HirLambdaBody::Expr(expr) => {
                        expr.pretty_print_to(out, indent + 1, ctx);
                    }
                    HirLambdaBody::Block(block) => {
                        ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
                    }
                }
            }
            HirExprKind::Alloc { expr } => {
                out.push_str(&format!("{}Alloc: {}\n", ind, self.ty.display(ctx.symbols)));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::AsyncBlock { block } => {
                out.push_str(&format!(
                    "{}AsyncBlock: {}\n",
                    ind,
                    self.ty.display(ctx.symbols)
                ));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::UnsafeBlock { block } => {
                out.push_str(&format!(
                    "{}UnsafeBlock: {}\n",
                    ind,
                    self.ty.display(ctx.symbols)
                ));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                out.push_str(&format!("{}If: {}\n", ind, self.ty.display(ctx.symbols)));
                condition.pretty_print_to(out, indent + 1, ctx);
                let sub_ind = "  ".repeat(indent + 1);
                out.push_str(&format!("{sub_ind}Then\n"));
                ctx.pool.block(*then_block).pretty_print_to(out, indent + 2, ctx);
                out.push_str(&format!("{sub_ind}Else\n"));
                ctx.pool.block(*else_block).pretty_print_to(out, indent + 2, ctx);
            }
            HirExprKind::Match { value, arms } => {
                out.push_str(&format!("{}Match: {}\n", ind, self.ty.display(ctx.symbols)));
                value.pretty_print_to(out, indent + 1, ctx);
                let arm_ind = "  ".repeat(indent + 1);
                for arm in arms {
                    let guard_str = if let Some(ref g) = arm.guard {
                        format!(" if {}", g.pretty_print_inline(ctx))
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        "{}Arm({:?}{}):\n",
                        arm_ind, arm.pattern, guard_str
                    ));
                    match &arm.body {
                        HirMatchArmBody::Expr(expr) => {
                            expr.pretty_print_to(out, indent + 2, ctx);
                        }
                        HirMatchArmBody::Block(block) => {
                            ctx.pool.block(*block).pretty_print_to(out, indent + 2, ctx);
                        }
                    }
                }
            }
            HirExprKind::Catch { expr, handler } => {
                out.push_str(&format!("{}Catch: {}\n", ind, self.ty.display(ctx.symbols)));
                expr.pretty_print_to(out, indent + 1, ctx);
                let sub_ind = "  ".repeat(indent + 1);
                match handler {
                    HirCatchHandler::Expr(h_expr) => {
                        out.push_str(&format!("{sub_ind}Handler\n"));
                        h_expr.pretty_print_to(out, indent + 2, ctx);
                    }
                    HirCatchHandler::Block {
                        error_name, block, ..
                    } => {
                        let err_str = error_name.as_deref().unwrap_or("error");
                        out.push_str(&format!("{sub_ind}Handler({err_str})\n"));
                        ctx.pool.block(*block).pretty_print_to(out, indent + 2, ctx);
                    }
                }
            }
            HirExprKind::NullCoalesce { left, right } => {
                out.push_str(&format!(
                    "{}NullCoalesce: {}\n",
                    ind,
                    self.ty.display(ctx.symbols)
                ));
                left.pretty_print_to(out, indent + 1, ctx);
                right.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Cast { expr, target_ty } => {
                out.push_str(&format!(
                    "{}Cast({}): {}\n",
                    ind,
                    target_ty.display(ctx.symbols),
                    self.ty.display(ctx.symbols)
                ));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Unary { op, expr } => {
                out.push_str(&format!(
                    "{}Unary({}): {}\n",
                    ind,
                    unary_op_str(op),
                    self.ty.display(ctx.symbols)
                ));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Binary { op, left, right } => {
                out.push_str(&format!(
                    "{}Binary({}): {}\n",
                    ind,
                    op_str(op),
                    self.ty.display(ctx.symbols)
                ));
                left.pretty_print_to(out, indent + 1, ctx);
                right.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Int(v) => {
                out.push_str(&format!(
                    "{}Int({}): {}\n",
                    ind,
                    v,
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::Float(v) => {
                out.push_str(&format!(
                    "{}Float({}): {}\n",
                    ind,
                    v,
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::Bool(v) => {
                out.push_str(&format!(
                    "{}Bool({}): {}\n",
                    ind,
                    v,
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::Char(v) => {
                out.push_str(&format!(
                    "{}Char({}): {}\n",
                    ind,
                    v,
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::Str(v) => {
                out.push_str(&format!(
                    "{}Str({}): {}\n",
                    ind,
                    v,
                    self.ty.display(ctx.symbols)
                ));
            }
            HirExprKind::Nil => {
                out.push_str(&format!("{}Nil: {}\n", ind, self.ty.display(ctx.symbols)));
            }
        }
    }
}

fn set_op_str(op: &SetOp) -> &str {
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

fn op_str(op: &BinaryOp) -> &str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::LtEqual => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::GtEqual => ">=",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::BitXor => "^",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::NullCoalesce => "??",
        BinaryOp::RangeExclusive => "..",
        BinaryOp::RangeInclusive => "..=",
    }
}

fn unary_op_str(op: &UnaryOp) -> &str {
    match op {
        UnaryOp::Not => "not ",
        UnaryOp::Neg => "-",
        UnaryOp::BitNot => "~",
        UnaryOp::Await => "await ",
    }
}
