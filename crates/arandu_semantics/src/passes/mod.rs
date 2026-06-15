pub mod lower_hir;
pub mod lowering;
pub mod monomorphize;

pub use arandu_resolve::name_resolution;
pub use arandu_typeck::type_checker;
pub use arandu_mir::liveness;
pub use arandu_mir::optimize;
