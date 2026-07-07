use arandu_parser::Program;
use arandu_parser::ast_pool::{AstPool, ExprId};

use crate::{Diagnostic, ResolutionResult, ResolvedNames, ScopeId, SymbolId, SymbolTable};

pub mod check;
pub mod constraints;
pub mod context;
pub mod errors;
pub mod info;
pub mod synth;
pub mod types;

pub use info::{EnumPayloadShape, TypeInfo};

use constraints::{Constraint, ConstraintOrigin};
use context::TyCtx;
use types::{ArType, TypeId, TypeInterner};

// ── Results ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TypeCheckResult {
    pub symbols: SymbolTable,
    pub resolved: ResolvedNames,
    pub type_info: TypeInfo,
    pub diagnostics: Vec<Diagnostic>,
}

// ── Entry point ─────────────────────────────────────────────────────

/// Standalone type-check for tests or isolated modules.
/// Runs both signature and body checks.
#[must_use]
#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(resolution, program))]
pub fn type_check(resolution: ResolutionResult, program: &Program) -> TypeCheckResult {
    let mut checker = TypeChecker::new(
        resolution.symbols,
        resolution.resolved,
        resolution.diagnostics,
        &program.pool,
    );
    check::check_signatures(&mut checker, program);
    check::check_bodies(&mut checker, program);
    checker.finish()
}

/// Runs ONLY the signature check phase, producing an initial TypeCheckResult.
#[must_use]
#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(resolution, program))]
pub fn check_signatures_only(resolution: ResolutionResult, program: &Program) -> TypeCheckResult {
    let mut checker = TypeChecker::new(
        resolution.symbols,
        resolution.resolved,
        resolution.diagnostics,
        &program.pool,
    );
    check::check_signatures(&mut checker, program);
    checker.finish()
}

