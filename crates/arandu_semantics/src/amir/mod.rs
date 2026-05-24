pub mod block;
pub mod dominators;
pub mod local;
pub mod pretty;
pub mod program;
pub mod stmt;
pub mod value;

pub use block::{AmirBasicBlock, BlockId};
pub use dominators::Dominators;
pub use local::{AmirLocal, AmirReceiver, AmirTemp, LocalId, TempId};
pub use program::{AmirFunc, AmirProgram};
pub use stmt::{AmirStmt, AmirTerminator};
pub use value::{AmirConstant, AmirOperand, AmirPlace, AmirProjection, AmirRvalue};
