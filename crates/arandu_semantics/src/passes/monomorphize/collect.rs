use arandu_diagnostics::{DiagCode, Diagnostic};
use arandu_lexer::Span;
use arandu_middle::hir::{
    HirCatchHandler, HirCondition, HirDecl, HirExprId, HirExprKind, HirLambdaBody, HirMatchArmBody,
    HirPool, HirProgram, HirSimpleStmt, HirStmt, HirStmtKind,
};
use arandu_middle::symbol_table::SymbolId;
use arandu_middle::types::{ArType, TypeInterner};
use arandu_typeck::TypeCheckResult;

use super::graph::{InstantiationGraph, InstantiationKey, InstantiationNodeId, MonoError};

#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(tc, hir))]
pub fn analyze_instantiations(
    tc: &TypeCheckResult,
    hir: &HirProgram,
) -> Result<InstantiationGraph, Vec<Diagnostic>> {
    let mut analyzer = InstantiationAnalyzer {
        tc,
        hir,
        interner: &tc.type_info.type_interner,
        graph: InstantiationGraph::new(),
        diagnostics: Vec::new(),
    };

    for &decl_id in &hir.decls {
        let decl = hir.pool.decl(decl_id);
        if let HirDecl::Func(func) = decl
            && let Some(body_id) = func.body
        {
            let current = analyzer.current_generic_node(func.symbol);
            analyzer.visit_block(body_id, current);
        }
    }

    if let Some(cycle) = analyzer.graph.find_cycle() {
        let names: Vec<String> = cycle
            .iter()
            .map(|node| analyzer.graph.get_node(*node).mangled_name.clone())
            .collect();
        analyzer.diagnostics.push(Diagnostic::error(
            DiagCode::G001GenericInstantiationCycle,
            format!(
                "generic instantiation cycle detected: {}",
                names.join(" -> ")
            ),
            Span::new(0, 0, 0),
        ));
    }

    if analyzer.diagnostics.is_empty() {
        Ok(analyzer.graph)
    } else {
        Err(analyzer.diagnostics)
    }
}

struct InstantiationAnalyzer<'a> {
    tc: &'a TypeCheckResult,
    hir: &'a HirProgram,
    interner: &'a TypeInterner,
    graph: InstantiationGraph,
    diagnostics: Vec<Diagnostic>,
}

