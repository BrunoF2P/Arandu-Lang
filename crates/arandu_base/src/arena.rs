#![cfg(feature = "vm")]

use crate::vm::VmReservation;
use std::cell::Cell;

/// A page-aligned Bump Arena backed by lazy-committed virtual memory.
/// All allocations are fast, linear pointer increments.
pub struct BumpArena {
    vm: VmReservation,
    bump: Cell<usize>,
    committed: Cell<usize>,
}

impl BumpArena {
    /// Creates a new `BumpArena` with the specified virtual memory reservation size.
    /// Default standard size is 4GB for seamless growth.
    #[must_use]
    pub fn new(reserve_size: usize) -> Self {
        let vm =
            VmReservation::reserve(reserve_size).expect("Failed to reserve VM block for BumpArena");
        Self {
            vm,
            bump: Cell::new(0),
            committed: Cell::new(0),
        }
    }

    /// Allocates memory with the given `Layout`. Commits pages (64KB chunks) lazily as needed.
    #[must_use]
    pub fn alloc_layout(&mut self, layout: std::alloc::Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let current_bump = self.bump.get();

        // Align the bump offset
        let aligned_bump = (current_bump + align - 1) & !(align - 1);
        let next_bump = aligned_bump + size;

        if next_bump > self.vm.size() {
            panic!("Arena virtual address reservation limit exceeded");
        }

        // Commit extra 64KB pages on demand if next_bump crosses committed range
        let current_committed = self.committed.get();
        if next_bump > current_committed {
            let new_committed = (next_bump + 65535) & !65535;
            self.vm
                .commit(current_committed, new_committed - current_committed)
                .expect("Failed to commit VM page on BumpArena allocation");
            self.committed.set(new_committed);
        }

        self.bump.set(next_bump);
        unsafe { self.vm.base_ptr().add(aligned_bump) }
    }

    /// Allocates a single value of type `T` inside the arena and returns a mutable reference.
    pub fn alloc<T>(&mut self, val: T) -> &mut T {
        let ptr = self.alloc_layout(std::alloc::Layout::new::<T>()) as *mut T;
        unsafe {
            ptr.write(val);
            &mut *ptr
        }
    }

    /// Allocates a slice of elements of type `T` by cloning them into the arena.
    pub fn alloc_slice<T: Clone>(&mut self, slice: &[T]) -> &mut [T] {
        if slice.is_empty() {
            return &mut [];
        }
        let layout = std::alloc::Layout::array::<T>(slice.len()).expect("Invalid array layout");
        let ptr = self.alloc_layout(layout) as *mut T;
        unsafe {
            for (i, item) in slice.iter().enumerate() {
                ptr.add(i).write(item.clone());
            }
            std::slice::from_raw_parts_mut(ptr, slice.len())
        }
    }

    /// Begins a temporary arena scope, returning the current allocation mark.
    #[must_use]
    pub fn begin_temp(&self) -> usize {
        self.bump.get()
    }

    /// Ends a temporary arena scope, rolling back the bump pointer to the mark.
    /// The physical memory pages remain committed, ensuring fast reuse on subsequent passes.
    pub fn end_temp(&mut self, mark: usize) {
        self.bump.set(mark);
    }

    /// Resets the bump pointer to the beginning. Memory is reused instantly.
    pub fn reset(&mut self) {
        self.bump.set(0);
    }
}

unsafe impl Send for BumpArena {}
unsafe impl Sync for BumpArena {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_arena_alloc() {
        let mut arena = BumpArena::new(1024 * 1024);

        let ptr1 = arena.alloc(42i32) as *mut i32;
        let ptr2 = arena.alloc(100i32) as *mut i32;

        unsafe {
            assert_eq!(*ptr1, 42);
            assert_eq!(*ptr2, 100);
        }

        let slice = arena.alloc_slice(&[1, 2, 3]);
        assert_eq!(slice, &[1, 2, 3]);
    }
}
