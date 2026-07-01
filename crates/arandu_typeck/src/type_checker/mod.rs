use rustc_hash::FxHashMap;

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

use arandu_middle::CompileSession;

#[must_use]
pub fn type_check_with_session(
    resolution: ResolutionResult,
    program: &Program,
    session: &mut CompileSession,
) -> TypeCheckResult {
    let mut checker = TypeChecker::new_with_interner(
        resolution.symbols,
        resolution.resolved,
        resolution.diagnostics,
        &program.pool,
        std::mem::take(&mut session.type_interner),
    );
    check::check_program(&mut checker, program);
    let res = checker.finish();
    session.type_interner = res.type_info.type_interner.clone();
    res
}

#[must_use]
pub fn type_check(resolution: ResolutionResult, program: &Program) -> TypeCheckResult {
    let mut session = CompileSession::new();
    type_check_with_session(resolution, program, &mut session)
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
        Self::new_with_interner(symbols, resolved, diagnostics, pool, TypeInterner::new())
    }

    #[must_use]
    pub fn new_with_interner(
        symbols: SymbolTable,
        resolved: ResolvedNames,
        diagnostics: Vec<Diagnostic>,
        pool: &'a AstPool,
        type_interner: TypeInterner,
    ) -> Self {
        Self {
            symbols,
            resolved,
            ctx: TyCtx::new(),
            type_info: TypeInfo::with_interner(type_interner),
            diagnostics,
            type_scope_id: None,
            pool,
        }
    }

    pub fn intern(&mut self, ty: ArType) -> TypeId {
        self.type_info.type_interner.intern(ty)
    }

    #[must_use]
    pub fn resolve(&self, id: TypeId) -> &ArType {
        self.type_info.type_interner.resolve(id)
    }

    #[must_use]
    pub fn is_result_type(&self, ty: &ArType) -> bool {
        types::is_result_type(ty, &self.type_info.type_interner)
    }

    #[must_use]
    pub fn result_ok_err(&self, ty: &ArType) -> Option<(ArType, ArType)> {
        types::result_ok_err(ty, &self.type_info.type_interner)
    }

    #[must_use]
    pub fn result_ok_err_ids(&self, id: TypeId) -> Option<(TypeId, TypeId)> {
        match self.type_info.type_interner.resolve(id) {
            ArType::Result(ok, err) => Some((*ok, *err)),
            _ => None,
        }
    }

    #[must_use]
    pub fn try_ok_type(&self, ty: &ArType) -> Option<ArType> {
        types::try_ok_type(ty, &self.type_info.type_interner)
    }

    #[must_use]
    pub fn try_ok_type_id(&self, id: TypeId) -> Option<TypeId> {
        match self.type_info.type_interner.resolve(id) {
            ArType::Result(ok, _) => Some(*ok),
            ArType::Option(inner) => Some(*inner),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_result_type_id(&self, id: TypeId) -> bool {
        self.is_result_type(self.resolve(id))
    }

    #[must_use]
    pub fn is_err_type(&self, ty: &ArType) -> bool {
        types::is_err_type(ty, &self.type_info.type_interner)
    }

    #[must_use]
    pub fn unify_return(&self, expected: &ArType, actual: &ArType) -> bool {
        types::unify_return(expected, actual, &self.type_info.type_interner)
    }

    pub fn lower_type_expr(
        &mut self,
        expr_id: arandu_parser::TypeExprId,
        scope: ScopeId,
    ) -> ArType {
        let ctx = types::LowerCtx {
            pool: self.pool,
            symbols: &self.symbols,
            scope,
            resolved: &self.resolved,
        };
        arandu_middle::types::lower::lower_type_expr_ctx(expr_id, &ctx, &mut self.type_info.type_interner)
    }

    pub fn lower_result_type(
        &mut self,
        result: &arandu_parser::ResultType,
        scope: ScopeId,
    ) -> ArType {
        let ctx = types::LowerCtx {
            pool: self.pool,
            symbols: &self.symbols,
            scope,
            resolved: &self.resolved,
        };
        arandu_middle::types::lower::lower_result_type_ctx(result, &ctx, &mut self.type_info.type_interner)
    }

    pub fn lower_named_type(
        &mut self,
        span: arandu_lexer::Span,
        name: &arandu_parser::TypeName,
        args: &[arandu_parser::TypeExprId],
        scope: ScopeId,
    ) -> ArType {
        let ctx = types::LowerCtx {
            pool: self.pool,
            symbols: &self.symbols,
            scope,
            resolved: &self.resolved,
        };
        types::lower_named_type(span, name, args, &ctx, &mut self.type_info.type_interner)
    }

    /// Scope used when lowering type expressions in the current context.
    #[must_use]
    pub(crate) fn type_scope(&self) -> ScopeId {
        self.type_scope_id
            .unwrap_or_else(|| self.symbols.global_scope())
    }
}

pub enum ArTypeOrId {
    Type(ArType),
    Id(TypeId),
}

impl From<ArType> for ArTypeOrId {
    fn from(t: ArType) -> Self {
        ArTypeOrId::Type(t)
    }
}

impl From<TypeId> for ArTypeOrId {
    fn from(id: TypeId) -> Self {
        ArTypeOrId::Id(id)
    }
}

impl TypeChecker<'_> {
    /// Add a type constraint failure. Translates it immediately into a
    /// flow-based error message and pushes it to the diagnostics list.
    pub fn add_constraint(
        &mut self,
        expected: impl Into<ArTypeOrId>,
        found: impl Into<ArTypeOrId>,
        origin: ConstraintOrigin,
    ) {
        let expected = match expected.into() {
            ArTypeOrId::Type(t) => t,
            ArTypeOrId::Id(id) => self.resolve(id).clone(),
        };
        let found = match found.into() {
            ArTypeOrId::Type(t) => t,
            ArTypeOrId::Id(id) => self.resolve(id).clone(),
        };
        let constraint = Constraint {
            expected,
            found,
            origin,
        };
        let diag = errors::constraint_to_diagnostic(&constraint, &self.symbols, &self.type_info);
        self.diagnostics.push(diag);
    }

    pub(crate) fn record_expr_type(&mut self, expr: ExprId, id: TypeId) {
        self.type_info.record_expr_type(expr, id);
    }

    pub(crate) fn record_decl_type(&mut self, symbol: SymbolId, id: TypeId) {
        self.type_info.record_decl_type(symbol, id);
    }

    #[must_use]
    pub(crate) fn expr_type_id(&self, expr: ExprId) -> Option<TypeId> {
        self.type_info.expr_type_id(expr)
    }

    #[must_use]
    pub(crate) fn decl_type(&self, symbol: SymbolId) -> Option<ArType> {
        self.type_info.decl_type(symbol).cloned()
    }

    #[must_use]
    pub(crate) fn decl_type_id(&self, symbol: SymbolId) -> Option<TypeId> {
        self.type_info.decl_type_id(symbol)
    }

    pub(crate) fn unify_ids(&self, a: TypeId, b: TypeId) -> bool {
        if a == b {
            return true;
        }
        let a_ty = self.type_info.resolve_type_id(a);
        let b_ty = self.type_info.resolve_type_id(b);
        types::unify(a_ty, b_ty, &self.type_info.type_interner)
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
    pub struct_field_indices: FxHashMap<SymbolId, FxHashMap<String, usize>>,
    pub enum_variants: FxHashMap<SymbolId, (SymbolId, EnumPayloadShape)>,
    /// Pre-computed discriminant tag for each enum variant symbol.
    ///
    /// Populated during `collect_type_shapes` to allow the AMIR lowering pass
    /// to resolve `variant_symbol → tag` in O(1) without scanning HIR decls.
    pub enum_variant_tags: FxHashMap<SymbolId, usize>,
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
        Self::with_interner(TypeInterner::new())
    }

    #[must_use]
    pub fn with_interner(type_interner: TypeInterner) -> Self {
        Self {
            type_interner,
            expr_types: Vec::new(),
            decl_types: FxHashMap::default(),
            struct_fields: FxHashMap::default(),
            struct_field_symbols: FxHashMap::default(),
            struct_field_indices: FxHashMap::default(),
            enum_variants: FxHashMap::default(),
            enum_variant_tags: FxHashMap::default(),
            generic_params: FxHashMap::default(),
            param_constraints: FxHashMap::default(),
            interfaces: FxHashMap::default(),
        }
    }

    /// Record the discriminant tag index for an enum variant.
    ///
    /// Called once per variant during `collect_type_shapes`. The stored tag
    /// is the 0-based declaration order index, which matches the value emitted
    /// by `AmirRvalue::Discriminant` and used by `SwitchInt` in the backend.
    pub fn record_enum_variant_tag(&mut self, variant: SymbolId, tag: usize) {
        self.enum_variant_tags.insert(variant, tag);
    }
}

