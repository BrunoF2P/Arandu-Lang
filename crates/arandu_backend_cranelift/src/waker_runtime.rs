//! SL_R — host Waker / Context for cooperative wake (no OS thread park yet).
//!
//! Wakers are explicit handles (like SyncExecutor). `wake` sets a flag;
//! `wait` spins / sleeps until set or timeout. Full task queues land with
//! multi-thread SyncExecutor.

use std::sync::Mutex;
use std::time::{Duration, Instant};

struct WakerSlot {
    woken: bool,
}

static WAKERS: Mutex<Vec<Option<WakerSlot>>> = Mutex::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, Vec<Option<WakerSlot>>> {
    WAKERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Create a waker. Returns id >= 0.
///
/// # Safety
/// C ABI for JIT.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_waker_create() -> i64 {
    let mut g = lock();
    let slot = WakerSlot { woken: false };
    if let Some(idx) = g.iter().position(|s| s.is_none()) {
        g[idx] = Some(slot);
        return idx as i64;
    }
    let id = g.len() as i64;
    g.push(Some(slot));
    id
}

/// Mark waker ready (idempotent).
///
/// # Safety
/// `id` from create.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_waker_wake(id: i64) {
    if id < 0 {
        return;
    }
    let mut g = lock();
    if let Some(Some(slot)) = g.get_mut(id as usize) {
        slot.woken = true;
    }
}

/// Wait until woken or `timeout_ms` elapses (-1 = forever).
/// Returns 1 if woken, 0 on timeout, -1 on error.
///
/// # Safety
/// `id` from create.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_waker_wait(id: i64, timeout_ms: i64) -> i64 {
    if id < 0 {
        return -1;
    }
    let deadline = if timeout_ms < 0 {
        None
    } else {
        Some(Instant::now() + Duration::from_millis(timeout_ms as u64))
    };
    loop {
        {
            let mut g = lock();
            if let Some(Some(slot)) = g.get_mut(id as usize) {
                if slot.woken {
                    slot.woken = false;
                    return 1;
                }
            } else {
                return -1;
            }
        }
        if let Some(dl) = deadline
            && Instant::now() >= dl
        {
            return 0;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Destroy a waker handle.
///
/// # Safety
/// `id` from create.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_waker_destroy(id: i64) {
    if id < 0 {
        return;
    }
    let mut g = lock();
    if let Some(slot) = g.get_mut(id as usize) {
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_wait() {
        unsafe {
            let w = ar_rt_waker_create();
            ar_rt_waker_wake(w);
            assert_eq!(ar_rt_waker_wait(w, 100), 1);
            ar_rt_waker_destroy(w);
        }
    }
}
