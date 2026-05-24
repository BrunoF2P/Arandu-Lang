use arandu_parser::{Program, TopLevelDecl};

use super::super::TypeChecker;
use super::collect::{collect_signature_types, collect_type_shapes};
use super::super::types::collect_interfaces_and_constraints;
use super::func::check_func_body;
use super::prelude::register_prelude;
use super::validate::validate_top_level_any;

pub fn check_program(checker: &mut TypeChecker, program: &Program) {
    register_prelude(checker);
    collect_type_shapes(checker, program);
    collect_signature_types(checker, program);
    collect_interfaces_and_constraints(checker, program);

    for decl in &program.decls {
        validate_top_level_any(checker, decl);

        match decl {
            TopLevelDecl::Func(func_decl) => {
                check_func_body(checker, func_decl);
            }
            TopLevelDecl::Const(const_decl) => {
                let val_ty = super::super::synth::synth_expr(checker, &const_decl.value);
                let const_key = crate::NodeKey::from(const_decl.span);
                if let Some(symbol_id) = checker.resolved.definitions.get(&const_key) {
                    checker.type_info.decl_types.insert(*symbol_id, val_ty);
                }
            }
            _ => {}
        }
    }
}
