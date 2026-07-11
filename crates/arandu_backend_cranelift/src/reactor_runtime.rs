//! SL_R.2 — cooperative reactor host for debug JIT (Linux epoll + timerfd).
//!
//! ## Surface
//! - [`ar_rt_reactor_create`] / [`ar_rt_reactor_destroy`]: opaque reactor handle
//! - [`ar_rt_reactor_sleep_ms`]: block until `ms` elapses (timerfd + epoll_wait)
//! - [`ar_rt_reactor_poll_ms`]: wait up to `timeout_ms` for the next armed event
//!
//! Non-Linux builds use a portable `thread::sleep` fallback so the language
//! surface stays available; production path is Linux epoll (io_uring = SL_R.3).
//!
//! No global language-level reactor: handles are explicit values (like
//! `SyncExecutor`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Opaque reactor id (>= 0). Invalid / closed = negative.
type ReactorId = i64;

struct ReactorSlot {
    /// Linux: epoll fd. Portable fallback: ignored (sleep only).
    #[cfg(target_os = "linux")]
    epoll_fd: i32,
    /// Optional one-shot timer fd still registered (linux).
    #[cfg(target_os = "linux")]
    timer_fd: Option<i32>,
    /// Wall-clock deadline for the armed timer (all platforms).
    deadline: Option<Instant>,
}

// JIT is single-threaded today; Mutex keeps the table sound if that changes.
static REACTORS: Mutex<Vec<Option<ReactorSlot>>> = Mutex::new(Vec::new());

fn lock_reactors() -> std::sync::MutexGuard<'static, Vec<Option<ReactorSlot>>> {
    REACTORS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Create a reactor. Returns handle (>= 0) or -1 on failure.
///
/// # Safety
/// C ABI for Cranelift JIT.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_reactor_create() -> ReactorId {
    #[cfg(target_os = "linux")]
    {
        let epfd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if epfd < 0 {
            return -1;
        }
        let slot = ReactorSlot {
            epoll_fd: epfd,
            timer_fd: None,
            deadline: None,
        };
        let mut guard = lock_reactors();
        if let Some(idx) = guard.iter().position(|s| s.is_none()) {
            guard[idx] = Some(slot);
            return idx as i64;
        }
        let id = guard.len() as i64;
        guard.push(Some(slot));
        id
    }
    #[cfg(not(target_os = "linux"))]
    {
        let slot = ReactorSlot { deadline: None };
        let mut guard = lock_reactors();
        if let Some(idx) = guard.iter().position(|s| s.is_none()) {
            guard[idx] = Some(slot);
            return idx as i64;
        }
        let id = guard.len() as i64;
        guard.push(Some(slot));
        id
    }
}

/// Destroy a reactor and free OS resources.
///
/// # Safety
/// `id` from [`ar_rt_reactor_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_reactor_destroy(id: ReactorId) {
    if id < 0 {
        return;
    }
    let mut guard = lock_reactors();
    let Some(slot) = guard.get_mut(id as usize).and_then(|s| s.take()) else {
        return;
    };
    #[cfg(target_os = "linux")]
    {
        if let Some(tfd) = slot.timer_fd {
            unsafe {
                let _ = libc::close(tfd);
            }
        }
        unsafe {
            let _ = libc::close(slot.epoll_fd);
        }
    }
    let _ = slot;
}

/// Block until `ms` milliseconds elapse (uses reactor's epoll + timerfd on Linux).
///
/// Returns 0 on success, -1 on error. Does not leave a pending timer armed.
///
/// # Safety
/// `id` from create; `ms` >= 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_reactor_sleep_ms(id: ReactorId, ms: i64) -> i64 {
    if id < 0 || ms < 0 {
        return -1;
    }
    // Arm then wait with infinite timeout for this one timer.
    if unsafe { ar_rt_reactor_arm_timer_ms(id, ms) } != 0 {
        return -1;
    }
    let rc = unsafe { ar_rt_reactor_poll_ms(id, -1) };
    if rc < 0 {
        return -1;
    }
    0
}

/// Arm a one-shot timer for `ms` without blocking. Cancels any previous timer.
///
/// # Safety
/// `id` valid reactor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_reactor_arm_timer_ms(id: ReactorId, ms: i64) -> i64 {
    if id < 0 || ms < 0 {
        return -1;
    }
    let mut guard = lock_reactors();
    let Some(slot) = guard.get_mut(id as usize).and_then(|s| s.as_mut()) else {
        return -1;
    };
    slot.deadline = Some(Instant::now() + Duration::from_millis(ms as u64));

    #[cfg(target_os = "linux")]
    {
        // Drop previous timerfd if any.
        if let Some(old) = slot.timer_fd.take() {
            unsafe {
                let _ = libc::epoll_ctl(slot.epoll_fd, libc::EPOLL_CTL_DEL, old, std::ptr::null_mut());
                let _ = libc::close(old);
            }
        }
        let tfd = unsafe {
            libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_CLOEXEC | libc::TFD_NONBLOCK)
        };
        if tfd < 0 {
            return -1;
        }
        let its = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: (ms / 1000) as libc::time_t,
                tv_nsec: ((ms % 1000) * 1_000_000) as libc::c_long,
            },
        };
        // Zero it_value would disarm; clamp sub-ms to 1ns.
        let mut its = its;
        if its.it_value.tv_sec == 0 && its.it_value.tv_nsec == 0 {
            its.it_value.tv_nsec = 1;
        }
        if unsafe { libc::timerfd_settime(tfd, 0, &its, std::ptr::null_mut()) } < 0 {
            unsafe {
                let _ = libc::close(tfd);
            }
            return -1;
        }
        let mut ev: libc::epoll_event = unsafe { std::mem::zeroed() };
        ev.events = libc::EPOLLIN as u32;
        ev.u64 = tfd as u64;
        if unsafe { libc::epoll_ctl(slot.epoll_fd, libc::EPOLL_CTL_ADD, tfd, &mut ev) } < 0 {
            unsafe {
                let _ = libc::close(tfd);
            }
            return -1;
        }
        slot.timer_fd = Some(tfd);
    }
    0
}

