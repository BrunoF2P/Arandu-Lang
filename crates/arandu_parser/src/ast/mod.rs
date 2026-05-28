pub mod ast_pool;
mod decl;
mod dump;
mod expr;
mod pattern;
mod stmt;
mod types;

pub use ast_pool::*;
pub use decl::*;
pub use expr::*;
pub use pattern::*;
pub use stmt::*;
pub use types::*;

impl Program {
    #[must_use]
    pub fn dump(&self) -> String {
        dump::dump_program(self)
    }
}
