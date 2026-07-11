use super::ar_type::ArType;
use super::primitive::Primitive;
use super::result_option::{is_err_type, result_ok_err};
use super::type_interner::TypeInterner;

/// Unify return value type against declared `Result` return.
#[must_use]
#[tracing::instrument(
    level = "trace",
    target = "arandu_middle::types::unify",
    skip(interner)
)]
pub fn unify_return_type(expected: &ArType, actual: &ArType, interner: &TypeInterner) -> bool {
    if unify(expected, actual, interner) {
        return true;
    }
    if let Some((ok_exp, err_exp)) = result_ok_err(expected, interner) {
        if let Some((ok_act, err_act)) = result_ok_err(actual, interner) {
            return unify(&ok_exp, &ok_act, interner) && unify(&err_exp, &err_act, interner);
        }
        // `return nil` on `Result<void, Err>`
        if matches!(ok_exp, ArType::Void)
            && matches!(actual, ArType::Nullable(inner) if matches!(&interner.resolve(*inner), ArType::Error))
        {
            return true;
        }
        if is_err_type(actual, interner) && is_err_type(&err_exp, interner) {
            return true;
        }
        if matches!(actual, ArType::Err) && matches!(err_exp, ArType::Err) {
            return true;
        }
    }
    false
}

// ── Unification ─────────────────────────────────────────────────────

