//! SL_R.0 — cooperative multi-task host for debug JIT (i64 payload MVP).
//!
//! Complements [`crate::poll_runtime`] (single-coroutine poll/block_on).
//!
//! ## Model
//! - Explicit handles, no global language-level executor in user code beyond
//!   these host symbols (stdlib wraps them as `SyncExecutor`).
//! - `spawn` parks a coroutine state blob; `join` drives it with
//!   [`ar_co_block_on_i64`](crate::poll_runtime::ar_co_block_on_i64).
//! - Cooperative only: Pending spins (no OS reactor yet — SL_R.2).

use crate::poll_runtime::{ar_co_block_on_i64, ar_co_free};
use std::sync::Mutex;

struct TaskSlot {
    state: *mut u8,
    done: bool,
    result: i64,
}

// Safety: JIT is single-threaded today; Mutex for future multi-thread SyncExecutor.
unsafe impl Send for TaskSlot {}

static TASKS: Mutex<Vec<Option<TaskSlot>>> = Mutex::new(Vec::new());

/// Spawn a coroutine state onto the SyncExecutor queue. Returns handle (>= 0).
///
/// # Safety
/// `state` must be a valid coroutine blob (same as poll_runtime).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_spawn_i64(state: *mut u8) -> i64 {
    if state.is_null() {
        std::process::abort();
    }
    let mut guard = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    let slot = TaskSlot {
        state,
        done: false,
        result: 0,
    };
    // Reuse free slots
    if let Some(idx) = guard.iter().position(|s| s.is_none()) {
        guard[idx] = Some(slot);
        return idx as i64;
    }
    let id = guard.len();
    guard.push(Some(slot));
    id as i64
}

/// Drive task `handle` to completion; returns i64 payload. Invalid handle aborts.
///
/// # Safety
/// `handle` must come from [`ar_rt_spawn_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_join_i64(handle: i64) -> i64 {
    if handle < 0 {
        std::process::abort();
    }
    let idx = handle as usize;
    let state = {
        let mut guard = TASKS.lock().unwrap_or_else(|e| e.into_inner());
        let slot = guard.get_mut(idx).and_then(|s| s.as_mut());
        let Some(slot) = slot else {
            std::process::abort();
        };
        if slot.done {
            return slot.result;
        }
        slot.state
    };
    let result = unsafe { ar_co_block_on_i64(state) };
    {
        let mut guard = TASKS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(Some(slot)) = guard.get_mut(idx) {
            slot.done = true;
            slot.result = result;
            // Free blob after join (ownership transfer to runtime).
            unsafe {
                ar_co_free(slot.state);
            }
            slot.state = std::ptr::null_mut();
        }
    }
    result
}

/// Block on a single coroutine without spawn (alias surface for std.runtime).
///
/// # Safety
/// Same as [`ar_co_block_on_i64`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_block_on_i64(state: *mut u8) -> i64 {
    unsafe { ar_co_block_on_i64(state) }
}

/// Drop a finished/unneeded handle without joining (frees if not done).
///
/// # Safety
/// Handle from spawn; not usable after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_cancel_i64(handle: i64) {
    if handle < 0 {
        return;
    }
    let idx = handle as usize;
    let mut guard = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(slot) = guard.get_mut(idx).and_then(|s| s.take()) {
        if !slot.state.is_null() {
            unsafe {
                ar_co_free(slot.state);
            }
        }
    }
}

/// Path absolute check for SL_S / Minimal path helpers.
///
/// Uses the host [`std::path::Path::is_absolute`] semantics (Unix `/…`, Windows
/// drive/UNC). Empty and invalid UTF-8 are never absolute.
///
/// # Safety
/// `ptr`/`len` fat string from Arandu JIT.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_path_is_absolute(ptr: *const u8, len: i64) -> i64 {
    if len <= 0 || ptr.is_null() {
        return 0;
    }
    let s = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let Ok(text) = std::str::from_utf8(s) else {
        return 0;
    };
    i64::from(std::path::Path::new(text).is_absolute())
}

/// Path empty check.
///
/// # Safety
/// Fat string ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_path_is_empty(_ptr: *const u8, len: i64) -> i64 {
    i64::from(len <= 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poll_runtime::ar_co_make_ready_i64;

    #[test]
    fn spawn_join_ready() {
        unsafe {
            let s = ar_co_make_ready_i64(42);
            let h = ar_rt_spawn_i64(s);
            assert_eq!(ar_rt_join_i64(h), 42);
        }
    }

    #[test]
    fn path_absolute() {
        unsafe {
            let p = b"/tmp";
            assert_eq!(ar_path_is_absolute(p.as_ptr(), 4), 1);
            assert_eq!(ar_path_is_absolute(b"/".as_ptr(), 1), 1);
            assert_eq!(ar_path_is_absolute(b"rel".as_ptr(), 3), 0);
            assert_eq!(ar_path_is_absolute(b"".as_ptr(), 0), 0);
            assert_eq!(ar_path_is_absolute(b"./x".as_ptr(), 3), 0);
            assert_eq!(ar_path_is_empty(b"".as_ptr(), 0), 1);
        }
    }
}