/// Wait for the next reactor event, up to `timeout_ms` (-1 = forever, 0 = nonblock).
///
/// Returns:
/// - `1` if the armed timer fired (and is cleared)
/// - `0` on timeout with no event
/// - `-1` on error
///
/// # Safety
/// `id` valid reactor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_reactor_poll_ms(id: ReactorId, timeout_ms: i64) -> i64 {
    if id < 0 {
        return -1;
    }

    #[cfg(target_os = "linux")]
    {
        let (epfd, tfd_opt, deadline) = {
            let guard = lock_reactors();
            let Some(slot) = guard.get(id as usize).and_then(|s| s.as_ref()) else {
                return -1;
            };
            (slot.epoll_fd, slot.timer_fd, slot.deadline)
        };

        // If no timerfd but we have a deadline (shouldn't happen on linux path), sleep.
        let Some(tfd) = tfd_opt else {
            if let Some(dl) = deadline {
                let now = Instant::now();
                if now >= dl {
                    let mut guard = lock_reactors();
                    if let Some(Some(slot)) = guard.get_mut(id as usize) {
                        slot.deadline = None;
                    }
                    return 1;
                }
                let remaining = dl.saturating_duration_since(now);
                let wait_ms = if timeout_ms < 0 {
                    remaining.as_millis() as i64
                } else {
                    remaining.as_millis().min(timeout_ms as u128) as i64
                };
                if wait_ms > 0 {
                    std::thread::sleep(Duration::from_millis(wait_ms as u64));
                }
                let mut guard = lock_reactors();
                if let Some(Some(slot)) = guard.get_mut(id as usize) {
                    if slot.deadline.is_some_and(|d| Instant::now() >= d) {
                        slot.deadline = None;
                        return 1;
                    }
                }
                return 0;
            }
            if timeout_ms > 0 {
                std::thread::sleep(Duration::from_millis(timeout_ms as u64));
            }
            return 0;
        };

        let mut events: [libc::epoll_event; 4] = unsafe { std::mem::zeroed() };
        let n = unsafe {
            libc::epoll_wait(
                epfd,
                events.as_mut_ptr(),
                events.len() as i32,
                timeout_ms as i32,
            )
        };
        if n < 0 {
            return -1;
        }
        if n == 0 {
            return 0;
        }
        // Drain timerfd
        let mut buf = [0u8; 8];
        let _ = unsafe { libc::read(tfd, buf.as_mut_ptr() as *mut _, buf.len()) };
        let mut guard = lock_reactors();
        if let Some(Some(slot)) = guard.get_mut(id as usize) {
            if let Some(old) = slot.timer_fd.take() {
                unsafe {
                    let _ = libc::epoll_ctl(
                        slot.epoll_fd,
                        libc::EPOLL_CTL_DEL,
                        old,
                        std::ptr::null_mut(),
                    );
                    let _ = libc::close(old);
                }
            }
            slot.deadline = None;
        }
        1
    }

    #[cfg(not(target_os = "linux"))]
    {
        let deadline = {
            let guard = lock_reactors();
            let Some(slot) = guard.get(id as usize).and_then(|s| s.as_ref()) else {
                return -1;
            };
            slot.deadline
        };
        let Some(dl) = deadline else {
            if timeout_ms > 0 {
                std::thread::sleep(Duration::from_millis(timeout_ms as u64));
            }
            return 0;
        };
        let now = Instant::now();
        if now >= dl {
            let mut guard = lock_reactors();
            if let Some(Some(slot)) = guard.get_mut(id as usize) {
                slot.deadline = None;
            }
            return 1;
        }
        let remaining = dl.saturating_duration_since(now);
        let wait = if timeout_ms < 0 {
            remaining
        } else {
            remaining.min(Duration::from_millis(timeout_ms as u64))
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
        let mut guard = lock_reactors();
        if let Some(Some(slot)) = guard.get_mut(id as usize) {
            if slot.deadline.is_some_and(|d| Instant::now() >= d) {
                slot.deadline = None;
                return 1;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_sleep_destroy() {
        unsafe {
            let r = ar_rt_reactor_create();
            assert!(r >= 0);
            let t0 = Instant::now();
            assert_eq!(ar_rt_reactor_sleep_ms(r, 5), 0);
            assert!(t0.elapsed() >= Duration::from_millis(3));
            ar_rt_reactor_destroy(r);
        }
    }

    #[test]
    fn arm_and_poll() {
        unsafe {
            let r = ar_rt_reactor_create();
            assert_eq!(ar_rt_reactor_arm_timer_ms(r, 5), 0);
            // Wait long enough for the timer.
            let rc = ar_rt_reactor_poll_ms(r, 200);
            assert_eq!(rc, 1);
            ar_rt_reactor_destroy(r);
        }
    }
}
