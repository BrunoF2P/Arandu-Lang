use super::{
    BinaryOp, HirBlock, HirCatchHandler, HirCondition, HirConst, HirDecl, HirEnum, HirExpr,
    HirExprKind, HirExtern, HirFieldPattern, HirForClause, HirFunc, HirInterface, HirLambdaBody,
    HirMatchArmBody, HirParam, HirPattern, HirPlace, HirPlaceSuffix, HirProgram, HirSimpleStmt,
    HirStmt, HirStmtKind, HirStruct, HirTypeAlias, ReceiverKind, SetOp, SymbolTable, UnaryOp,
    symbol_name,
};

// ── Pretty Printer Implementation ───────────────────────────────────

impl super::HirExprId {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        ctx.pool.expr(*self).pretty_print_to(out, indent, ctx);
    }
    fn pretty_print_inline(&self, ctx: &HirPrettyCtx<'_>) -> String {
        ctx.pool.expr(*self).pretty_print_inline(ctx)
    }
}

impl super::HirBlockId {
    #[allow(dead_code)]
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        ctx.pool.block(*self).pretty_print_to(out, indent, ctx);
    }
}

impl super::HirDeclId {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        ctx.pool.decl(*self).pretty_print_to(out, indent, ctx);
    }
}

impl super::HirPatternId {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        ctx.pool.pattern(*self).pretty_print_to(out, indent, ctx);
    }
}

impl super::HirFieldPatternId {
    #[allow(dead_code)]
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        ctx.pool
            .field_pattern(*self)
            .pretty_print_to(out, indent, ctx);
    }
}

fn format_pattern_ref(pat: &HirPattern, ctx: &HirPrettyCtx<'_>) -> String {
    match pat {
        HirPattern::Wildcard { span } => {
            format!("Wildcard {{ span: {:?} }}", span)
        }
        HirPattern::Bind { span, name, symbol } => {
            format!(
                "Bind {{ span: {:?}, name: {:?}, symbol: {:?} }}",
                span, name, symbol
            )
        }
        HirPattern::Literal { span, expr } => {
            format!("Literal {{ span: {:?}, expr: {:?} }}", span, expr)
        }
        HirPattern::Enum {
            span,
            type_symbol,
            variant,
            variant_symbol,
            payload,
        } => {
            let mut payload_strs = Vec::new();
            for &pid in ctx.pool.pattern_list(*payload) {
                payload_strs.push(format_pattern_ref(ctx.pool.pattern(pid), ctx));
            }
            format!(
                "Enum {{ span: {:?}, type_symbol: {:?}, variant: {:?}, variant_symbol: {:?}, payload: [{}] }}",
                span,
                type_symbol,
                variant,
                variant_symbol,
                payload_strs.join(", ")
            )
        }
        HirPattern::TypeTuple {
            span,
            name,
            payload,
        } => {
            let mut payload_strs = Vec::new();
            for &pid in ctx.pool.pattern_list(*payload) {
                payload_strs.push(format_pattern_ref(ctx.pool.pattern(pid), ctx));
            }
            format!(
                "TypeTuple {{ span: {:?}, name: {:?}, payload: [{}] }}",
                span,
                name,
                payload_strs.join(", ")
            )
        }
        HirPattern::Struct {
            span,
            struct_symbol,
            fields,
        } => {
            let mut field_strs = Vec::new();
            for &fid in ctx.pool.field_pattern_list(*fields) {
                let f = ctx.pool.field_pattern(fid);
                let pat_str = f.pattern.map_or("None".to_string(), |pid| {
                    format!("Some({})", format_pattern_ref(ctx.pool.pattern(pid), ctx))
                });
                field_strs.push(format!(
                    "HirFieldPattern {{ span: {:?}, name: {:?}, pattern: {} }}",
                    f.span, f.name, pat_str
                ));
            }
            format!(
                "Struct {{ span: {:?}, struct_symbol: {:?}, fields: [{}] }}",
                span,
                struct_symbol,
                field_strs.join(", ")
            )
        }
        HirPattern::Tuple { span, items } => {
            let mut item_strs = Vec::new();
            for &pid in ctx.pool.pattern_list(*items) {
                item_strs.push(format_pattern_ref(ctx.pool.pattern(pid), ctx));
            }
            format!(
                "Tuple {{ span: {:?}, items: [{}] }}",
                span,
                item_strs.join(", ")
            )
        }
        HirPattern::Range {
            span,
            start,
            inclusive,
            end,
        } => {
            format!(
                "Range {{ span: {:?}, start: {:?}, inclusive: {:?}, end: {:?} }}",
                span, start, inclusive, end
            )
        }
    }
}

