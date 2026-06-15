mod ar_type;
mod lower;
mod primitive;
mod result_option;
mod subst;
pub mod type_interner;
mod unify;

pub use ar_type::ArType;
pub use lower::{lower_result_type, lower_type_expr, lower_named_type};
pub use primitive::Primitive;
pub use result_option::{
    is_err_type, is_option_type, is_result_type, is_tryable_type, result_ok_err,
    result_type_decl_span, try_ok_type, type_name_base,
};
pub use subst::{GenericSubst, build_subst, substitute_type};
pub use type_interner::{TypeId, TypeInterner};
pub use unify::{resolve_literal_pair, unify, unify_return};
