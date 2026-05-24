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
use types::ArType;

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
    pub expr_types: HashMap<NodeKey, ArType>,
    pub decl_types: HashMap<SymbolId, ArType>,
    pub struct_fields: HashMap<SymbolId, HashMap<String, ArType>>,
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
            expr_types: HashMap::new(),
            decl_types: HashMap::new(),
            struct_fields: HashMap::new(),
            enum_variants: HashMap::new(),
            generic_params: HashMap::new(),
            param_constraints: HashMap::new(),
            interfaces: HashMap::new(),
        }
    }
}

pub struct TypeCheckResult {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub type_info: TypeInfo,
    pub diagnostics: Vec<Diagnostic>,
}