/// Runs ONLY the bodies check phase, given a TypeCheckResult from check_signatures_only.
#[must_use]
#[tracing::instrument(level = "trace", target = "arandu_typeck", skip(signatures, program))]
pub fn check_bodies_only(signatures: TypeCheckResult, program: &Program) -> TypeCheckResult {
    let mut checker = TypeChecker::new(
        signatures.symbols,
        signatures.resolved,
        signatures.diagnostics,
        &program.pool,
    );
    checker.type_info = signatures.type_info;

    println!(
        "check_bodies_only diagnostics BEFORE check_bodies: {:?}",
        checker.diagnostics
    );
    check::check_bodies(&mut checker, program);
    println!(
        "check_bodies_only diagnostics AFTER check_bodies: {:?}",
        checker.diagnostics
    );
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
    pub fn resolve(&self, id: TypeId) -> ArType {
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
            ArType::Result(ok, err) => Some((ok, err)),
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
            ArType::Result(ok, _) => Some(ok),
            ArType::Option(inner) => Some(inner),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_result_type_id(&self, id: TypeId) -> bool {
        self.is_result_type(&self.resolve(id))
    }

    #[must_use]
    pub fn is_err_type(&self, ty: &ArType) -> bool {
        types::is_err_type(ty, &self.type_info.type_interner)
    }

    #[must_use]
    pub fn unify_return_type(&self, expected: &ArType, actual: &ArType) -> bool {
        types::unify_return_type(expected, actual, &self.type_info.type_interner)
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
        arandu_middle::types::lower::lower_type_expr_ctx(
            expr_id,
            &ctx,
            &mut self.type_info.type_interner,
        )
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
        arandu_middle::types::lower::lower_result_type_ctx(
            result,
            &ctx,
            &mut self.type_info.type_interner,
        )
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
        self.type_info.decl_type(symbol)
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
        types::unify(&a_ty, &b_ty, &self.type_info.type_interner)
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

#[cfg(test)]
mod tests {
    use super::errors::constraint_to_diagnostic;
    use super::types::InterfaceInfo;
    use super::types::Primitive;
    use super::*;
    use crate::Span;
    use crate::SymbolKind;
    use crate::type_checker::info::translate_type;
    use arandu_middle::DiagCode;

    // ── helpers ──

    fn new_interner() -> TypeInterner {
        TypeInterner::new()
    }

    fn empty_symbols() -> SymbolTable {
        SymbolTable::new(0)
    }

    fn dummy_span() -> Span {
        Span::new(0, 0, 0)
    }

    // ── TyCtx ──

    #[test]
    fn ty_ctx_bind_and_lookup() {
        let mut ctx = TyCtx::new();
        let sym = SymbolId::new(0, 0);
        let mut i = new_interner();
        let tid = i.intern(ArType::Primitive(Primitive::Int));
        ctx.bind(sym, tid);
        assert_eq!(ctx.lookup(sym), Some(tid));
    }

    #[test]
    fn ty_ctx_lookup_missing_returns_none() {
        let ctx = TyCtx::new();
        assert_eq!(ctx.lookup(SymbolId::new(0, 999)), None);
    }

    #[test]
    fn ty_ctx_return_stack() {
        let mut ctx = TyCtx::new();
        let mut i = new_interner();
        let int_id = i.intern(ArType::Primitive(Primitive::Int));
        let bool_id = i.intern(ArType::Primitive(Primitive::Bool));
        assert_eq!(ctx.current_return(), None);
        ctx.push_return(int_id, dummy_span());
        assert_eq!(ctx.current_return(), Some(int_id));
        ctx.push_return(bool_id, dummy_span());
        assert_eq!(ctx.current_return(), Some(bool_id));
        ctx.pop_return();
        assert_eq!(ctx.current_return(), Some(int_id));
        ctx.pop_return();
        assert_eq!(ctx.current_return(), None);
    }

    #[test]
    fn ty_ctx_loop_tracking() {
        let mut ctx = TyCtx::new();
        assert!(!ctx.is_in_loop());
        ctx.enter_loop();
        assert!(ctx.is_in_loop());
        ctx.enter_loop();
        assert!(ctx.is_in_loop());
        ctx.exit_loop();
        assert!(ctx.is_in_loop());
        ctx.exit_loop();
        assert!(!ctx.is_in_loop());
    }

    #[test]
    fn ty_ctx_exit_loop_does_not_underflow() {
        let mut ctx = TyCtx::new();
        ctx.exit_loop();
        assert!(!ctx.is_in_loop());
    }

    #[test]
    fn ty_ctx_bind_resizes_vec() {
        let mut ctx = TyCtx::new();
        let sym = SymbolId::new(0, 5);
        let mut i = new_interner();
        let tid = i.intern(ArType::Primitive(Primitive::Int));
        ctx.bind(sym, tid);
        assert_eq!(ctx.lookup(sym), Some(tid));
        assert_eq!(ctx.lookup(SymbolId::new(0, 4)), None);
    }

    // ── TypeInfo ──

    #[test]
    fn type_info_record_and_lookup_expr() {
        let mut i = new_interner();
        let tid = i.intern(ArType::Primitive(Primitive::Int));
        let mut info = TypeInfo::with_interner(i);
        let eid = ExprId::new(3);
        info.record_expr_type(eid, tid);
        assert_eq!(
            info.expr_type(eid),
            Some(&ArType::Primitive(Primitive::Int))
        );
        assert_eq!(info.expr_type_id(eid), Some(tid));
    }

    #[test]
    fn type_info_missing_expr_returns_none() {
        let info = TypeInfo::new();
        assert_eq!(info.expr_type(ExprId::new(0)), None);
    }

    #[test]
    fn type_info_record_and_lookup_decl() {
        let mut i = new_interner();
        let tid = i.intern(ArType::Primitive(Primitive::Bool));
        let mut info = TypeInfo::with_interner(i);
        let sym = SymbolId::new(0, 1);
        info.record_decl_type(sym, tid);
        assert_eq!(
            info.decl_type(sym),
            Some(&ArType::Primitive(Primitive::Bool))
        );
        assert_eq!(info.decl_type_id(sym), Some(tid));
    }

    #[test]
    fn type_info_missing_decl_returns_none() {
        let info = TypeInfo::new();
        assert_eq!(info.decl_type(SymbolId::new(0, 0)), None);
    }

    #[test]
    fn type_info_enum_variant_tag() {
        let mut info = TypeInfo::new();
        info.record_enum_variant_tag(SymbolId::new(0, 0), 0);
        info.record_enum_variant_tag(SymbolId::new(0, 1), 1);
        assert_eq!(info.enum_variant_tags.get(&SymbolId::new(0, 0)), Some(&0));
        assert_eq!(info.enum_variant_tags.get(&SymbolId::new(0, 1)), Some(&1));
    }

    // ── translate_type ──

    #[test]
    fn translate_primitive() {
        let from = new_interner();
        let mut to = new_interner();
        let result = translate_type(&ArType::Primitive(Primitive::Int), &from, &mut to);
        assert_eq!(result, ArType::Primitive(Primitive::Int));
    }

    #[test]
    fn translate_named_with_args() {
        let mut from = new_interner();
        let int_id = from.intern(ArType::Primitive(Primitive::Int));
        let named = ArType::Named(SymbolId::new(0, 0), vec![int_id]);
        let mut to = new_interner();
        let result = translate_type(&named, &from, &mut to);
        let expected_int = to.intern(ArType::Primitive(Primitive::Int));
        assert_eq!(
            result,
            ArType::Named(SymbolId::new(0, 0), vec![expected_int])
        );
    }

    #[test]
    fn translate_func() {
        let mut from = new_interner();
        let int_id = from.intern(ArType::Primitive(Primitive::Int));
        let void_id = from.intern(ArType::Void);
        let func = ArType::Func(vec![int_id], void_id);
        let mut to = new_interner();
        let result = translate_type(&func, &from, &mut to);
        let expected_int = to.intern(ArType::Primitive(Primitive::Int));
        let expected_void = to.intern(ArType::Void);
        assert_eq!(result, ArType::Func(vec![expected_int], expected_void));
    }

    #[test]
    fn translate_nullable() {
        let mut from = new_interner();
        let inner = from.intern(ArType::Primitive(Primitive::Int));
        let nullable = ArType::Nullable(inner);
        let mut to = new_interner();
        let result = translate_type(&nullable, &from, &mut to);
        assert_eq!(
            result,
            ArType::Nullable(to.intern(ArType::Primitive(Primitive::Int)))
        );
    }

    #[test]
    fn translate_slice_ptr_array_tuple_result_option_coroutine_range() {
        let mut from = new_interner();
        let int_id = from.intern(ArType::Primitive(Primitive::Int));
        let bool_id = from.intern(ArType::Primitive(Primitive::Bool));
        let variants: &[ArType] = &[
            ArType::Slice(int_id),
            ArType::Ptr(int_id),
            ArType::Array(5, int_id),
            ArType::Tuple(vec![int_id, bool_id]),
            ArType::Result(int_id, bool_id),
            ArType::Option(int_id),
            ArType::Coroutine(int_id),
            ArType::Range(int_id),
        ];
        let mut to = new_interner();
        for ty in variants {
            let result = translate_type(ty, &from, &mut to);
            assert!(!matches!(result, ArType::Error), "failed for {ty:?}");
        }
    }

    #[test]
    fn translate_err_void_error_int_literal_float_literal() {
        let from = new_interner();
        let mut to = new_interner();
        for ty in &[
            ArType::Err,
            ArType::Void,
            ArType::Error,
            ArType::IntLiteral,
            ArType::FloatLiteral,
        ] {
            assert_eq!(translate_type(ty, &from, &mut to), *ty);
        }
    }

    // ── merge_from ──

    #[test]
    fn merge_from_decl_types() {
        let mut i = TypeInterner::new();
        let int_id = i.intern(ArType::Primitive(Primitive::Int));
        let mut from_info = TypeInfo::with_interner(i);
        let mut to_info = TypeInfo::new();
        from_info.decl_types.insert(SymbolId::new(0, 1), int_id);
        to_info.merge_from(&from_info);
        assert_eq!(
            to_info.decl_type(SymbolId::new(0, 1)),
            Some(&ArType::Primitive(Primitive::Int))
        );
    }

    #[test]
    fn merge_from_struct_fields() {
        let mut from_info = TypeInfo::new();
        from_info.struct_fields.insert(
            SymbolId::new(0, 0),
            [("x".to_string(), ArType::Primitive(Primitive::Int))]
                .into_iter()
                .collect(),
        );
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        let fields = to_info.struct_fields.get(&SymbolId::new(0, 0));
        assert!(fields.is_some());
        assert_eq!(
            fields.unwrap().get("x"),
            Some(&ArType::Primitive(Primitive::Int))
        );
    }

    #[test]
    fn merge_from_enum_variants() {
        let mut from_info = TypeInfo::new();
        from_info.enum_variants.insert(
            SymbolId::new(0, 1),
            (SymbolId::new(0, 0), EnumPayloadShape::Unit),
        );
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        assert_eq!(
            to_info.enum_variants.get(&SymbolId::new(0, 1)),
            Some(&(SymbolId::new(0, 0), EnumPayloadShape::Unit))
        );
    }

    #[test]
    fn merge_from_enum_payload_shape_tuple() {
        let mut from_info = TypeInfo::new();
        from_info.enum_variants.insert(
            SymbolId::new(0, 2),
            (
                SymbolId::new(0, 0),
                EnumPayloadShape::Tuple(vec![ArType::Primitive(Primitive::Int)]),
            ),
        );
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        let variant = to_info.enum_variants.get(&SymbolId::new(0, 2));
        assert!(matches!(variant, Some((_, EnumPayloadShape::Tuple(_)))));
    }

    #[test]
    fn merge_from_enum_variant_tags() {
        let mut from_info = TypeInfo::new();
        from_info.enum_variant_tags.insert(SymbolId::new(0, 0), 0);
        from_info.enum_variant_tags.insert(SymbolId::new(0, 1), 1);
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        assert_eq!(
            to_info.enum_variant_tags.get(&SymbolId::new(0, 0)),
            Some(&0)
        );
        assert_eq!(
            to_info.enum_variant_tags.get(&SymbolId::new(0, 1)),
            Some(&1)
        );
    }

    #[test]
    fn merge_from_generic_params() {
        let mut from_info = TypeInfo::new();
        from_info.generic_params.insert(
            SymbolId::new(0, 0),
            vec![SymbolId::new(0, 1), SymbolId::new(0, 2)],
        );
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        assert_eq!(
            to_info.generic_params.get(&SymbolId::new(0, 0)),
            Some(&vec![SymbolId::new(0, 1), SymbolId::new(0, 2)])
        );
    }

    #[test]
    fn merge_from_param_constraints() {
        let mut from_info = TypeInfo::new();
        from_info
            .param_constraints
            .insert(SymbolId::new(0, 1), vec![SymbolId::new(0, 2)]);
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        assert_eq!(
            to_info.param_constraints.get(&SymbolId::new(0, 1)),
            Some(&vec![SymbolId::new(0, 2)])
        );
    }

    #[test]
    fn merge_from_interfaces() {
        let mut from_info = TypeInfo::new();
        from_info.interfaces.insert(
            SymbolId::new(0, 0),
            InterfaceInfo {
                methods: Vec::new(),
            },
        );
        let mut to_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        assert!(to_info.interfaces.contains_key(&SymbolId::new(0, 0)));
    }

    #[test]
    fn merge_from_empty_does_nothing() {
        let mut to_info = TypeInfo::new();
        let from_info = TypeInfo::new();
        to_info.merge_from(&from_info);
        assert!(to_info.decl_types.is_empty());
        assert!(to_info.struct_fields.is_empty());
    }

    // ── constraint_to_diagnostic ──

    #[test]
    fn constraint_assignment() {
        let i = new_interner();
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::Assignment {
                lhs_span: Span::new(0, 0, 3),
                rhs_span: Span::new(0, 6, 9),
            },
        };
        let symbols = empty_symbols();
        let type_info = TypeInfo::with_interner(i);
        let diag = constraint_to_diagnostic(&constraint, &symbols, &type_info);
        assert_eq!(diag.code, DiagCode::T002IncompatibleAssignment);
        assert!(diag.message.contains("int"));
        assert!(diag.message.contains("str"));
        assert_eq!(diag.labels.len(), 2);
    }

    #[test]
    fn constraint_call_arg() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Bool),
            origin: ConstraintOrigin::CallArg {
                call_span: dummy_span(),
                param_span: dummy_span(),
                arg_span: dummy_span(),
                arg_index: 0,
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T003IncompatibleCallArg);
        assert!(diag.message.contains("argument 1"));
        assert_eq!(diag.labels.len(), 2);
    }

    #[test]
    fn constraint_return_type() {
        let constraint = Constraint {
            expected: ArType::Void,
            found: ArType::Primitive(Primitive::Int),
            origin: ConstraintOrigin::ReturnType {
                return_span: dummy_span(),
                declared_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T004IncompatibleReturnType);
        assert_eq!(diag.hints.len(), 1);
    }

    #[test]
    fn constraint_if_branches() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::IfBranches {
                then_span: dummy_span(),
                else_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T007IfBranchMismatch);
    }

    #[test]
    fn constraint_match_arms() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Bool),
            origin: ConstraintOrigin::MatchArms {
                first_span: dummy_span(),
                mismatch_span: dummy_span(),
                arm_index: 1,
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T008MatchArmMismatch);
        assert!(diag.message.contains("arm 2"));
    }

    #[test]
    fn constraint_binary_op() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::BinaryOp {
                op_span: dummy_span(),
                left_span: dummy_span(),
                right_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T005OperatorNotApplicable);
        // str + int suggests interpolation
        assert!(
            diag.hints
                .iter()
                .any(|h| h.message.contains("interpolation"))
        );
    }

    #[test]
    fn constraint_unary_op() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Bool),
            found: ArType::Primitive(Primitive::Int),
            origin: ConstraintOrigin::UnaryOp {
                op_span: dummy_span(),
                operand_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T005OperatorNotApplicable);
    }

    #[test]
    fn constraint_condition() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Bool),
            found: ArType::Primitive(Primitive::Int),
            origin: ConstraintOrigin::Condition { span: dummy_span() },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T009ConditionNotBool);
        assert!(diag.hints.iter().any(|h| h.message.contains("!=")));
    }

    #[test]
    fn constraint_field_init() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::FieldInit {
                struct_span: dummy_span(),
                field_name: "name".to_string(),
                field_span: dummy_span(),
                value_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T002IncompatibleAssignment);
        assert!(diag.message.contains("name"));
    }

    #[test]
    fn constraint_set_target() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Bool),
            origin: ConstraintOrigin::SetTarget {
                place_span: dummy_span(),
                value_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T002IncompatibleAssignment);
    }

    #[test]
    fn constraint_cast_expr() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::CastExpr {
                expr_span: dummy_span(),
                target_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T010InvalidCast);
    }

    #[test]
    fn constraint_implicit_widening() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Float),
            found: ArType::Primitive(Primitive::Int),
            origin: ConstraintOrigin::ImplicitWidening {
                source_span: dummy_span(),
                target_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T015ImplicitWidening);
        assert!(diag.hints.iter().any(|h| h.message.contains("explicit")));
    }

    #[test]
    fn constraint_try_invalid() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::TryInvalid { span: dummy_span() },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T016TryInvalid);
    }

    #[test]
    fn constraint_await_invalid() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::AwaitInvalid { span: dummy_span() },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T032AwaitInvalid);
    }

    #[test]
    fn constraint_invalid_index_base_error() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::InvalidIndex {
                base_span: dummy_span(),
                index_span: dummy_span(),
                is_base_error: true,
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T017InvalidIndex);
        assert!(diag.message.contains("cannot be indexed"));
    }

    #[test]
    fn constraint_invalid_index_type() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::InvalidIndex {
                base_span: dummy_span(),
                index_span: dummy_span(),
                is_base_error: false,
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T017InvalidIndex);
        assert!(diag.message.contains("index must be"));
    }

    #[test]
    fn constraint_undefined_field() {
        let mut symbols = SymbolTable::new(0);
        let sym = symbols
            .define(ScopeId(0), "MyStruct", SymbolKind::Struct, dummy_span())
            .unwrap();
        let constraint = Constraint {
            expected: ArType::Named(sym, vec![]),
            found: ArType::Void,
            origin: ConstraintOrigin::UndefinedField {
                base_span: dummy_span(),
                field_span: dummy_span(),
                field_name: "x".to_string(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &symbols, &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T018UndefinedField);
        assert!(diag.message.contains("x"));
    }

    #[test]
    fn constraint_array_literal() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::ArrayLiteral {
                array_span: dummy_span(),
                item_span: dummy_span(),
                item_index: 1,
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T002IncompatibleAssignment);
        assert!(diag.message.contains("element 2"));
    }

    #[test]
    fn constraint_null_coalesce() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Bool),
            origin: ConstraintOrigin::NullCoalesce {
                left_span: dummy_span(),
                right_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T002IncompatibleAssignment);
        assert!(diag.message.contains("??"));
    }

    #[test]
    fn constraint_catch_handler() {
        let constraint = Constraint {
            expected: ArType::Primitive(Primitive::Int),
            found: ArType::Primitive(Primitive::Str),
            origin: ConstraintOrigin::CatchHandler {
                expr_span: dummy_span(),
                handler_span: dummy_span(),
            },
        };
        let diag = constraint_to_diagnostic(&constraint, &empty_symbols(), &TypeInfo::new());
        assert_eq!(diag.code, DiagCode::T002IncompatibleAssignment);
        assert!(diag.message.contains("handler"));
    }

    // ── ConstraintOrigin ──

    #[test]
    fn constraint_origin_debug() {
        let origins: &[ConstraintOrigin] = &[
            ConstraintOrigin::Assignment {
                lhs_span: dummy_span(),
                rhs_span: dummy_span(),
            },
            ConstraintOrigin::CallArg {
                call_span: dummy_span(),
                param_span: dummy_span(),
                arg_span: dummy_span(),
                arg_index: 0,
            },
            ConstraintOrigin::ReturnType {
                return_span: dummy_span(),
                declared_span: dummy_span(),
            },
            ConstraintOrigin::IfBranches {
                then_span: dummy_span(),
                else_span: dummy_span(),
            },
            ConstraintOrigin::MatchArms {
                first_span: dummy_span(),
                mismatch_span: dummy_span(),
                arm_index: 0,
            },
            ConstraintOrigin::BinaryOp {
                op_span: dummy_span(),
                left_span: dummy_span(),
                right_span: dummy_span(),
            },
            ConstraintOrigin::UnaryOp {
                op_span: dummy_span(),
                operand_span: dummy_span(),
            },
            ConstraintOrigin::Condition { span: dummy_span() },
            ConstraintOrigin::FieldInit {
                struct_span: dummy_span(),
                field_name: "f".into(),
                field_span: dummy_span(),
                value_span: dummy_span(),
            },
            ConstraintOrigin::SetTarget {
                place_span: dummy_span(),
                value_span: dummy_span(),
            },
            ConstraintOrigin::CastExpr {
                expr_span: dummy_span(),
                target_span: dummy_span(),
            },
            ConstraintOrigin::ImplicitWidening {
                source_span: dummy_span(),
                target_span: dummy_span(),
            },
            ConstraintOrigin::TryInvalid { span: dummy_span() },
            ConstraintOrigin::AwaitInvalid { span: dummy_span() },
            ConstraintOrigin::InvalidIndex {
                base_span: dummy_span(),
                index_span: dummy_span(),
                is_base_error: true,
            },
            ConstraintOrigin::UndefinedField {
                base_span: dummy_span(),
                field_span: dummy_span(),
                field_name: "f".into(),
            },
            ConstraintOrigin::ArrayLiteral {
                array_span: dummy_span(),
                item_span: dummy_span(),
                item_index: 0,
            },
            ConstraintOrigin::NullCoalesce {
                left_span: dummy_span(),
                right_span: dummy_span(),
            },
            ConstraintOrigin::CatchHandler {
                expr_span: dummy_span(),
                handler_span: dummy_span(),
            },
        ];
        for origin in origins {
            let _ = format!("{origin:?}");
        }
    }
}
