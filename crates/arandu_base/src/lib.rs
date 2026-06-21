pub mod bitset;
pub mod index_vec;
pub mod line_index;
pub mod scratch;
pub mod source_registry;
pub mod span;
pub mod stable_id;
pub mod string_pool;

#[cfg(feature = "vm")]
pub mod vm;

#[cfg(feature = "vm")]
pub mod arena;

// Re-export core items for ease of use
pub use bitset::{BitMatrix, BitSet};
pub use index_vec::IndexVec;
pub use line_index::LineIndex;
pub use scratch::with_scratch;
pub use source_registry::{SourceFile, SourceRegistry};
pub use span::Span;
pub use stable_id::{DenseSlotMap, GenerationalId, SlotMap, StableHandle};
pub use string_pool::{SsoString, StringId, StringPool};

#[cfg(feature = "vm")]
pub use arena::BumpArena;
#[cfg(feature = "vm")]
pub use vm::VmReservation;
