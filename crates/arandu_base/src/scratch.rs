use std::cell::RefCell;

/// A thread-local buffer reuse utility to avoid memory allocation churn.
pub struct ScratchFormatter {
    buf: String,
}

thread_local! {
    static SCRATCH: RefCell<ScratchFormatter> = RefCell::new(ScratchFormatter {
        buf: String::with_capacity(256)
    });
}

/// Executes a closure passing a mutable reference to a thread-local reused scratch String buffer.
/// Safe against reentrant borrow panics by falling back to a fresh allocation.
pub fn with_scratch<F: FnOnce(&mut String) -> R, R>(f: F) -> R {
    SCRATCH.with(|s| match s.try_borrow_mut() {
        Ok(mut scratch) => {
            scratch.buf.clear();
            f(&mut scratch.buf)
        }
        Err(_) => f(&mut String::with_capacity(256)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratch_formatter_basic() {
        let result = with_scratch(|buf| {
            buf.push_str("hello");
            buf.push_str(" world");
            buf.clone()
        });
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_scratch_formatter_reentrancy() {
        let nested = with_scratch(|outer| {
            outer.push_str("outer");
            let inner_val = with_scratch(|inner| {
                inner.push_str("inner");
                inner.clone()
            });
            format!("{outer}-{inner_val}")
        });
        assert_eq!(nested, "outer-inner");
    }
}