/// Structural type equality check using the interner for deep resolution.
///
/// - `Error` unifies with anything (poison propagation)
/// - `Any` unifies with anything (FFI/variadic)
/// - `IntLiteral` unifies with any numeric type
/// - `FloatLiteral` unifies with any float type
/// - Named types compare `SymbolId` + generic args
/// - Func types compare param count, params, and return
#[must_use]
#[tracing::instrument(
    level = "trace",
    target = "arandu_middle::types::unify",
    skip(interner)
)]
pub fn unify(a: &ArType, b: &ArType, interner: &TypeInterner) -> bool {
    // Poison and Any always unify
    if a.is_error() || b.is_error() {
        return true;
    }
    if matches!(a, ArType::Primitive(Primitive::Any))
        || matches!(b, ArType::Primitive(Primitive::Any))
    {
        return true;
    }

    // Literal absorption
    if a.is_literal() && a.literal_absorbs(b) {
        return true;
    }
    if b.is_literal() && b.literal_absorbs(a) {
        return true;
    }
    // Two int literals or two float literals unify
    if matches!((a, b), (ArType::IntLiteral, ArType::IntLiteral)) {
        return true;
    }
    if matches!((a, b), (ArType::FloatLiteral, ArType::FloatLiteral)) {
        return true;
    }
    // IntLiteral and FloatLiteral: the int absorbs float context
    if matches!(
        (a, b),
        (ArType::IntLiteral, ArType::FloatLiteral) | (ArType::FloatLiteral, ArType::IntLiteral)
    ) {
        return true;
    }

    match (a, b) {
        (ArType::Primitive(pa), ArType::Primitive(pb)) => pa == pb,
        (ArType::Named(id_a, args_a), ArType::Named(id_b, args_b)) => {
            id_a == id_b
                && args_a.len() == args_b.len()
                && args_a.iter().zip(args_b).all(|(&x, &y)| {
                    if x == y {
                        return true;
                    }
                    unify(&interner.resolve(x), &interner.resolve(y), interner)
                })
        }
        (ArType::Func(params_a, ret_a), ArType::Func(params_b, ret_b)) => {
            params_a.len() == params_b.len()
                && params_a.iter().zip(params_b).all(|(&x, &y)| {
                    if x == y {
                        return true;
                    }
                    unify(&interner.resolve(x), &interner.resolve(y), interner)
                })
                && (*ret_a == *ret_b
                    || unify(
                        &interner.resolve(*ret_a),
                        &interner.resolve(*ret_b),
                        interner,
                    ))
        }
        (ArType::Nullable(inner_a), ArType::Nullable(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::Nullable(inner), other) | (other, ArType::Nullable(inner)) => {
            unify(&interner.resolve(*inner), other, interner)
        }
        (ArType::Slice(inner_a), ArType::Slice(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::Array(n_a, elem_a), ArType::Array(n_b, elem_b)) => {
            n_a == n_b
                && (*elem_a == *elem_b
                    || unify(
                        &interner.resolve(*elem_a),
                        &interner.resolve(*elem_b),
                        interner,
                    ))
        }
        (ArType::Ptr(inner_a), ArType::Ptr(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        // Shared refs unify structurally.
        (ArType::Ref(inner_a), ArType::Ref(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        // Exclusive refs unify with each other.
        (ArType::RefMut(inner_a), ArType::RefMut(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        // Coercion: `&mut T` can decay to `&T` (exclusive → shared). Never reverse.
        (ArType::Ref(inner_a), ArType::RefMut(inner_b))
        | (ArType::RefMut(inner_b), ArType::Ref(inner_a)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::GenRef, ArType::GenRef) => true,
        (ArType::Tuple(types_a), ArType::Tuple(types_b)) => {
            types_a.len() == types_b.len()
                && types_a.iter().zip(types_b).all(|(&x, &y)| {
                    if x == y {
                        return true;
                    }
                    unify(&interner.resolve(x), &interner.resolve(y), interner)
                })
        }
        (ArType::Result(ok_a, err_a), ArType::Result(ok_b, err_b)) => {
            (*ok_a == *ok_b || unify(&interner.resolve(*ok_a), &interner.resolve(*ok_b), interner))
                && (*err_a == *err_b
                    || unify(
                        &interner.resolve(*err_a),
                        &interner.resolve(*err_b),
                        interner,
                    ))
        }
        (ArType::Option(inner_a), ArType::Option(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::Coroutine(inner_a), ArType::Coroutine(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::Poll(inner_a), ArType::Poll(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::Range(inner_a), ArType::Range(inner_b)) => {
            *inner_a == *inner_b
                || unify(
                    &interner.resolve(*inner_a),
                    &interner.resolve(*inner_b),
                    interner,
                )
        }
        (ArType::Err, ArType::Err) => true,
        (ArType::Void, ArType::Void) => true,
        _ => false,
    }
}

/// Given two types where at least one may be a literal, resolve to the
/// concrete type. This is used to determine the result type of binary
/// operations where literals are involved.
#[must_use]
pub fn resolve_literal_pair(a: &ArType, b: &ArType) -> ArType {
    match (a, b) {
        (ArType::IntLiteral, other) | (other, ArType::IntLiteral) if !other.is_literal() => {
            other.clone()
        }
        (ArType::FloatLiteral, other) | (other, ArType::FloatLiteral) if !other.is_literal() => {
            other.clone()
        }
        (ArType::IntLiteral, ArType::IntLiteral) => ArType::Primitive(Primitive::Int),
        (ArType::FloatLiteral, ArType::FloatLiteral) => ArType::Primitive(Primitive::Float),
        (ArType::IntLiteral, ArType::FloatLiteral) | (ArType::FloatLiteral, ArType::IntLiteral) => {
            ArType::Primitive(Primitive::Float)
        }
        _ => a.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymbolId;
    use crate::types::Primitive;

    fn new_interner() -> TypeInterner {
        TypeInterner::new()
    }

    fn int_t(interner: &mut TypeInterner) -> super::super::type_interner::TypeId {
        interner.intern(ArType::Primitive(Primitive::Int))
    }
    fn bool_t(interner: &mut TypeInterner) -> super::super::type_interner::TypeId {
        interner.intern(ArType::Primitive(Primitive::Bool))
    }
    fn str_t(interner: &mut TypeInterner) -> super::super::type_interner::TypeId {
        interner.intern(ArType::Primitive(Primitive::Str))
    }

    // ── Primitive unification ──

    #[test]
    fn unify_same_primitives() {
        let i = new_interner();
        assert!(unify(
            &ArType::Primitive(Primitive::Int),
            &ArType::Primitive(Primitive::Int),
            &i
        ));
        assert!(unify(
            &ArType::Primitive(Primitive::Bool),
            &ArType::Primitive(Primitive::Bool),
            &i
        ));
        assert!(unify(
            &ArType::Primitive(Primitive::Str),
            &ArType::Primitive(Primitive::Str),
            &i
        ));
    }

    #[test]
    fn unify_different_primitives() {
        let i = new_interner();
        assert!(!unify(
            &ArType::Primitive(Primitive::Int),
            &ArType::Primitive(Primitive::Bool),
            &i
        ));
        assert!(!unify(
            &ArType::Primitive(Primitive::Str),
            &ArType::Primitive(Primitive::Int),
            &i
        ));
        assert!(!unify(
            &ArType::Primitive(Primitive::F32),
            &ArType::Primitive(Primitive::F64),
            &i
        ));
    }

    // ── Poison (Error) — unifies with everything ──

    #[test]
    fn error_unifies_with_everything() {
        let mut i = new_interner();
        let cases = [
            ArType::Primitive(Primitive::Int),
            ArType::Primitive(Primitive::Bool),
            ArType::Void,
            ArType::Err,
            ArType::Named(SymbolId::new(0, 42), vec![]),
            ArType::Nullable(int_t(&mut i)),
            ArType::Slice(int_t(&mut i)),
            ArType::Array(3, int_t(&mut i)),
            ArType::Ptr(int_t(&mut i)),
            ArType::Tuple(vec![int_t(&mut i), bool_t(&mut i)]),
            ArType::Result(int_t(&mut i), int_t(&mut i)),
            ArType::Option(int_t(&mut i)),
            ArType::Coroutine(int_t(&mut i)),
            ArType::Range(int_t(&mut i)),
            ArType::IntLiteral,
            ArType::FloatLiteral,
            ArType::Func(vec![], int_t(&mut i)),
        ];
        for case in &cases {
            assert!(
                unify(&ArType::Error, case, &i),
                "Error should unify with {case:?}"
            );
            assert!(
                unify(case, &ArType::Error, &i),
                "{case:?} should unify with Error"
            );
        }
    }

    // ── Any unifies with everything ──

    #[test]
    fn any_unifies_with_everything() {
        let mut i = new_interner();
        let cases = [
            ArType::Primitive(Primitive::Int),
            ArType::Primitive(Primitive::Bool),
            ArType::Void,
            ArType::Err,
            ArType::Named(SymbolId::new(0, 1), vec![]),
            ArType::Nullable(int_t(&mut i)),
            ArType::Slice(int_t(&mut i)),
            ArType::Result(int_t(&mut i), int_t(&mut i)),
            ArType::IntLiteral,
            ArType::Error,
        ];
        for case in &cases {
            assert!(
                unify(&ArType::Primitive(Primitive::Any), case, &i),
                "Any should unify with {case:?}"
            );
            assert!(
                unify(case, &ArType::Primitive(Primitive::Any), &i),
                "{case:?} should unify with Any"
            );
        }
    }

    // ── Literal absorption ──

    #[test]
    fn int_literal_absorbs_numeric_primitives() {
        let i = new_interner();
        let numerics = [
            Primitive::Int,
            Primitive::Uint,
            Primitive::I8,
            Primitive::I16,
            Primitive::I32,
            Primitive::I64,
            Primitive::U8,
            Primitive::U16,
            Primitive::U32,
            Primitive::U64,
            Primitive::Byte,
        ];
        for numeric in &numerics {
            assert!(unify(&ArType::IntLiteral, &ArType::Primitive(*numeric), &i));
            assert!(unify(&ArType::Primitive(*numeric), &ArType::IntLiteral, &i));
        }
    }

    #[test]
    fn int_literal_does_not_absorb_non_numeric() {
        let i = new_interner();
        assert!(!unify(
            &ArType::IntLiteral,
            &ArType::Primitive(Primitive::Bool),
            &i
        ));
        assert!(!unify(
            &ArType::IntLiteral,
            &ArType::Primitive(Primitive::Str),
            &i
        ));
        assert!(!unify(
            &ArType::IntLiteral,
            &ArType::Primitive(Primitive::Char),
            &i
        ));
    }

    #[test]
    fn float_literal_absorbs_float_primitives() {
        let i = new_interner();
        let floats = [Primitive::Float, Primitive::F32, Primitive::F64];
        for flt in &floats {
            assert!(unify(&ArType::FloatLiteral, &ArType::Primitive(*flt), &i));
            assert!(unify(&ArType::Primitive(*flt), &ArType::FloatLiteral, &i));
        }
    }

    #[test]
    fn float_literal_does_not_absorb_int() {
        let i = new_interner();
        assert!(!unify(
            &ArType::FloatLiteral,
            &ArType::Primitive(Primitive::Int),
            &i
        ));
        assert!(!unify(
            &ArType::FloatLiteral,
            &ArType::Primitive(Primitive::Bool),
            &i
        ));
    }

    #[test]
    fn literal_literal_unification() {
        let i = new_interner();
        assert!(unify(&ArType::IntLiteral, &ArType::IntLiteral, &i));
        assert!(unify(&ArType::FloatLiteral, &ArType::FloatLiteral, &i));
        assert!(unify(&ArType::IntLiteral, &ArType::FloatLiteral, &i));
        assert!(unify(&ArType::FloatLiteral, &ArType::IntLiteral, &i));
    }

    // ── Named type unification ──

    #[test]
    fn named_same_id_no_args() {
        let i = new_interner();
        assert!(unify(
            &ArType::Named(SymbolId::new(0, 1), vec![]),
            &ArType::Named(SymbolId::new(0, 1), vec![]),
            &i
        ));
    }

    #[test]
    fn named_different_id_no_args() {
        let i = new_interner();
        assert!(!unify(
            &ArType::Named(SymbolId::new(0, 1), vec![]),
            &ArType::Named(SymbolId::new(0, 2), vec![]),
            &i
        ));
    }

    #[test]
    fn named_with_args_same() {
        let mut i = new_interner();
        let int = int_t(&mut i);
        let args = vec![int];
        assert!(unify(
            &ArType::Named(SymbolId::new(0, 1), args.clone()),
            &ArType::Named(SymbolId::new(0, 1), args),
            &i
        ));
    }

    #[test]
    fn named_with_args_different_length() {
        let mut i = new_interner();
        let a = ArType::Named(SymbolId::new(0, 1), vec![int_t(&mut i)]);
        let b = ArType::Named(SymbolId::new(0, 1), vec![]);
        assert!(!unify(&a, &b, &i));
    }

    #[test]
    fn named_with_args_different_inner_type() {
        let mut i = new_interner();
        let a = ArType::Named(SymbolId::new(0, 1), vec![int_t(&mut i)]);
        let b = ArType::Named(SymbolId::new(0, 1), vec![bool_t(&mut i)]);
        assert!(!unify(&a, &b, &i));
    }

    // ── Func type unification ──

    #[test]
    fn func_same_params_and_return() {
        let mut i = new_interner();
        let params = vec![int_t(&mut i), bool_t(&mut i)];
        let ret = int_t(&mut i);
        let f = ArType::Func(params.clone(), ret);
        assert!(unify(&f, &f, &i));
    }

    #[test]
    fn func_different_param_count() {
        let mut i = new_interner();
        let ret = int_t(&mut i);
        let a = ArType::Func(vec![int_t(&mut i)], ret);
        let b = ArType::Func(vec![int_t(&mut i), bool_t(&mut i)], ret);
        assert!(!unify(&a, &b, &i));
    }

    #[test]
    fn func_different_return() {
        let mut i = new_interner();
        let params = vec![int_t(&mut i)];
        let a = ArType::Func(params.clone(), int_t(&mut i));
        let b = ArType::Func(params, bool_t(&mut i));
        assert!(!unify(&a, &b, &i));
    }

    // ── Nullable unification ──

    #[test]
    fn nullable_same_inner() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        assert!(unify(
            &ArType::Nullable(inner),
            &ArType::Nullable(inner),
            &i
        ));
    }

    #[test]
    fn nullable_different_inner() {
        let mut i = new_interner();
        assert!(!unify(
            &ArType::Nullable(int_t(&mut i)),
            &ArType::Nullable(bool_t(&mut i)),
            &i
        ));
    }

    #[test]
    fn nullable_unifies_with_inner_type() {
        let mut i = new_interner();
        let int_id = int_t(&mut i);
        assert!(unify(
            &ArType::Nullable(int_id),
            &ArType::Primitive(Primitive::Int),
            &i
        ));
        assert!(unify(
            &ArType::Primitive(Primitive::Int),
            &ArType::Nullable(int_id),
            &i
        ));
    }

    // ── Slice unification ──

    #[test]
    fn slice_same_element() {
        let mut i = new_interner();
        let elem = int_t(&mut i);
        assert!(unify(&ArType::Slice(elem), &ArType::Slice(elem), &i));
    }

    #[test]
    fn slice_different_element() {
        let mut i = new_interner();
        assert!(!unify(
            &ArType::Slice(int_t(&mut i)),
            &ArType::Slice(bool_t(&mut i)),
            &i
        ));
    }

    // ── Array unification ──

    #[test]
    fn array_same_size_and_element() {
        let mut i = new_interner();
        let elem = int_t(&mut i);
        assert!(unify(&ArType::Array(3, elem), &ArType::Array(3, elem), &i));
    }

    #[test]
    fn array_different_size() {
        let mut i = new_interner();
        let elem = int_t(&mut i);
        assert!(!unify(&ArType::Array(3, elem), &ArType::Array(4, elem), &i));
    }

    #[test]
    fn array_different_element() {
        let mut i = new_interner();
        assert!(!unify(
            &ArType::Array(3, int_t(&mut i)),
            &ArType::Array(3, bool_t(&mut i)),
            &i
        ));
    }

    // ── Ptr unification ──

    #[test]
    fn ptr_same_inner() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        assert!(unify(&ArType::Ptr(inner), &ArType::Ptr(inner), &i));
    }

    #[test]
    fn ptr_different_inner() {
        let mut i = new_interner();
        assert!(!unify(
            &ArType::Ptr(int_t(&mut i)),
            &ArType::Ptr(bool_t(&mut i)),
            &i
        ));
    }

    // ── F2.0 Ref / RefMut unification ──

    #[test]
    fn ref_same_inner() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        assert!(unify(&ArType::Ref(inner), &ArType::Ref(inner), &i));
    }

    #[test]
    fn refmut_decays_to_ref() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        // Exclusive may decay to shared (Rust rule).
        assert!(unify(&ArType::Ref(inner), &ArType::RefMut(inner), &i));
        assert!(unify(&ArType::RefMut(inner), &ArType::Ref(inner), &i));
    }

    #[test]
    fn ref_does_not_unify_with_different_inner() {
        let mut i = new_interner();
        assert!(!unify(
            &ArType::Ref(int_t(&mut i)),
            &ArType::Ref(bool_t(&mut i)),
            &i
        ));
    }

    // ── Tuple unification ──

    #[test]
    fn tuple_same_types() {
        let mut i = new_interner();
        let t = ArType::Tuple(vec![int_t(&mut i), bool_t(&mut i)]);
        assert!(unify(&t, &t, &i));
    }

    #[test]
    fn tuple_different_length() {
        let mut i = new_interner();
        let a = ArType::Tuple(vec![int_t(&mut i)]);
        let b = ArType::Tuple(vec![int_t(&mut i), bool_t(&mut i)]);
        assert!(!unify(&a, &b, &i));
    }

    #[test]
    fn tuple_different_element_type() {
        let mut i = new_interner();
        let a = ArType::Tuple(vec![int_t(&mut i), int_t(&mut i)]);
        let b = ArType::Tuple(vec![int_t(&mut i), bool_t(&mut i)]);
        assert!(!unify(&a, &b, &i));
    }

    // ── Result unification ──

    #[test]
    fn result_same_ok_err() {
        let mut i = new_interner();
        let r = ArType::Result(int_t(&mut i), str_t(&mut i));
        assert!(unify(&r, &r, &i));
    }

    #[test]
    fn result_different_ok() {
        let mut i = new_interner();
        let s = str_t(&mut i);
        let a = ArType::Result(int_t(&mut i), s);
        let b = ArType::Result(bool_t(&mut i), s);
        assert!(!unify(&a, &b, &i));
    }

    #[test]
    fn result_different_err() {
        let mut i = new_interner();
        let int_id = int_t(&mut i);
        let a = ArType::Result(int_id, str_t(&mut i));
        let b = ArType::Result(int_id, bool_t(&mut i));
        assert!(!unify(&a, &b, &i));
    }

    // ── Option / Coroutine / Range unification ──

    #[test]
    fn option_same_inner() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        assert!(unify(&ArType::Option(inner), &ArType::Option(inner), &i));
    }

    #[test]
    fn option_different_inner() {
        let mut i = new_interner();
        assert!(!unify(
            &ArType::Option(int_t(&mut i)),
            &ArType::Option(bool_t(&mut i)),
            &i
        ));
    }

    #[test]
    fn coroutine_same_inner() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        assert!(unify(
            &ArType::Coroutine(inner),
            &ArType::Coroutine(inner),
            &i
        ));
    }

    #[test]
    fn range_same_inner() {
        let mut i = new_interner();
        let inner = int_t(&mut i);
        assert!(unify(&ArType::Range(inner), &ArType::Range(inner), &i));
    }

    // ── Err / Void unification ──

    #[test]
    fn err_with_err() {
        let i = new_interner();
        assert!(unify(&ArType::Err, &ArType::Err, &i));
    }

    #[test]
    fn err_does_not_unify_with_void() {
        let i = new_interner();
        assert!(!unify(&ArType::Err, &ArType::Void, &i));
        assert!(!unify(&ArType::Void, &ArType::Err, &i));
    }

    #[test]
    fn void_with_void() {
        let i = new_interner();
        assert!(unify(&ArType::Void, &ArType::Void, &i));
    }

    // ── Cross-category mismatches ──

    #[test]
    fn primitive_does_not_unify_with_named() {
        let i = new_interner();
        assert!(!unify(
            &ArType::Primitive(Primitive::Int),
            &ArType::Named(SymbolId::new(0, 1), vec![]),
            &i
        ));
    }

    #[test]
    fn nullablity_unwraps_once_only() {
        let mut i = new_interner();
        let int_id = int_t(&mut i);
        let null_int = ArType::Nullable(int_id);
        assert!(unify(&null_int, &ArType::Primitive(Primitive::Int), &i));
        // Nullable<Int> and plain Named do NOT unify
        assert!(!unify(
            &null_int,
            &ArType::Named(SymbolId::new(0, 1), vec![]),
            &i
        ));
    }

    // ── unify_return ──

    #[test]
    fn return_unifies_direct_match() {
        let i = new_interner();
        assert!(unify_return_type(
            &ArType::Primitive(Primitive::Int),
            &ArType::Primitive(Primitive::Int),
            &i
        ));
    }

    #[test]
    fn return_unifies_result_ok_err() {
        let mut i = new_interner();
        let ok_i = int_t(&mut i);
        let err_i = str_t(&mut i);
        let expected = ArType::Result(ok_i, err_i);
        let actual = ArType::Result(int_t(&mut i), str_t(&mut i));
        assert!(unify_return_type(&expected, &actual, &i));
    }

    #[test]
    fn return_unifies_result_void_err_with_nil() {
        let i = new_interner();
        let void_id = i.intern(ArType::Void);
        let err_type_id = i.intern(ArType::Error);
        let expected = ArType::Result(void_id, i.intern(ArType::Err));
        let actual = ArType::Nullable(err_type_id);
        assert!(unify_return_type(&expected, &actual, &i));
    }

    #[test]
    fn return_unifies_both_err() {
        let i = new_interner();
        let void_id = i.intern(ArType::Void);
        let err_id = i.intern(ArType::Err);
        let expected = ArType::Result(void_id, err_id);
        assert!(unify_return_type(&expected, &ArType::Err, &i));
        assert!(unify_return_type(
            &expected,
            &ArType::Nullable(i.intern(ArType::Error)),
            &i
        ));
    }

    #[test]
    fn return_rejects_mismatch() {
        let i = new_interner();
        assert!(!unify_return_type(
            &ArType::Primitive(Primitive::Int),
            &ArType::Primitive(Primitive::Bool),
            &i
        ));
    }

    // ── resolve_literal_pair ──

    #[test]
    fn literal_pair_int_literal_with_concrete() {
        let result = resolve_literal_pair(&ArType::IntLiteral, &ArType::Primitive(Primitive::I32));
        assert_eq!(result, ArType::Primitive(Primitive::I32));

        let result = resolve_literal_pair(&ArType::Primitive(Primitive::I32), &ArType::IntLiteral);
        assert_eq!(result, ArType::Primitive(Primitive::I32));
    }

    #[test]
    fn literal_pair_float_literal_with_concrete() {
        let result =
            resolve_literal_pair(&ArType::FloatLiteral, &ArType::Primitive(Primitive::F64));
        assert_eq!(result, ArType::Primitive(Primitive::F64));
    }

    #[test]
    fn literal_pair_two_int_literals() {
        let result = resolve_literal_pair(&ArType::IntLiteral, &ArType::IntLiteral);
        assert_eq!(result, ArType::Primitive(Primitive::Int));
    }

    #[test]
    fn literal_pair_two_float_literals() {
        let result = resolve_literal_pair(&ArType::FloatLiteral, &ArType::FloatLiteral);
        assert_eq!(result, ArType::Primitive(Primitive::Float));
    }

    #[test]
    fn literal_pair_int_and_float_literals() {
        let result = resolve_literal_pair(&ArType::IntLiteral, &ArType::FloatLiteral);
        assert_eq!(result, ArType::Primitive(Primitive::Float));

        let result = resolve_literal_pair(&ArType::FloatLiteral, &ArType::IntLiteral);
        assert_eq!(result, ArType::Primitive(Primitive::Float));
    }

    #[test]
    fn literal_pair_both_concrete_returns_first() {
        let result = resolve_literal_pair(
            &ArType::Primitive(Primitive::Int),
            &ArType::Primitive(Primitive::Bool),
        );
        assert_eq!(result, ArType::Primitive(Primitive::Int));
    }

    #[test]
    fn literal_pair_literal_absorbs_u32() {
        let result = resolve_literal_pair(&ArType::IntLiteral, &ArType::Primitive(Primitive::U32));
        assert_eq!(result, ArType::Primitive(Primitive::U32));
    }

    #[test]
    fn literal_pair_float_literal_with_int_concrete_promotes() {
        // IntLiteral + Float -> float should win
        let result = resolve_literal_pair(&ArType::IntLiteral, &ArType::FloatLiteral);
        assert_eq!(result, ArType::Primitive(Primitive::Float));
    }
}