impl HirPattern {
    fn pretty_print_to(&self, out: &mut String, _indent: usize, ctx: &HirPrettyCtx<'_>) {
        out.push_str(&format_pattern_ref(self, ctx));
    }
}

impl HirFieldPattern {
    #[allow(dead_code)]
    fn pretty_print_to(&self, out: &mut String, _indent: usize, ctx: &HirPrettyCtx<'_>) {
        let pat_str = self.pattern.map_or("None".to_string(), |pid| {
            format!("Some({})", format_pattern_ref(ctx.pool.pattern(pid), ctx))
        });
        out.push_str(&format!(
            "HirFieldPattern {{ span: {:?}, name: {:?}, pattern: {} }}",
            self.span, self.name, pat_str
        ));
    }
}

fn display_type(ty: &crate::types::ArType, ctx: &HirPrettyCtx<'_>) -> String {
    ctx.display_ty(ty)
}

pub struct HirPrettyCtx<'a> {
    pub pool: &'a crate::hir::HirPool,
    pub symbols: &'a SymbolTable,
    pub show_spans: bool,
    pub type_interner: Option<&'a crate::types::TypeInterner>,
}

impl HirPrettyCtx<'_> {
    /// Display an `ArType` using the interner if available, or a placeholder.
    fn display_ty(&self, ty: &crate::types::ArType) -> String {
        static EMPTY: std::sync::LazyLock<crate::types::TypeInterner> =
            std::sync::LazyLock::new(crate::types::TypeInterner::new);
        let interner = self.type_interner.unwrap_or(&EMPTY);
        ty.display(self.symbols, interner)
    }
}

fn format_hir_param(p: &HirParam, ctx: &HirPrettyCtx<'_>) -> String {
    let name = symbol_name(ctx.symbols, p.symbol);
    let ty = ctx.display_ty(&p.ty);
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
            ctx.display_ty(&self.ty)
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
            ctx.display_ty(&self.target)
        ));
    }
}

