use arandu_lexer::Span;

use super::types::ArType;

// ── Constraint ──────────────────────────────────────────────────────

/// A type constraint recording that `expected` and `found` must be
/// compatible. When unification fails, the `origin` is used to produce
/// a flow-based error message showing *where* the expected type came
/// from and *where* the incompatible type was produced.
#[derive(Debug, Clone)]
pub struct Constraint {
    pub expected: ArType,
    pub found: ArType,
    pub origin: ConstraintOrigin,
}

// ── ConstraintOrigin — flow-based error source ──────────────────────

/// The origin of a constraint, carrying source spans so error messages
/// can show the data-flow path instead of exposing compiler internals.
#[derive(Debug, Clone)]
pub enum ConstraintOrigin {
    /// `x Type = value` — type annotation vs inferred value type.
    Assignment { lhs_span: Span, rhs_span: Span },

    /// `f(arg)` — argument type vs declared parameter type.
    CallArg {
        call_span: Span,
        param_span: Span,
        arg_span: Span,
        arg_index: usize,
    },

    /// `return value` — return value type vs declared return type.
    ReturnType {
        return_span: Span,
        declared_span: Span,
    },

    /// `if cond { A } else { B }` — then-branch vs else-branch type.
    IfBranches { then_span: Span, else_span: Span },

    /// Multiple match arms must have the same type.
    MatchArms {
        first_span: Span,
        mismatch_span: Span,
        arm_index: usize,
    },

    /// `a + b` — operator applied to incompatible operand types.
    BinaryOp {
        op_span: Span,
        left_span: Span,
        right_span: Span,
    },

    /// `-a` or `!a` — unary operator on wrong type.
    UnaryOp { op_span: Span, operand_span: Span },

    /// A condition expression that should be `bool` but isn't.
    Condition { span: Span },

    /// `Struct { field: value }` — field type mismatch.
    FieldInit {
        struct_span: Span,
        field_name: String,
        field_span: Span,
        value_span: Span,
    },

    /// `set place = value` — assigned value type vs place type.
    SetTarget { place_span: Span, value_span: Span },

    /// `expr as Type` — invalid cast.
    CastExpr { expr_span: Span, target_span: Span },

    /// Implicit widening from variable (not literal) — T015.
    ImplicitWidening {
        source_span: Span,
        target_span: Span,
    },

    /// `expr?` applied to a type that is neither `Result` nor `Option`.
    TryInvalid { span: Span },

    /// `base[index]` applied to non-array/slice or non-int index.
    InvalidIndex {
        base_span: Span,
        index_span: Span,
        is_base_error: bool,
    },

    /// `base.field` where field does not exist.
    UndefinedField {
        base_span: Span,
        field_span: Span,
        field_name: String,
    },

    /// Array literal elements must share the same type.
    ArrayLiteral {
        array_span: Span,
        item_span: Span,
        item_index: usize,
    },

    /// `left ?? right` — nullable left must unify with right-hand type.
    NullCoalesce { left_span: Span, right_span: Span },

    /// `expr catch handler` — handler type must match `Result` ok type.
    CatchHandler { expr_span: Span, handler_span: Span },
}
