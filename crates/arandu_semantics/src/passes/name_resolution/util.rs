use arandu_parser::{BinaryOp, UnaryOp};

pub(crate) fn is_type_case(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

#[allow(dead_code)]
fn _keep_ops_exhaustive(_: UnaryOp, _: BinaryOp) {}
