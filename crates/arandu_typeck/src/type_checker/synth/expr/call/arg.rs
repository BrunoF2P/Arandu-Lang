use arandu_lexer::Span;

use crate::type_checker::TypeChecker;
use crate::type_checker::constraints::ConstraintOrigin;
use crate::type_checker::types::{ArType, Primitive};

use arandu_middle::types::type_interner::TypeId;

/// Check a call argument against its formal parameter.
///
/// When the formal type is `str` and the argument is ToStr-v0.1-formatable,
/// accept without a constraint (AMIR lower inserts `ToStr`). When formal is
/// `str` and the argument is not formatable, emit T034. Otherwise fall back
/// to the usual CallArg constraint.
pub(crate) fn check_call_arg(
    checker: &mut TypeChecker<'_>,
    param_id: TypeId,
    arg_ty_id: TypeId,
    call_span: Span,
    param_span: Span,
    arg_span: Span,
    arg_index: usize,
) {
    if checker.is_assignable(arg_ty_id, param_id) {
        let arg_ty = checker.resolve(arg_ty_id);
        let param_ty = checker.resolve(param_id);
        if !arg_ty.is_literal()
            && arg_ty != param_ty
            && param_ty.is_numeric()
            && arg_ty.is_numeric()
        {
            checker.add_constraint(
                param_id,
                arg_ty_id,
                ConstraintOrigin::ImplicitWidening {
                    source_span: arg_span,
                    target_span: call_span,
                },
            );
        }
        return;
    }

    let param_ty = checker.resolve(param_id);
    let arg_ty = checker.resolve(arg_ty_id);

    // W3.3 auto-ref: formal `&T` / `&mut T`, actual `T` → accept (lower inserts Borrow).
    if let ArType::Ref(inner) | ArType::RefMut(inner) = param_ty
        && checker.is_assignable(arg_ty_id, inner)
    {
        return;
    }
    // Auto-deref: formal `T`, actual `&T` / `&mut T`.
    if let ArType::Ref(inner) | ArType::RefMut(inner) = arg_ty
        && checker.is_assignable(inner, param_id)
    {
        return;
    }

    if matches!(param_ty, ArType::Primitive(Primitive::Str)) {
        if arg_ty.is_error() || arg_ty.is_to_str_v01() {
            // ToStr v0.1: lower will insert AmirRvalue::ToStr.
            return;
        }
        let interner = &checker.type_info.type_interner;
        let found = arg_ty.display(&checker.symbols, interner);
        checker.diagnostics.push(
            crate::Diagnostic::error(
                crate::DiagCode::T034CannotFormat,
                format!("cannot format value of type `{found}` as `str`"),
                arg_span,
            )
            .with_note(
                "only bool, integers, floats, char, and str are supported in v0.1".to_string(),
            )
            .with_label(param_span, "parameter expects `str`"),
        );
        return;
    }

    checker.add_subtype_constraint(
        param_id,
        arg_ty_id,
        ConstraintOrigin::CallArg {
            call_span,
            param_span,
            arg_span,
            arg_index,
        },
    );
}
