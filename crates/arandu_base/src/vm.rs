#![cfg(feature = "vm")]

#[cfg(unix)]
use std::ptr;

/// A page-aligned virtual memory reservation wrapper.
/// Reserves a large contiguous address block and allows committing pages lazily.
pub struct VmReservation {
    #[cfg(unix)]
    addr: *mut libc::c_void,
    #[cfg(not(unix))]
    ptr: *mut u8,
    #[cfg(not(unix))]
    layout: std::alloc::Layout,
    size: usize,
}

#[cfg(unix)]
impl VmReservation {
    /// Reserves a contiguous address space of the given size.
    /// The size is rounded up to 64KB (page boundary).
    pub fn reserve(size: usize) -> Result<Self, &'static str> {
        let size = (size + 65535) & !65535;
        let addr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if addr == libc::MAP_FAILED {
            return Err("Failed to reserve virtual memory address space");
        }
        Ok(Self { addr, size })
    }

    /// Commits physical pages for a range within the reservation.
    pub fn commit(&self, offset: usize, len: usize) -> Result<(), &'static str> {
        if offset + len > self.size {
            return Err("Commit range is out of bounds");
        }
        let page_offset = offset & !65535;
        let page_len = (offset + len - page_offset + 65535) & !65535;
        let commit_addr = unsafe { self.addr.add(page_offset) };
        let ret =
            unsafe { libc::mprotect(commit_addr, page_len, libc::PROT_READ | libc::PROT_WRITE) };
        if ret != 0 {
            return Err("Failed to commit virtual memory pages");
        }
        Ok(())
    }

    /// Returns a raw pointer to the base address of the reservation.
    #[must_use]
    pub fn base_ptr(&self) -> *mut u8 {
        self.addr as *mut u8
    }

    /// Returns the reserved virtual memory size in bytes.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }
}

#[cfg(unix)]
impl Drop for VmReservation {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr, self.size);
        }
    }
}

#[cfg(not(unix))]
impl VmReservation {
    pub fn reserve(size: usize) -> Result<Self, &'static str> {
        let size = (size + 65535) & !65535;
        let layout = std::alloc::Layout::from_size_align(size, 65536)
            .map_err(|_| "Failed to create 64KB aligned layout")?;
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            return Err("Failed to allocate virtual memory fallback block");
        }
        Ok(Self { ptr, layout, size })
    }

    pub fn commit(&self, _offset: usize, _len: usize) -> Result<(), &'static str> {
        // Fallback layout is pre-allocated and committed by the OS allocator
        Ok(())
    }

    #[must_use]
    pub fn base_ptr(&self) -> *mut u8 {
        self.ptr
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }
}

#[cfg(not(unix))]
impl Drop for VmReservation {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.ptr, self.layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_reservation_lifecycle() {
        let size = 65536 * 4; // 256 KB
        let vm = VmReservation::reserve(size).expect("Failed to reserve VM block");
        assert!(vm.size() >= size);
        assert!(!vm.base_ptr().is_null());

        // Commit first page
        vm.commit(0, 65536).expect("Failed to commit page");
    }
}
