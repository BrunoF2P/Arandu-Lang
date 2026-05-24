use std::collections::HashMap;

use arandu_parser::Program;

use crate::{Diagnostic, NodeKey, ResolutionResult, ResolvedNames, ScopeId, SymbolId, SymbolTable};

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
    );
    check::check_program(&mut checker, program);
    checker.finish()
}

// ── TypeChecker state ───────────────────────────────────────────────

pub struct TypeChecker {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub ctx: TyCtx,
    pub type_info: TypeInfo,
    pub diagnostics: Vec<Diagnostic>,
    /// Scope for lowering type expressions inside the current function body.
    type_scope_id: Option<ScopeId>,
}

impl TypeChecker {
    #[must_use]
    pub fn new(
        symbols: SymbolTable,
        resolved: ResolvedNames,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            symbols,
            resolved,
            ctx: TyCtx::new(),
            type_info: TypeInfo::new(),
            diagnostics,
            type_scope_id: None,
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
        let diag = errors::constraint_to_diagnostic(&constraint, &self.symbols);
        self.diagnostics.push(diag);
    }

    pub(crate) fn record_expr_type(&mut self, key: NodeKey, ty: ArType) -> TypeId {
        self.type_info.record_expr_type(key, ty)
    }

    pub(crate) fn record_decl_type(&mut self, symbol: SymbolId, ty: ArType) -> TypeId {
        self.type_info.record_decl_type(symbol, ty)
    }

    #[must_use]
    pub(crate) fn expr_type(&self, key: NodeKey) -> Option<ArType> {
        self.type_info.expr_type(key).cloned()
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
    pub expr_types: HashMap<NodeKey, TypeId>,
    pub decl_types: HashMap<SymbolId, TypeId>,
    pub struct_fields: HashMap<SymbolId, HashMap<String, ArType>>,
    pub struct_field_symbols: HashMap<SymbolId, HashMap<String, SymbolId>>,
    pub enum_variants: HashMap<SymbolId, (SymbolId, EnumPayloadShape)>,
    /// Ordered type-parameter symbols for generic decls (func, struct, …).
    pub generic_params: HashMap<SymbolId, Vec<SymbolId>>,
    /// Type-parameter symbol → interface symbols required (`T: Display`).
    pub param_constraints: HashMap<SymbolId, Vec<SymbolId>>,
    /// Interface symbol → method signatures (nominal, Go-style structural check).
    pub(crate) interfaces: HashMap<SymbolId, types::InterfaceInfo>,
}

impl TypeInfo {
    #[must_use]
    pub fn new() -> Self {
        Self {
            type_interner: TypeInterner::new(),
            expr_types: HashMap::new(),
            decl_types: HashMap::new(),
            struct_fields: HashMap::new(),
            struct_field_symbols: HashMap::new(),
            enum_variants: HashMap::new(),
            generic_params: HashMap::new(),
            param_constraints: HashMap::new(),
            interfaces: HashMap::new(),
        }
    }

    pub fn record_expr_type(&mut self, key: NodeKey, ty: ArType) -> TypeId {
        let id = self.type_interner.intern(ty);
        self.expr_types.insert(key, id);
        id
    }

    pub fn record_decl_type(&mut self, symbol: SymbolId, ty: ArType) -> TypeId {
        let id = self.type_interner.intern(ty);
        self.decl_types.insert(symbol, id);
        id
    }

    #[must_use]
    pub fn expr_type(&self, key: NodeKey) -> Option<&ArType> {
        self.expr_types
            .get(&key)
            .map(|id| self.type_interner.resolve(*id))
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
