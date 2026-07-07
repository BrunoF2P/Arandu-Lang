use super::super::TypeChecker;
use super::super::types::{ArType, Primitive};
use arandu_parser::Program;

pub(crate) fn register_prelude(checker: &mut TypeChecker<'_>, _program: &Program) {
    let any_id = checker.intern(ArType::Primitive(Primitive::Any));
    let void_id = checker.intern(ArType::Void);
    let str_id = checker.intern(ArType::Primitive(Primitive::Str));
    let err_literal_id = checker.intern(ArType::Err);

    let result_any_err = checker.intern(ArType::Result(any_id, err_literal_id));
    let result_void_err = checker.intern(ArType::Result(void_id, err_literal_id));

    let println_ty = ArType::Func(vec![any_id], void_id);
    let create_ty = ArType::Func(vec![str_id], result_any_err);
    let remove_ty = ArType::Func(vec![str_id], result_void_err);
    let err_new_ty = ArType::Func(vec![str_id], err_literal_id);

    for (module, members_with_types) in [
        (
            "io",
            vec![
                ("println", println_ty),
                ("create", create_ty),
                ("remove", remove_ty),
            ],
        ),
        ("err", vec![("new", err_new_ty)]),
    ] {
        for (member, ty) in members_with_types {
            if let Some(symbol_id) = checker.symbols.lookup_module_member(module, member) {
                let ty_id = checker.intern(ty);
                checker.record_decl_type(symbol_id, ty_id);
            }
        }
    }
}
