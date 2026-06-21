pub use arandu_middle::types::{
    ArType, GenericSubst, Primitive, TypeId, TypeInterner, build_subst, intern_type, is_err_type,
    is_option_type, is_result_type, is_tryable_type, lower_named_type, lower_result_type,
    lower_type_expr, resolve_literal_pair, result_ok_err, result_type_decl_span, substitute_type,
    try_ok_type, type_interner, type_name_base, unify, unify_return,
};

pub mod generic_inst;
pub mod interfaces;

pub use generic_inst::{
    collect_generic_param_symbols, struct_fields_instantiated, synth_generic_instantiation,
};
pub use interfaces::{InterfaceInfo, collect_interfaces_and_constraints};
