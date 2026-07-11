//! Helpers for `lsp_types::Uri` (lsp-types ≥ 0.97 replaced `url::Url`).
//!
//! The newtype no longer exposes `to_file_path` / `from_file_path`; we convert
//! via the `file://` string form with minimal percent-encoding.

use lsp_types::Uri;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Parse a URI string (`file:///…` or generic).
#[must_use]
pub fn parse_uri(s: &str) -> Option<Uri> {
    Uri::from_str(s).ok()
}

/// Convert a filesystem path to an LSP `file://` URI.
#[must_use]
pub fn uri_from_path(path: &Path) -> Option<Uri> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let s = abs.to_str()?;
    let mut out = String::from("file://");
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'.'
            | b'_'
            | b'-'
            | b'~' => out.push(b as char),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    Uri::from_str(&out).ok()
}

/// Convert an LSP URI to a filesystem path (best-effort for `file://`).
#[must_use]
pub fn path_from_uri(uri: &Uri) -> PathBuf {
    let s = uri.as_str();
    if let Some(rest) = s.strip_prefix("file://") {
        let rest = rest
            .strip_prefix("localhost")
            .unwrap_or(rest);
        PathBuf::from(percent_decode(rest))
    } else if let Some(rest) = s.strip_prefix("file:") {
        PathBuf::from(percent_decode(rest))
    } else {
        PathBuf::from(s)
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_unix_path() {
        let p = PathBuf::from("/tmp/hello world.aru");
        let uri = uri_from_path(&p).expect("uri");
        assert!(uri.as_str().starts_with("file://"));
        assert!(uri.as_str().contains("%20") || uri.as_str().contains("hello"));
        let back = path_from_uri(&uri);
        assert_eq!(back, p);
    }

    #[test]
    fn parse_file_uri() {
        let u = parse_uri("file:///home/user/a.aru").expect("parse");
        assert_eq!(path_from_uri(&u), PathBuf::from("/home/user/a.aru"));
    }
}