fn translate_type(ty: &ArType, from: &TypeInterner, to: &mut TypeInterner) -> ArType {
    match ty {
        ArType::Primitive(p) => ArType::Primitive(*p),
        ArType::Named(id, args) => {
            let new_args = args
                .iter()
                .map(|&arg_id| {
                    let resolved = from.resolve(arg_id);
                    let translated = translate_type(resolved, from, to);
                    to.intern(translated)
                })
                .collect();
            ArType::Named(*id, new_args)
        }
        ArType::Func(params, ret) => {
            let new_params = params
                .iter()
                .map(|&param_id| {
                    let resolved = from.resolve(param_id);
                    let translated = translate_type(resolved, from, to);
                    to.intern(translated)
                })
                .collect();
            let resolved_ret = from.resolve(*ret);
            let translated_ret = translate_type(resolved_ret, from, to);
            let new_ret = to.intern(translated_ret);
            ArType::Func(new_params, new_ret)
        }
        ArType::Nullable(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Nullable(new_inner)
        }
        ArType::Slice(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Slice(new_inner)
        }
        ArType::Array(n, inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Array(*n, new_inner)
        }
        ArType::Ptr(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Ptr(new_inner)
        }
        ArType::Tuple(items) => {
            let new_items = items
                .iter()
                .map(|&item_id| {
                    let resolved = from.resolve(item_id);
                    let translated = translate_type(resolved, from, to);
                    to.intern(translated)
                })
                .collect();
            ArType::Tuple(new_items)
        }
        ArType::Result(ok, err) => {
            let resolved_ok = from.resolve(*ok);
            let translated_ok = translate_type(resolved_ok, from, to);
            let new_ok = to.intern(translated_ok);

            let resolved_err = from.resolve(*err);
            let translated_err = translate_type(resolved_err, from, to);
            let new_err = to.intern(translated_err);

            ArType::Result(new_ok, new_err)
        }
        ArType::Option(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Option(new_inner)
        }
        ArType::Coroutine(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Coroutine(new_inner)
        }
        ArType::Range(inner) => {
            let resolved = from.resolve(*inner);
            let translated = translate_type(resolved, from, to);
            let new_inner = to.intern(translated);
            ArType::Range(new_inner)
        }
        ArType::Err => ArType::Err,
        ArType::Void => ArType::Void,
        ArType::Error => ArType::Error,
        ArType::IntLiteral => ArType::IntLiteral,
        ArType::FloatLiteral => ArType::FloatLiteral,
    }
}

