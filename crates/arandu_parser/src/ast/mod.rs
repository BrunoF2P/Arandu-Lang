mod decl;
mod dump;
mod expr;
mod pattern;
mod stmt;
mod types;

pub use decl::*;
pub use expr::*;
pub use pattern::*;
pub use stmt::*;
pub use types::*;

impl Program {
    pub fn dump(&self) -> String {
        dump::dump_program(self)
    }
}