impl InstantiationAnalyzer<'_> {
    fn current_generic_node(&mut self, symbol: SymbolId) -> Option<InstantiationNodeId> {
        let params = self.tc.type_info.generic_params.get(&symbol)?;
        let type_args = params
            .iter()
            .map(|param| self.interner.intern(ArType::Named(*param, Vec::new())))
            .collect();
        self.insert_key(InstantiationKey { symbol, type_args }, Span::new(0, 0, 0))
    }

    fn insert_key(&mut self, key: InstantiationKey, span: Span) -> Option<InstantiationNodeId> {
        match self
            .graph
            .get_or_insert(&key, self.interner, &self.tc.symbols)
        {
            Ok(node) => Some(node),
            Err(MonoError::RecursionLimitExceeded { symbol, limit }) => {
                self.diagnostics.push(Diagnostic::error(
                    DiagCode::G002GenericInstantiationLimit,
                    format!(
                        "generic instantiation recursion limit exceeded for `{}` (limit {limit})",
                        self.tc.symbols.get(symbol).name
                    ),
                    span,
                ));
                None
            }
        }
    }

    fn visit_block(
        &mut self,
        block: arandu_middle::hir::HirBlockId,
        current: Option<InstantiationNodeId>,
    ) {
        let blk = self.hir.pool.block(block);
        for &stmt_id in self.hir.pool.stmt_list(blk.statements) {
            let stmt = self.hir.pool.stmt(stmt_id);
            self.visit_stmt(stmt, current);
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStmt, current: Option<InstantiationNodeId>) {
        match &stmt.kind {
            HirStmtKind::VarDecl { value, .. }
            | HirStmtKind::Expr(value)
            | HirStmtKind::Free(value) => self.visit_expr(*value, current),
            HirStmtKind::Set { value, .. } => self.visit_expr(*value, current),
            HirStmtKind::Return { values } => {
                for &value in self.hir.pool.expr_list(*values) {
                    self.visit_expr(value, current);
                }
            }
            HirStmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.visit_condition(condition, current);
                self.visit_block(*then_block, current);
                if let Some(block) = else_block {
                    self.visit_block(*block, current);
                }
            }
            HirStmtKind::For { clause, body } => {
                match clause {
                    arandu_middle::hir::HirForClause::In { iterable, .. } => {
                        self.visit_expr(*iterable, current);
                    }
                    arandu_middle::hir::HirForClause::CStyle {
                        init,
                        condition,
                        step,
                        ..
                    } => {
                        if let Some(init) = init {
                            self.visit_simple_stmt(init, current);
                        }
                        if let Some(condition) = condition {
                            self.visit_expr(*condition, current);
                        }
                        if let Some(step) = step {
                            self.visit_simple_stmt(step, current);
                        }
                    }
                }
                self.visit_block(*body, current);
            }
            HirStmtKind::While { condition, body } => {
                self.visit_condition(condition, current);
                self.visit_block(*body, current);
            }
            HirStmtKind::Match { value, arms } => {
                self.visit_expr(*value, current);
                for arm in self.hir.pool.match_arms_list(*arms) {
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(*guard, current);
                    }
                    match &arm.body {
                        HirMatchArmBody::Expr(expr) => self.visit_expr(*expr, current),
                        HirMatchArmBody::Block(block) => self.visit_block(*block, current),
                    }
                }
            }
            HirStmtKind::Defer(block)
            | HirStmtKind::ErrDefer(block)
            | HirStmtKind::Unsafe(block) => {
                self.visit_block(*block, current);
            }
            HirStmtKind::Break | HirStmtKind::Continue | HirStmtKind::Error => {}
        }
    }

    fn visit_simple_stmt(&mut self, stmt: &HirSimpleStmt, current: Option<InstantiationNodeId>) {
        match stmt {
            HirSimpleStmt::VarDecl { value, .. }
            | HirSimpleStmt::Set { value, .. }
            | HirSimpleStmt::Expr(value) => self.visit_expr(*value, current),
        }
    }

    fn visit_condition(&mut self, condition: &HirCondition, current: Option<InstantiationNodeId>) {
        match condition {
            HirCondition::Expr(expr) | HirCondition::Is { expr, .. } => {
                self.visit_expr(*expr, current);
            }
        }
    }

    fn visit_expr(&mut self, expr_id: HirExprId, current: Option<InstantiationNodeId>) {
        let expr = self.hir.pool.expr(expr_id);
        match &expr.kind {
            HirExprKind::Generic { callee, args } => {
                self.visit_expr(*callee, current);
                if let Some(symbol) = generic_callee_symbol(*callee, &self.hir.pool) {
                    let type_args = args
                        .iter()
                        .cloned()
                        .map(|ty| self.interner.intern(ty))
                        .collect();
                    let key = InstantiationKey { symbol, type_args };
                    if let Some(callee_node) = self.insert_key(key, expr.span)
                        && let Some(caller_node) = current
                    {
                        self.graph.add_edge(caller_node, callee_node);
                    }
                }
            }
            HirExprKind::Field { base, .. }
            | HirExprKind::SafeField { base, .. }
            | HirExprKind::Alloc { expr: base }
            | HirExprKind::Try { expr: base }
            | HirExprKind::Cast { expr: base, .. }
            | HirExprKind::Unary { expr: base, .. } => self.visit_expr(*base, current),
            HirExprKind::Index { base, index } | HirExprKind::SafeIndex { base, index } => {
                self.visit_expr(*base, current);
                self.visit_expr(*index, current);
            }
            HirExprKind::Call {
                callee,
                args,
                trailing_block,
            } => {
                self.visit_expr(*callee, current);
                for &arg in self.hir.pool.expr_list(*args) {
                    self.visit_expr(arg, current);
                }
                if let Some(block) = trailing_block {
                    self.visit_block(*block, current);
                }
            }
            HirExprKind::ResultCtor { value, .. } => self.visit_expr(*value, current),
            HirExprKind::StructLiteral { fields, .. } => {
                for field in self.hir.pool.field_inits_list(*fields) {
                    self.visit_expr(field.value, current);
                }
            }
            HirExprKind::Array { items } => {
                for &item in self.hir.pool.expr_list(*items) {
                    self.visit_expr(item, current);
                }
            }
            HirExprKind::Lambda { body, .. } => match body {
                HirLambdaBody::Expr(expr) => self.visit_expr(*expr, current),
                HirLambdaBody::Block(block) => self.visit_block(*block, current),
            },
            HirExprKind::AsyncBlock { block } | HirExprKind::UnsafeBlock { block } => {
                self.visit_block(*block, current);
            }
            HirExprKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.visit_condition(condition, current);
                self.visit_block(*then_block, current);
                self.visit_block(*else_block, current);
            }
            HirExprKind::Match { value, arms } => {
                self.visit_expr(*value, current);
                for arm in self.hir.pool.match_arms_list(*arms) {
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(*guard, current);
                    }
                    match &arm.body {
                        HirMatchArmBody::Expr(expr) => self.visit_expr(*expr, current),
                        HirMatchArmBody::Block(block) => self.visit_block(*block, current),
                    }
                }
            }
            HirExprKind::Catch { expr, handler } => {
                self.visit_expr(*expr, current);
                match handler {
                    HirCatchHandler::Expr(expr) => self.visit_expr(*expr, current),
                    HirCatchHandler::Block { block, .. } => self.visit_block(*block, current),
                }
            }
            HirExprKind::NullCoalesce { left, right } | HirExprKind::Binary { left, right, .. } => {
                self.visit_expr(*left, current);
                self.visit_expr(*right, current);
            }
            HirExprKind::Path { .. }
            | HirExprKind::TypePath { .. }
            | HirExprKind::Int(_)
            | HirExprKind::Float(_)
            | HirExprKind::Bool(_)
            | HirExprKind::Char(_)
            | HirExprKind::Str(_)
            | HirExprKind::Nil
            | HirExprKind::Error => {}
        }
    }
}

fn generic_callee_symbol(callee_id: HirExprId, pool: &HirPool) -> Option<SymbolId> {
    let callee = pool.expr(callee_id);
    match &callee.kind {
        HirExprKind::Path { symbol } => Some(*symbol),
        _ => None,
    }
}
