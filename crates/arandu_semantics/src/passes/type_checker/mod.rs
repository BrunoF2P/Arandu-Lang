use fxhash::FxHashMap;

use arandu_parser::Program;
use arandu_parser::ast_pool::{AstPool, ExprId};

use crate::{Diagnostic, ResolutionResult, ResolvedNames, ScopeId, SymbolId, SymbolTable};

pub mod check;
pub mod constraints;
pub mod context;
pub mod errors;
pub mod synth;
pub mod types;

use constraints::{Constraint, ConstraintOrigin};
use context::TyCtx;
use types::{ArType, TypeId, TypeInterner};

// ── Entry point ─────────────────────────────────────────────────────

#[must_use]
pub fn type_check(resolution: ResolutionResult, program: &Program) -> TypeCheckResult {
    let mut checker = TypeChecker::new(
        resolution.symbols,
        resolution.resolved,
        resolution.diagnostics,
        &program.pool,
    );
    check::check_program(&mut checker, program);
    checker.finish()
}

// ── TypeChecker state ───────────────────────────────────────────────

pub struct TypeChecker<'a> {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub ctx: TyCtx,
    pub type_info: TypeInfo,
    pub diagnostics: Vec<Diagnostic>,
    /// Scope for lowering type expressions inside the current function body.
    type_scope_id: Option<ScopeId>,
    pub pool: &'a AstPool,
}

impl<'a> TypeChecker<'a> {
    #[must_use]
    pub fn new(
        symbols: SymbolTable,
        resolved: ResolvedNames,
        diagnostics: Vec<Diagnostic>,
        pool: &'a AstPool,
    ) -> Self {
        Self {
            symbols,
            resolved,
            ctx: TyCtx::new(),
            type_info: TypeInfo::new(),
            diagnostics,
            type_scope_id: None,
            pool,
        }
    }

    /// Scope used when lowering type expressions in the current context.
    #[must_use]
    pub(crate) fn type_scope(&self) -> ScopeId {
        self.type_scope_id
            .unwrap_or_else(|| self.symbols.global_scope())
    }

    /// Add a type constraint failure. Translates it immediately into a
    /// flow-based error message and pushes it to the diagnostics list.
    pub fn add_constraint(&mut self, expected: ArType, found: ArType, origin: ConstraintOrigin) {
        let constraint = Constraint {
            expected,
            found,
            origin,
        };
        let diag = errors::constraint_to_diagnostic(&constraint, &self.symbols, &self.type_info);
        self.diagnostics.push(diag);
    }

    pub(crate) fn record_expr_type(&mut self, expr: ExprId, ty: ArType) -> TypeId {
        self.type_info.record_expr_type(expr, ty)
    }

    pub(crate) fn record_decl_type(&mut self, symbol: SymbolId, ty: ArType) -> TypeId {
        self.type_info.record_decl_type(symbol, ty)
    }

    #[must_use]
    pub(crate) fn expr_type(&self, expr: ExprId) -> Option<ArType> {
        self.type_info.expr_type(expr).cloned()
    }

    #[must_use]
    pub(crate) fn decl_type(&self, symbol: SymbolId) -> Option<ArType> {
        self.type_info.decl_type(symbol).cloned()
    }

    #[must_use]
    pub fn finish(self) -> TypeCheckResult {
        TypeCheckResult {
            symbols: self.symbols,
            resolved: self.resolved,
            type_info: self.type_info,
            diagnostics: self.diagnostics,
        }
    }
}

// ── Results ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum EnumPayloadShape {
    Unit,
    Tuple(Vec<ArType>),
}

#[derive(Debug, Clone, Default)]
pub struct TypeInfo {
    pub type_interner: TypeInterner,
    pub expr_types: Vec<Option<TypeId>>,
    pub decl_types: FxHashMap<SymbolId, TypeId>,
    pub struct_fields: FxHashMap<SymbolId, FxHashMap<String, ArType>>,
    pub struct_field_symbols: FxHashMap<SymbolId, FxHashMap<String, SymbolId>>,
    pub enum_variants: FxHashMap<SymbolId, (SymbolId, EnumPayloadShape)>,
    /// Ordered type-parameter symbols for generic decls (func, struct, …).
    pub generic_params: FxHashMap<SymbolId, Vec<SymbolId>>,
    /// Type-parameter symbol → interface symbols required (`T: Display`).
    pub param_constraints: FxHashMap<SymbolId, Vec<SymbolId>>,
    /// Interface symbol → method signatures (nominal, Go-style structural check).
    pub(crate) interfaces: FxHashMap<SymbolId, types::InterfaceInfo>,
}

impl TypeInfo {
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_interner: TypeInterner::new(),
            expr_types: Vec::new(),
            decl_types: FxHashMap::default(),
            struct_fields: FxHashMap::default(),
            struct_field_symbols: FxHashMap::default(),
            enum_variants: FxHashMap::default(),
            generic_params: FxHashMap::default(),
            param_constraints: FxHashMap::default(),
            interfaces: FxHashMap::default(),
        }
    }

    pub fn record_expr_type(&mut self, expr: ExprId, ty: ArType) -> TypeId {
        let id = self.type_interner.intern(ty);
        let idx = expr.as_usize();
        if self.expr_types.len() <= idx {
            self.expr_types.resize(idx + 1, None);
        }
        self.expr_types[idx] = Some(id);
        id
    }

    pub fn record_decl_type(&mut self, symbol: SymbolId, ty: ArType) -> TypeId {
        let id = self.type_interner.intern(ty);
        self.decl_types.insert(symbol, id);
        id
    }

    #[must_use]
    pub fn expr_type(&self, expr: ExprId) -> Option<&ArType> {
        self.expr_types
            .get(expr.as_usize())
            .and_then(|id| id.as_ref().map(|id| self.type_interner.resolve(*id)))
    }

    #[must_use]
    pub fn decl_type(&self, symbol: SymbolId) -> Option<&ArType> {
        self.decl_types
            .get(&symbol)
            .map(|id| self.type_interner.resolve(*id))
    }

    #[must_use]
    pub fn resolve_type_id(&self, id: TypeId) -> &ArType {
        self.type_interner.resolve(id)
    }
}

pub struct TypeCheckResult {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub type_info: TypeInfo,
    pub diagnostics: Vec<Diagnostic>,
}
