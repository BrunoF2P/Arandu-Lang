#![cfg(feature = "vm")]

use crate::vm::VmReservation;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A page-aligned Bump Arena backed by lazy-committed virtual memory.
/// All allocations are fast, linear pointer increments.
pub struct BumpArena {
    vm: VmReservation,
    bump: AtomicUsize,
    committed: AtomicUsize,
    commit_lock: Mutex<()>,
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
            bump: AtomicUsize::new(0),
            committed: AtomicUsize::new(0),
            commit_lock: Mutex::new(()),
        }
    }

    /// Allocates memory with the given `Layout`. Commits pages (64KB chunks) lazily as needed.
    #[must_use]
    pub fn alloc_layout(&self, layout: std::alloc::Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();

        loop {
            let current_bump = self.bump.load(Ordering::Relaxed);

            // Align the bump offset
            let aligned_bump = (current_bump + align - 1) & !(align - 1);
            let next_bump = aligned_bump + size;

            if next_bump > self.vm.size() {
                panic!("Arena virtual address reservation limit exceeded");
            }

            // Commit extra 64KB pages on demand if next_bump crosses committed range
            let current_committed = self.committed.load(Ordering::Acquire);
            if next_bump > current_committed {
                let _lock = self.commit_lock.lock().unwrap();
                let current_committed = self.committed.load(Ordering::Relaxed);
                if next_bump > current_committed {
                    let new_committed = (next_bump + 65535) & !65535;
                    self.vm
                        .commit(current_committed, new_committed - current_committed)
                        .expect("Failed to commit VM page on BumpArena allocation");
                    self.committed.store(new_committed, Ordering::Release);
                }
            }

            if self
                .bump
                .compare_exchange_weak(current_bump, next_bump, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                return unsafe { self.vm.base_ptr().add(aligned_bump) };
            }
        }
    }

    /// Allocates a single value of type `T` inside the arena and returns a mutable reference.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc<T>(&self, val: T) -> &mut T {
        let ptr = self.alloc_layout(std::alloc::Layout::new::<T>()) as *mut T;
        unsafe {
            ptr.write(val);
            &mut *ptr
        }
    }

    /// Allocates a slice of elements of type `T` by cloning them into the arena.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice<T: Clone>(&self, slice: &[T]) -> &mut [T] {
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
        self.bump.load(Ordering::Relaxed)
    }

    /// Ends a temporary arena scope, rolling back the bump pointer to the mark.
    /// The physical memory pages remain committed, ensuring fast reuse on subsequent passes.
    pub fn end_temp(&mut self, mark: usize) {
        self.bump.store(mark, Ordering::Relaxed);
    }

    /// Resets the bump pointer to the beginning. Memory is reused instantly.
    pub fn reset(&mut self) {
        self.bump.store(0, Ordering::Relaxed);
    }
}

unsafe impl Send for BumpArena {}
unsafe impl Sync for BumpArena {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_arena_alloc() {
        let arena = BumpArena::new(1024 * 1024);

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
