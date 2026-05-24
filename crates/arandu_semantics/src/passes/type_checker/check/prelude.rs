use super::super::TypeChecker;
use super::super::types::{ArType, Primitive};

pub(crate) fn register_prelude(checker: &mut TypeChecker) {
    for (module, members_with_types) in [
        (
            "io",
            vec![
                (
                    "println",
                    ArType::Func(
                        vec![ArType::Primitive(Primitive::Any)],
                        Box::new(ArType::Void),
                    ),
                ),
                (
                    "create",
                    ArType::Func(
                        vec![ArType::Primitive(Primitive::Str)],
                        Box::new(ArType::Result(
                            Box::new(ArType::Primitive(Primitive::Any)),
                            Box::new(ArType::Err),
                        )),
                    ),
                ),
                (
                    "remove",
                    ArType::Func(
                        vec![ArType::Primitive(Primitive::Str)],
                        Box::new(ArType::Result(
                            Box::new(ArType::Void),
                            Box::new(ArType::Err),
                        )),
                    ),
                ),
            ],
        ),
        (
            "err",
            vec![(
                "new",
                ArType::Func(
                    vec![ArType::Primitive(Primitive::Str)],
                    Box::new(ArType::Err),
                ),
            )],
        ),
    ] {
        for (member, ty) in members_with_types {
            if let Some(symbol_id) = checker.symbols.lookup_module_member(module, member) {
                checker.record_decl_type(symbol_id, ty);
            }
        }
    }
}