impl HirFunc {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        let params_str: Vec<String> = ctx
            .pool
            .params_list(self.params)
            .iter()
            .map(|p| format_hir_param(p, ctx))
            .collect();
        let return_ty_str = ctx.display_ty(&self.return_type);
        out.push_str(&format!(
            "{}Func {}({}) -> {}\n",
            ind,
            symbol_name(ctx.symbols, self.symbol),
            params_str.join(", "),
            return_ty_str
        ));
        if let Some(body_id) = self.body {
            ctx.pool
                .block(body_id)
                .pretty_print_to(out, indent + 1, ctx);
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
        for f in ctx.pool.struct_fields_list(self.fields) {
            out.push_str(&format!(
                "{}{}: {}\n",
                field_ind,
                symbol_name(ctx.symbols, f.symbol),
                ctx.display_ty(&f.ty)
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
        for v in ctx.pool.enum_variants_list(self.variants) {
            if let Some(ref payload) = v.payload {
                out.push_str(&format!(
                    "{}{}({})\n",
                    variant_ind,
                    symbol_name(ctx.symbols, v.symbol),
                    ctx.display_ty(payload)
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
        for m in ctx.pool.func_signatures_list(self.members) {
            let params_str: Vec<String> = ctx
                .pool
                .params_list(m.params)
                .iter()
                .map(|p| format_hir_param(p, ctx))
                .collect();
            out.push_str(&format!(
                "{}Func {}({}) -> {}\n",
                member_ind,
                symbol_name(ctx.symbols, m.symbol),
                params_str.join(", "),
                ctx.display_ty(&m.return_type)
            ));
        }
    }
}

impl HirBlock {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        for &stmt_id in ctx.pool.stmt_list(self.statements) {
            ctx.pool.stmt(stmt_id).pretty_print_to(out, indent, ctx);
        }
    }
}

impl HirStmt {
    fn pretty_print_to(&self, out: &mut String, indent: usize, ctx: &HirPrettyCtx<'_>) {
        let ind = "  ".repeat(indent);
        match &self.kind {
            HirStmtKind::VarDecl { bindings, value } => {
                let bindings_slice = ctx.pool.bindings_list(*bindings);
                if bindings_slice.len() == 1 {
                    let b = &bindings_slice[0];
                    out.push_str(&format!(
                        "{}Var {}: {} =\n",
                        ind,
                        symbol_name(ctx.symbols, b.symbol),
                        display_type(&b.ty, ctx)
                    ));
                } else {
                    let b_strs: Vec<String> = bindings_slice
                        .iter()
                        .map(|b| {
                            format!(
                                "{}: {}",
                                symbol_name(ctx.symbols, b.symbol),
                                display_type(&b.ty, ctx)
                            )
                        })
                        .collect();
                    out.push_str(&format!("{}Var ({}) =\n", ind, b_strs.join(", ")));
                }
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirStmtKind::Set { places, op, value } => {
                let place_strs: Vec<String> = ctx
                    .pool
                    .places_list(*places)
                    .iter()
                    .map(|p| p.pretty_print(ctx))
                    .collect();
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
                    for &v in ctx.pool.expr_list(*values) {
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
                ctx.pool
                    .block(*then_block)
                    .pretty_print_to(out, indent + 2, ctx);
                if let Some(else_blk) = else_block {
                    out.push_str(&format!("{block_ind}Else\n"));
                    ctx.pool
                        .block(*else_blk)
                        .pretty_print_to(out, indent + 2, ctx);
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
                for arm in ctx.pool.match_arms_list(*arms) {
                    let guard_str = if let Some(ref g) = arm.guard {
                        format!(" if {}", g.pretty_print_inline(ctx))
                    } else {
                        String::new()
                    };
                    let mut pat_str = String::new();
                    arm.pattern.pretty_print_to(&mut pat_str, 0, ctx);
                    out.push_str(&format!("{}Arm({}{}):\n", arm_ind, pat_str, guard_str));
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
            HirStmtKind::Error => {
                out.push_str(&format!("{ind}<ErrorStmt>\n"));
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
                let mut pat_str = String::new();
                pattern.pretty_print_to(&mut pat_str, 0, ctx);
                out.push_str(&format!("{pat_ind}Pattern: {pat_str}\n"));
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
                let b_strs: Vec<String> = ctx
                    .pool
                    .for_bindings_list(*bindings)
                    .iter()
                    .map(|b| {
                        format!(
                            "{}: {}",
                            super::symbol_name(ctx.symbols, b.symbol),
                            display_type(&b.ty, ctx)
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
                let bindings_slice = ctx.pool.bindings_list(*bindings);
                if bindings_slice.len() == 1 {
                    let b = &bindings_slice[0];
                    out.push_str(&format!(
                        "{}Var {}: {} =\n",
                        ind,
                        symbol_name(ctx.symbols, b.symbol),
                        display_type(&b.ty, ctx)
                    ));
                } else {
                    let b_strs: Vec<String> = bindings_slice
                        .iter()
                        .map(|b| {
                            format!(
                                "{}: {}",
                                symbol_name(ctx.symbols, b.symbol),
                                display_type(&b.ty, ctx)
                            )
                        })
                        .collect();
                    out.push_str(&format!("{}Var ({}) =\n", ind, b_strs.join(", ")));
                }
                value.pretty_print_to(out, indent + 1, ctx);
            }
            HirSimpleStmt::Set { places, op, value } => {
                let place_strs: Vec<String> = ctx
                    .pool
                    .places_list(*places)
                    .iter()
                    .map(|p| p.pretty_print(ctx))
                    .collect();
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
            HirExprKind::Error => "<ErrorExpr>".to_string(),
            HirExprKind::Path { symbol } => ctx.symbols.get(*symbol).name.to_string(),
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
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::TypePath { member_symbol, .. } => {
                out.push_str(&format!(
                    "{}TypePath({}): {}\n",
                    ind,
                    symbol_name(ctx.symbols, *member_symbol),
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::Generic { callee, args } => {
                let args_strs: Vec<String> = args.iter().map(|a| display_type(a, ctx)).collect();
                out.push_str(&format!(
                    "{}Generic<{}>: {}\n",
                    ind,
                    args_strs.join(", "),
                    display_type(&self.ty, ctx)
                ));
                callee.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Field { base, field } => {
                out.push_str(&format!(
                    "{}Field({}): {}\n",
                    ind,
                    field,
                    display_type(&self.ty, ctx)
                ));
                base.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::SafeField { base, field } => {
                out.push_str(&format!(
                    "{}SafeField({}): {}\n",
                    ind,
                    field,
                    display_type(&self.ty, ctx)
                ));
                base.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Index { base, index } => {
                out.push_str(&format!("{}Index: {}\n", ind, display_type(&self.ty, ctx)));
                base.pretty_print_to(out, indent + 1, ctx);
                index.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::SafeIndex { base, index } => {
                out.push_str(&format!(
                    "{}SafeIndex: {}\n",
                    ind,
                    display_type(&self.ty, ctx)
                ));
                base.pretty_print_to(out, indent + 1, ctx);
                index.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Try { expr } => {
                out.push_str(&format!("{}Try: {}\n", ind, display_type(&self.ty, ctx)));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Call {
                callee,
                args,
                trailing_block,
            } => {
                out.push_str(&format!("{}Call: {}\n", ind, display_type(&self.ty, ctx)));
                callee.pretty_print_to(out, indent + 1, ctx);
                for &a in ctx.pool.expr_list(*args) {
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
                    display_type(&self.ty, ctx)
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
                    display_type(&self.ty, ctx)
                ));
                let field_ind = "  ".repeat(indent + 1);
                for f in ctx.pool.field_inits_list(*fields) {
                    out.push_str(&format!("{}{}:\n", field_ind, f.name));
                    f.value.pretty_print_to(out, indent + 2, ctx);
                }
            }
            HirExprKind::Array { items } => {
                out.push_str(&format!("{}Array: {}\n", ind, display_type(&self.ty, ctx)));
                for &item in ctx.pool.expr_list(*items) {
                    item.pretty_print_to(out, indent + 1, ctx);
                }
            }
            HirExprKind::Lambda { params, body } => {
                let params_str: Vec<String> = ctx
                    .pool
                    .lambda_params_list(*params)
                    .iter()
                    .map(|p| {
                        format!(
                            "{}: {}",
                            symbol_name(ctx.symbols, p.symbol),
                            display_type(&p.ty, ctx)
                        )
                    })
                    .collect();
                out.push_str(&format!(
                    "{}Lambda({}): {}\n",
                    ind,
                    params_str.join(", "),
                    display_type(&self.ty, ctx)
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
                out.push_str(&format!("{}Alloc: {}\n", ind, display_type(&self.ty, ctx)));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::AsyncBlock { block } => {
                out.push_str(&format!(
                    "{}AsyncBlock: {}\n",
                    ind,
                    display_type(&self.ty, ctx)
                ));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::UnsafeBlock { block } => {
                out.push_str(&format!(
                    "{}UnsafeBlock: {}\n",
                    ind,
                    display_type(&self.ty, ctx)
                ));
                ctx.pool.block(*block).pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                out.push_str(&format!("{}If: {}\n", ind, display_type(&self.ty, ctx)));
                condition.pretty_print_to(out, indent + 1, ctx);
                let sub_ind = "  ".repeat(indent + 1);
                out.push_str(&format!("{sub_ind}Then\n"));
                ctx.pool
                    .block(*then_block)
                    .pretty_print_to(out, indent + 2, ctx);
                out.push_str(&format!("{sub_ind}Else\n"));
                ctx.pool
                    .block(*else_block)
                    .pretty_print_to(out, indent + 2, ctx);
            }
            HirExprKind::Match { value, arms } => {
                out.push_str(&format!("{}Match: {}\n", ind, display_type(&self.ty, ctx)));
                value.pretty_print_to(out, indent + 1, ctx);
                let arm_ind = "  ".repeat(indent + 1);
                for arm in ctx.pool.match_arms_list(*arms) {
                    let guard_str = if let Some(ref g) = arm.guard {
                        format!(" if {}", g.pretty_print_inline(ctx))
                    } else {
                        String::new()
                    };
                    let mut pat_str = String::new();
                    arm.pattern.pretty_print_to(&mut pat_str, 0, ctx);
                    out.push_str(&format!("{}Arm({}{}):\n", arm_ind, pat_str, guard_str));
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
                out.push_str(&format!("{}Catch: {}\n", ind, display_type(&self.ty, ctx)));
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
                    display_type(&self.ty, ctx)
                ));
                left.pretty_print_to(out, indent + 1, ctx);
                right.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Cast { expr, target_ty } => {
                out.push_str(&format!(
                    "{}Cast({}): {}\n",
                    ind,
                    display_type(target_ty, ctx),
                    display_type(&self.ty, ctx)
                ));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Unary { op, expr } => {
                out.push_str(&format!(
                    "{}Unary({}): {}\n",
                    ind,
                    unary_op_str(op),
                    display_type(&self.ty, ctx)
                ));
                expr.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Binary { op, left, right } => {
                out.push_str(&format!(
                    "{}Binary({}): {}\n",
                    ind,
                    op_str(op),
                    display_type(&self.ty, ctx)
                ));
                left.pretty_print_to(out, indent + 1, ctx);
                right.pretty_print_to(out, indent + 1, ctx);
            }
            HirExprKind::Int(v) => {
                out.push_str(&format!(
                    "{}Int({}): {}\n",
                    ind,
                    v,
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::Float(v) => {
                out.push_str(&format!(
                    "{}Float({}): {}\n",
                    ind,
                    v,
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::Bool(v) => {
                out.push_str(&format!(
                    "{}Bool({}): {}\n",
                    ind,
                    v,
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::Char(v) => {
                out.push_str(&format!(
                    "{}Char({}): {}\n",
                    ind,
                    v,
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::Str(v) => {
                out.push_str(&format!(
                    "{}Str({}): {}\n",
                    ind,
                    v,
                    display_type(&self.ty, ctx)
                ));
            }
            HirExprKind::Nil => {
                out.push_str(&format!("{}Nil: {}\n", ind, display_type(&self.ty, ctx)));
            }
            HirExprKind::Error => {
                out.push_str(&format!("{}Error: {}\n", ind, display_type(&self.ty, ctx)));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::pool::HirPool;
    use crate::hir::{
        HirBlock, HirConst, HirDecl, HirExpr, HirExprKind, HirFunc, HirStmt, HirStmtKind,
        IndexRange,
    };
    use crate::types::{ArType, Primitive};
    use crate::{ScopeId, SymbolKind, SymbolTable};
    use arandu_lexer::Span;

    fn make_ctx<'a>(pool: &'a HirPool, symbols: &'a SymbolTable) -> HirPrettyCtx<'a> {
        HirPrettyCtx {
            pool,
            symbols,
            show_spans: false,
            type_interner: None,
        }
    }

    #[test]
    fn pretty_print_empty_program() {
        let pool = HirPool::new();
        let symbols = SymbolTable::new(0);
        let program = HirProgram {
            span: Span::new(0, 0, 0),
            module: None,
            decls: Vec::new(),
            pool,
        };
        let ctx = make_ctx(&program.pool, &symbols);
        let out = program.pretty_print(&ctx);
        assert_eq!(out, "Program\n");
    }

    #[test]
    fn pretty_print_func_with_body() {
        let mut symbols = SymbolTable::new(0);
        let main_sym = symbols
            .define(ScopeId(0), "main", SymbolKind::Func, Span::new(0, 0, 0))
            .unwrap();
        let _x_sym = symbols
            .define(ScopeId(0), "x", SymbolKind::Param, Span::new(0, 0, 0))
            .unwrap();

        let mut pool = HirPool::new();
        let int_expr = pool.alloc_expr(HirExpr {
            kind: HirExprKind::Int("42".into()),
            ty: ArType::Primitive(Primitive::Int),
            span: Span::new(0, 0, 0),
        });
        let values = pool.alloc_expr_list(&[int_expr]);
        let ret_stmt = pool.alloc_stmt(HirStmt {
            kind: HirStmtKind::Return { values },
            span: Span::new(0, 0, 0),
        });
        let stmts = pool.alloc_stmt_list(&[ret_stmt]);
        let body = pool.alloc_block(HirBlock {
            statements: stmts,
            span: Span::new(0, 0, 0),
        });
        let func_decl = pool.alloc_decl(HirDecl::Func(HirFunc {
            symbol: main_sym,
            params: IndexRange::empty(),
            return_type: ArType::Primitive(Primitive::Int),
            body: Some(body),
            span: Span::new(0, 0, 0),
        }));

        let program = HirProgram {
            span: Span::new(0, 0, 0),
            module: None,
            decls: vec![func_decl],
            pool,
        };
        let ctx = make_ctx(&program.pool, &symbols);
        let out = program.pretty_print(&ctx);
        assert!(out.contains("Func main"));
        assert!(out.contains("Return"));
        assert!(out.contains("Int(42)"));
    }

    #[test]
    fn pretty_print_with_module() {
        let pool = HirPool::new();
        let symbols = SymbolTable::new(0);
        let program = HirProgram {
            span: Span::new(0, 0, 0),
            module: Some("mymod".into()),
            decls: Vec::new(),
            pool,
        };
        let ctx = make_ctx(&program.pool, &symbols);
        let out = program.pretty_print(&ctx);
        assert!(out.contains("Module mymod"));
    }

    #[test]
    fn pretty_print_multiple_decls() {
        let mut symbols = SymbolTable::new(0);
        let _a = symbols
            .define(ScopeId(0), "A", SymbolKind::Const, Span::new(0, 0, 0))
            .unwrap();
        let mut pool = HirPool::new();
        let val = pool.alloc_expr(HirExpr {
            kind: HirExprKind::Bool(true),
            ty: ArType::Primitive(Primitive::Bool),
            span: Span::new(0, 0, 0),
        });
        let decl_a = pool.alloc_decl(HirDecl::Const(HirConst {
            symbol: _a,
            ty: ArType::Primitive(Primitive::Bool),
            value: val,
            span: Span::new(0, 0, 0),
        }));
        let program = HirProgram {
            span: Span::new(0, 0, 0),
            module: None,
            decls: vec![decl_a],
            pool,
        };
        let ctx = make_ctx(&program.pool, &symbols);
        let out = program.pretty_print(&ctx);
        assert!(out.contains("Const A"));
        assert!(out.contains("Bool(true)"));
    }
}
