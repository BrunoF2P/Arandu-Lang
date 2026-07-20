//! Host-backed growable `i64` vectors for Minimal `std.alloc.vec`.
//!
//! ## Why host-backed (not pure Arandu mem intrinsics yet)
//! - `mem.ptrOffset` / `sizeOf` lower as AMIR calls (`fn@ptrOffset`) that the JIT
//!   must treat as intrinsics; partial support exists only for fully-qualified
//!   `std.core.mem.ptr_read*` names.
//! - Method monomorphization of imported templates still has residual gaps.
//!
//! Pattern matches `gen_runtime` (GenArena i64 MVP): language surface in
//! `stdlib/alloc/vec.aru`, storage and growth in this module.
//!
//! Elements are **i64 bit patterns** (Minimal gold uses `int`). Typed Drop /
//! non-int payloads remain PROMOTE-L6.1 / self-host.
//!
//! # Safety
//! All `pub unsafe extern "C"` entry points are ABI host functions invoked only
//! from JIT-compiled Arandu code (or unit tests). Handles are opaque indices
//! into the process-local table; invalid ids are treated as no-ops or abort
//! on write paths that would corrupt storage.

#![allow(clippy::missing_safety_doc)]

use std::sync::Mutex;

struct Slot {
    data: Vec<i64>,
}

static VECS: Mutex<Vec<Option<Slot>>> = Mutex::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, Vec<Option<Slot>>> {
    VECS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Create an empty vector; returns handle `>= 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_new() -> i64 {
    let mut g = lock();
    let slot = Slot { data: Vec::new() };
    if let Some(idx) = g.iter().position(|s| s.is_none()) {
        g[idx] = Some(slot);
        return idx as i64;
    }
    let id = g.len();
    g.push(Some(slot));
    id as i64
}

/// Push `value` onto vector `id`. Invalid id aborts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_push(id: i64, value: i64) {
    let mut g = lock();
    let Some(Some(slot)) = g.get_mut(id as usize) else {
        std::process::abort();
    };
    slot.data.push(value);
}

/// Length of vector `id`, or `-1` if invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_len(id: i64) -> i64 {
    let g = lock();
    match g.get(id as usize).and_then(|s| s.as_ref()) {
        Some(slot) => slot.data.len() as i64,
        None => -1,
    }
}

/// `1` if `index` is in range, else `0`. Invalid id → `0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_has(id: i64, index: i64) -> i64 {
    if index < 0 {
        return 0;
    }
    let g = lock();
    match g.get(id as usize).and_then(|s| s.as_ref()) {
        Some(slot) if (index as usize) < slot.data.len() => 1,
        _ => 0,
    }
}

/// Get element at `index`. Invalid / OOB → `0` (check [`ar_vec_has`] first).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_get(id: i64, index: i64) -> i64 {
    if index < 0 {
        return 0;
    }
    let g = lock();
    match g.get(id as usize).and_then(|s| s.as_ref()) {
        Some(slot) => slot.data.get(index as usize).copied().unwrap_or(0),
        None => 0,
    }
}

/// Overwrite index; returns `1` on success, `0` on OOB/invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_put(id: i64, index: i64, value: i64) -> i64 {
    if index < 0 {
        return 0;
    }
    let mut g = lock();
    let Some(Some(slot)) = g.get_mut(id as usize) else {
        return 0;
    };
    match slot.data.get_mut(index as usize) {
        Some(cell) => {
            *cell = value;
            1
        }
        None => 0,
    }
}

/// Pop last element and return it. Caller must ensure non-empty (`ar_vec_len > 0`).
/// Empty / invalid → `0` (ambiguous with a stored zero — check length first).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_pop(id: i64) -> i64 {
    let mut g = lock();
    let Some(Some(slot)) = g.get_mut(id as usize) else {
        return 0;
    };
    slot.data.pop().unwrap_or(0)
}

/// Set length to 0; capacity retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_clear(id: i64) {
    let mut g = lock();
    if let Some(Some(slot)) = g.get_mut(id as usize) {
        slot.data.clear();
    }
}

/// Destroy handle and free storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_vec_destroy(id: i64) {
    let mut g = lock();
    if (id as usize) < g.len() {
        g[id as usize] = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_get_len_free() {
        unsafe {
            let id = ar_vec_new();
            ar_vec_push(id, 10);
            ar_vec_push(id, 20);
            assert_eq!(ar_vec_len(id), 2);
            assert_eq!(ar_vec_has(id, 0), 1);
            assert_eq!(ar_vec_get(id, 0), 10);
            assert_eq!(ar_vec_get(id, 1), 20);
            assert_eq!(ar_vec_put(id, 1, 5), 1);
            assert_eq!(ar_vec_get(id, 1), 5);
            assert_eq!(ar_vec_pop(id), 5);
            assert_eq!(ar_vec_len(id), 1);
            ar_vec_destroy(id);
            assert_eq!(ar_vec_len(id), -1);
        }
    }
}