impl TypeInfo {
    pub fn merge_from(&mut self, other: &TypeInfo) {
        // We cannot use a closure that borrows `self` mutably and also references `other`
        // due to borrow checker rules. Instead, pass other and self to a helper function.
        for (&symbol, &other_type_id) in &other.decl_types {
            let other_type = other.type_interner.resolve(other_type_id);
            let translated =
                translate_type(other_type, &other.type_interner, &mut self.type_interner);
            let id = self.type_interner.intern(translated);
            self.record_decl_type(symbol, id);
        }
        for (symbol, fields) in &other.struct_fields {
            let mut translated_fields = FxHashMap::default();
            for (name, ty) in fields {
                let translated = translate_type(ty, &other.type_interner, &mut self.type_interner);
                translated_fields.insert(name.clone(), translated);
            }
            self.struct_fields.insert(*symbol, translated_fields);
        }
        for (symbol, field_symbols) in &other.struct_field_symbols {
            self.struct_field_symbols
                .insert(*symbol, field_symbols.clone());
        }
        for (symbol, field_indices) in &other.struct_field_indices {
            self.struct_field_indices
                .insert(*symbol, field_indices.clone());
        }
        for (symbol, (enum_id, shape)) in &other.enum_variants {
            let translated_shape = match shape {
                EnumPayloadShape::Unit => EnumPayloadShape::Unit,
                EnumPayloadShape::Tuple(tys) => {
                    let mut new_tys = Vec::new();
                    for ty in tys {
                        new_tys.push(translate_type(
                            ty,
                            &other.type_interner,
                            &mut self.type_interner,
                        ));
                    }
                    EnumPayloadShape::Tuple(new_tys)
                }
            };
            self.enum_variants
                .insert(*symbol, (*enum_id, translated_shape));
        }
        for (&symbol, &tag) in &other.enum_variant_tags {
            self.enum_variant_tags.insert(symbol, tag);
        }
        for (symbol, params) in &other.generic_params {
            self.generic_params.insert(*symbol, params.clone());
        }
        for (symbol, constraints) in &other.param_constraints {
            self.param_constraints.insert(*symbol, constraints.clone());
        }
        for (symbol, interface_info) in &other.interfaces {
            let mut translated_methods = Vec::new();
            for (name, ty) in &interface_info.methods {
                let translated = translate_type(ty, &other.type_interner, &mut self.type_interner);
                translated_methods.push((name.clone(), translated));
            }
            self.interfaces.insert(
                *symbol,
                types::InterfaceInfo {
                    methods: translated_methods,
                },
            );
        }
    }

