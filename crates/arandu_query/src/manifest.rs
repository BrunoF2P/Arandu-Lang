//! Project manifest (`Arandu.toml`) — Salsa input from day 1.
//!
//! Gold bar (P2): the manifest is a `#[salsa::input]` whose **content hash**
//! participates in the invalidation key. Changing `entry` / `name` / `version`
//! (or any future field) must not leave a stale cache; registering the input
//! now avoids a painful migration when deps/workspace land.
//!
//! Parse errors are **never swallowed** (BUG-09 discipline): a malformed
//! `Arandu.toml` is a hard error with path + reason.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Canonical on-disk filename for a project package.
pub const MANIFEST_FILENAME: &str = "Arandu.toml";

/// Parsed package fields (MVP: name / version / entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestData {
    pub name: String,
    pub version: String,
    pub entry: String,
}

/// Why reading or parsing `Arandu.toml` failed.
#[derive(Debug, Clone)]
pub enum ManifestError {
    Io {
        path: PathBuf,
        message: String,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    MissingField {
        path: PathBuf,
        field: &'static str,
    },
    /// Package `name` collides with a reserved stdlib/language root (PROMOTE-L2).
    ReservedName {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::Io { path, message } => {
                write!(f, "failed to read {}: {message}", path.display())
            }
            ManifestError::Parse { path, message } => {
                write!(f, "malformed {}: {message}", path.display())
            }
            ManifestError::MissingField { path, field } => {
                write!(
                    f,
                    "malformed {}: missing required field `{field}`",
                    path.display()
                )
            }
            ManifestError::ReservedName { path, message } => {
                write!(f, "invalid {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ManifestError {}

/// Salsa input for the project manifest.
///
/// `content_hash` is the BLAKE3 of the raw file bytes (hex). Any change to the
/// file — including whitespace or comments — updates the hash and invalidates
/// dependents. Field values are also inputs so queries can depend on `entry`
/// without re-parsing.
#[salsa::input]
pub struct ProjectManifest {
    #[returns(ref)]
    pub name: String,
    #[returns(ref)]
    pub version: String,
    #[returns(ref)]
    pub entry: String,
    /// BLAKE3-256 of raw `Arandu.toml` bytes, lowercase hex (64 chars).
    #[returns(ref)]
    pub content_hash: String,
    pub path: Arc<PathBuf>,
}

/// BLAKE3 hex of `bytes` (stable invalidation fingerprint).
#[must_use]
pub fn hash_manifest_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Parse `Arandu.toml` text. Does **not** read the filesystem.
///
/// Minimal TOML subset: top-level `key = "value"` string assignments only.
/// Unknown keys are ignored (forward-compatible). Missing required fields fail.
pub fn parse_manifest_str(path: &Path, text: &str) -> Result<ManifestData, ManifestError> {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut entry: Option<String> = None;

    for (line_no, raw_line) in text.lines().enumerate() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        // Sections / tables not supported in MVP — reject so we never silently
        // ignore structured config the user thought was active.
        if line.starts_with('[') {
            return Err(ManifestError::Parse {
                path: path.to_path_buf(),
                message: format!(
                    "line {}: tables/sections are not supported yet (got `{line}`)",
                    line_no + 1
                ),
            });
        }
        let Some((key, rest)) = line.split_once('=') else {
            return Err(ManifestError::Parse {
                path: path.to_path_buf(),
                message: format!(
                    "line {}: expected `key = \"value\"`, got `{line}`",
                    line_no + 1
                ),
            });
        };
        let key = key.trim();
        let value = match parse_toml_string(rest.trim()) {
            Ok(v) => v,
            Err(msg) => {
                return Err(ManifestError::Parse {
                    path: path.to_path_buf(),
                    message: format!("line {}: {msg}", line_no + 1),
                });
            }
        };
        match key {
            "name" => name = Some(value),
            "version" => version = Some(value),
            "entry" => entry = Some(value),
            // Forward-compatible: ignore unknown keys for now.
            _ => {}
        }
    }

    let path_buf = path.to_path_buf();
    let name = name.ok_or(ManifestError::MissingField {
        path: path_buf.clone(),
        field: "name",
    })?;
    let version = version.ok_or(ManifestError::MissingField {
        path: path_buf.clone(),
        field: "version",
    })?;
    let entry = entry.ok_or(ManifestError::MissingField {
        path: path_buf,
        field: "entry",
    })?;

    if name.is_empty() {
        return Err(ManifestError::Parse {
            path: path.to_path_buf(),
            message: "`name` must be non-empty".into(),
        });
    }
    if entry.is_empty() {
        return Err(ManifestError::Parse {
            path: path.to_path_buf(),
            message: "`entry` must be non-empty".into(),
        });
    }
    // PROMOTE-L2: package name must not collide with stdlib roots.
    if let Err(e) = crate::vfs::validate_package_name(&name) {
        return Err(ManifestError::ReservedName {
            path: path.to_path_buf(),
            message: e.to_string(),
        });
    }

    Ok(ManifestData {
        name,
        version,
        entry,
    })
}

/// Read and parse `Arandu.toml` at `path`. Propagates I/O and parse errors.
pub fn load_manifest(path: &Path) -> Result<(ManifestData, String, Vec<u8>), ManifestError> {
    let bytes = std::fs::read(path).map_err(|e| ManifestError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let text = String::from_utf8(bytes.clone()).map_err(|e| ManifestError::Parse {
        path: path.to_path_buf(),
        message: format!("file is not valid UTF-8: {e}"),
    })?;
    let data = parse_manifest_str(path, &text)?;
    let hash = hash_manifest_bytes(&bytes);
    Ok((data, hash, bytes))
}

/// Register a loaded manifest as a Salsa input on `db`.
pub fn register_manifest(
    db: &dyn salsa::Database,
    path: PathBuf,
    data: ManifestData,
    content_hash: String,
) -> ProjectManifest {
    ProjectManifest::new(
        db,
        data.name,
        data.version,
        data.entry,
        content_hash,
        Arc::new(path),
    )
}

/// Walk parents of `start` looking for `Arandu.toml` (project discovery).
///
/// This is **not** stdlib resolution — package roots may use cwd/path walk
/// (Cargo convention). Stdlib uses [`crate::stdlib::resolve_stdlib_root`].
pub fn find_manifest(start: &Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    // Normalize to absolute when possible so relative starts still walk.
    if let Ok(abs) = std::fs::canonicalize(&current) {
        current = abs;
    }
    loop {
        let candidate = current.join(MANIFEST_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn strip_toml_comment(line: &str) -> &str {
    // Naive: `#` starts a comment unless inside quotes. Good enough for MVP
    // keys which are simple strings without embedded `#`.
    let mut in_string = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if in_string => {
                // Handle escaped quote.
                if i > 0 && bytes[i - 1] == b'\\' {
                    i += 1;
                    continue;
                }
                in_string = false;
            }
            b'"' => in_string = true,
            b'#' if !in_string => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

fn parse_toml_string(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return Err(format!("expected double-quoted string, got `{s}`"));
    }
    let inner = &s[1..s.len() - 1];
    // Minimal escapes: \\ \" \n \t
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    return Err(format!("unknown escape `\\{other}` in string"));
                }
                None => return Err("trailing backslash in string".into()),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Tracked helper so dependents can pin work to the manifest fingerprint.
///
/// Exists primarily so the Salsa graph records the input edge from day 1
/// (even while the CLI still drives entry selection).
#[salsa::tracked]
pub fn manifest_fingerprint(db: &dyn crate::db::ArandCompilerDb, m: ProjectManifest) -> String {
    // Include fields + hash so any change shows up in explain-rebuild keys.
    format!(
        "{}@{}:{}#{}",
        m.name(db),
        m.version(db),
        m.entry(db),
        m.content_hash(db)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_happy_path() {
        let text = r#"
# comment
name = "hello"
version = "0.0.1"
entry = "src/main.aru"
"#;
        let data = parse_manifest_str(Path::new("Arandu.toml"), text).unwrap();
        assert_eq!(data.name, "hello");
        assert_eq!(data.version, "0.0.1");
        assert_eq!(data.entry, "src/main.aru");
    }

    #[test]
    fn parse_missing_entry_errors() {
        let text = r#"name = "x"
version = "1.0.0"
"#;
        let err = parse_manifest_str(Path::new("Arandu.toml"), text).unwrap_err();
        assert!(matches!(
            err,
            ManifestError::MissingField { field: "entry", .. }
        ));
    }

    #[test]
    fn parse_malformed_line_errors() {
        let text = "name = hello\n";
        let err = parse_manifest_str(Path::new("Arandu.toml"), text).unwrap_err();
        assert!(matches!(err, ManifestError::Parse { .. }));
    }

    #[test]
    fn parse_rejects_tables() {
        let text = r#"
name = "x"
version = "0.0.1"
entry = "src/main.aru"
[dependencies]
"#;
        let err = parse_manifest_str(Path::new("Arandu.toml"), text).unwrap_err();
        assert!(matches!(err, ManifestError::Parse { .. }));
    }

    #[test]
    fn content_hash_stable() {
        let a = hash_manifest_bytes(b"name = \"a\"\n");
        let b = hash_manifest_bytes(b"name = \"a\"\n");
        let c = hash_manifest_bytes(b"name = \"b\"\n");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }
}
