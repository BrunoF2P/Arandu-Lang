pub mod span;
pub mod index_vec;
pub mod stable_id;
pub mod string_pool;
pub mod line_index;
pub mod source_registry;
pub mod bitset;
pub mod scratch;

#[cfg(feature = "vm")]
pub mod vm;

#[cfg(feature = "vm")]
pub mod arena;

// Re-export core items for ease of use
pub use span::Span;
pub use index_vec::IndexVec;
pub use stable_id::{GenerationalId, SlotMap, DenseSlotMap, StableHandle};
pub use string_pool::{SsoString, StringPool, StringId};
pub use line_index::LineIndex;
pub use source_registry::{SourceFile, SourceRegistry};
pub use bitset::{BitSet, BitMatrix};
pub use scratch::with_scratch;

#[cfg(feature = "vm")]
pub use arena::BumpArena;
#[cfg(feature = "vm")]
pub use vm::VmReservation;