    pub fn record_expr_type(&mut self, expr: ExprId, id: TypeId) {
        let idx = expr.as_usize();
        if self.expr_types.len() <= idx {
            self.expr_types.resize(idx + 1, None);
        }
        self.expr_types[idx] = Some(id);
    }

    pub fn record_decl_type(&mut self, symbol: SymbolId, id: TypeId) {
        self.decl_types.insert(symbol, id);
    }

    #[must_use]
    pub fn expr_type(&self, expr: ExprId) -> Option<&ArType> {
        self.expr_types
            .get(expr.as_usize())
            .and_then(|id| id.as_ref().map(|id| self.type_interner.resolve(*id)))
    }

    #[must_use]
    pub fn expr_type_id(&self, expr: ExprId) -> Option<TypeId> {
        self.expr_types.get(expr.as_usize()).copied().flatten()
    }

    #[must_use]
    pub fn decl_type(&self, symbol: SymbolId) -> Option<&ArType> {
        self.decl_types
            .get(&symbol)
            .map(|id| self.type_interner.resolve(*id))
    }

    #[must_use]
    pub fn decl_type_id(&self, symbol: SymbolId) -> Option<TypeId> {
        self.decl_types.get(&symbol).copied()
    }

    #[must_use]
    pub fn resolve_type_id(&self, id: TypeId) -> &ArType {
        self.type_interner.resolve(id)
    }
}

#[derive(Clone)]
pub struct TypeCheckResult {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub type_info: TypeInfo,
    pub diagnostics: Vec<Diagnostic>,
}

impl arandu_middle::layout::StructLayoutProvider for TypeInfo {
    fn get_struct_fields(
        &self,
        struct_id: SymbolId,
    ) -> Option<&rustc_hash::FxHashMap<String, ArType>> {
        self.struct_fields.get(&struct_id)
    }

    fn get_struct_field_indices(
        &self,
        struct_id: SymbolId,
    ) -> Option<&rustc_hash::FxHashMap<String, usize>> {
        self.struct_field_indices.get(&struct_id)
    }

    fn get_generic_params(&self, struct_id: SymbolId) -> Option<&[SymbolId]> {
        self.generic_params.get(&struct_id).map(|v| v.as_slice())
    }
}
