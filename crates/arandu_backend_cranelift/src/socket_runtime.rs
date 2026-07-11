//! SL_R — host TCP socket helpers (blocking I/O MVP + reactor-ready fds).
//!
//! Full async socket state machines need Waker registration (SL_R.2/3).
//! These hosts provide the OS surface std.runtime can wrap.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

struct SockSlot {
    kind: SockKind,
}

enum SockKind {
    Listener(TcpListener),
    Stream(TcpStream),
}

static SOCKS: Mutex<Vec<Option<SockSlot>>> = Mutex::new(Vec::new());

fn lock() -> std::sync::MutexGuard<'static, Vec<Option<SockSlot>>> {
    SOCKS.lock().unwrap_or_else(|e| e.into_inner())
}

fn insert(kind: SockKind) -> i64 {
    let mut g = lock();
    let slot = SockSlot { kind };
    if let Some(idx) = g.iter().position(|s| s.is_none()) {
        g[idx] = Some(slot);
        return idx as i64;
    }
    let id = g.len() as i64;
    g.push(Some(slot));
    id
}

/// Listen on `127.0.0.1:port`. Returns handle >= 0 or -1.
///
/// # Safety
/// C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_tcp_listen(port: i64) -> i64 {
    if !(0..=65535).contains(&port) {
        return -1;
    }
    let addr = format!("127.0.0.1:{port}");
    match TcpListener::bind(&addr) {
        Ok(l) => {
            let _ = l.set_nonblocking(false);
            insert(SockKind::Listener(l))
        }
        Err(_) => -1,
    }
}

/// Accept one connection. Returns stream handle >= 0 or -1.
///
/// # Safety
/// `listener` from listen.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_tcp_accept(listener: i64) -> i64 {
    if listener < 0 {
        return -1;
    }
    let mut g = lock();
    let Some(Some(slot)) = g.get_mut(listener as usize) else {
        return -1;
    };
    let SockKind::Listener(l) = &slot.kind else {
        return -1;
    };
    match l.accept() {
        Ok((stream, _)) => {
            drop(g);
            insert(SockKind::Stream(stream))
        }
        Err(_) => -1,
    }
}

/// Connect to `127.0.0.1:port`. Returns stream handle or -1.
///
/// # Safety
/// C ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_tcp_connect(port: i64) -> i64 {
    if !(0..=65535).contains(&port) {
        return -1;
    }
    let addr = format!("127.0.0.1:{port}");
    match TcpStream::connect(&addr) {
        Ok(s) => {
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
            insert(SockKind::Stream(s))
        }
        Err(_) => -1,
    }
}

/// Read up to `len` bytes into `buf`. Returns bytes read, 0 EOF, -1 error.
///
/// # Safety
/// `buf` valid for `len`; `sock` stream handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_tcp_read(sock: i64, buf: *mut u8, len: i64) -> i64 {
    if sock < 0 || buf.is_null() || len <= 0 {
        return -1;
    }
    let mut g = lock();
    let Some(Some(slot)) = g.get_mut(sock as usize) else {
        return -1;
    };
    let SockKind::Stream(s) = &mut slot.kind else {
        return -1;
    };
    let slice = unsafe { std::slice::from_raw_parts_mut(buf, len as usize) };
    match s.read(slice) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// Write `len` bytes from `buf`. Returns bytes written or -1.
///
/// # Safety
/// Fat buffer ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_tcp_write(sock: i64, buf: *const u8, len: i64) -> i64 {
    if sock < 0 || buf.is_null() || len < 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let mut g = lock();
    let Some(Some(slot)) = g.get_mut(sock as usize) else {
        return -1;
    };
    let SockKind::Stream(s) = &mut slot.kind else {
        return -1;
    };
    let slice = unsafe { std::slice::from_raw_parts(buf, len as usize) };
    match s.write(slice) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// Close socket handle.
///
/// # Safety
/// Handle from listen/accept/connect.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ar_rt_tcp_close(sock: i64) {
    if sock < 0 {
        return;
    }
    let mut g = lock();
    if let Some(slot) = g.get_mut(sock as usize) {
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listen_connect_write_read() {
        unsafe {
            let lis = ar_rt_tcp_listen(0); // port 0 may fail on some binds — use high port
            // Prefer ephemeral: bind 0 doesn't work with our API; pick free port via OS.
            // Use a fixed high port for the unit test.
            let port = 18765i64;
            ar_rt_tcp_close(lis);
            let lis = ar_rt_tcp_listen(port);
            if lis < 0 {
                // Port busy — skip soft
                return;
            }
            let client = ar_rt_tcp_connect(port);
            assert!(client >= 0);
            let server = ar_rt_tcp_accept(lis);
            assert!(server >= 0);
            let msg = b"hi";
            assert_eq!(ar_rt_tcp_write(client, msg.as_ptr(), 2), 2);
            let mut buf = [0u8; 8];
            assert_eq!(ar_rt_tcp_read(server, buf.as_mut_ptr(), 8), 2);
            assert_eq!(&buf[..2], b"hi");
            ar_rt_tcp_close(client);
            ar_rt_tcp_close(server);
            ar_rt_tcp_close(lis);
        }
    }
}
