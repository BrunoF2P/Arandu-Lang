#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Guardrails: Salsa boundary purity (no ad-hoc source I/O in analysis crates).

use std::fs;
use std::path::PathBuf;

fn crate_src(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name)
        .join("src")
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

fn assert_no_source_fs_reads(crate_name: &str) {
    let mut files = Vec::new();
    collect_rs(&crate_src(crate_name), &mut files);
    assert!(
        !files.is_empty(),
        "expected .rs files under {crate_name}/src"
    );

    let banned = [
        "std::fs::read_to_string",
        "std::fs::read(",
        "fs::read_to_string",
        "tokio::fs::read",
    ];
    let mut offenders = Vec::new();
    for path in &files {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            for b in banned {
                if line.contains(b) {
                    offenders.push(format!("{}:{}: {}", path.display(), i + 1, trimmed));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "source I/O must stay in arandu_query::DatabaseImpl (SourceFile / resolve_module_path).\n\
         Offenders in {crate_name}:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn typeck_does_not_read_source_files_directly() {
    assert_no_source_fs_reads("arandu_typeck");
}

#[test]
fn resolve_does_not_read_source_files_directly() {
    assert_no_source_fs_reads("arandu_resolve");
}

#[test]
fn compile_session_type_is_gone_from_pipeline_crates() {
    // After removal, no production crate may reference CompileSession.
    let roots = [
        "arandu_query",
        "arandu_typeck",
        "arandu_resolve",
        "arandu_mir",
        "arandu_cli",
        "arandu_lsp",
        "arandu_middle",
        "arandu_semantics",
    ];
    let mut hits = Vec::new();
    for name in roots {
        let mut files = Vec::new();
        collect_rs(&crate_src(name), &mut files);
        for path in files {
            // This test file itself mentions the name.
            if path.ends_with("architecture_invariants.rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("//") || t.starts_with("///") || t.starts_with("//!") {
                    continue;
                }
                if line.contains("CompileSession") {
                    hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "CompileSession was removed; unexpected references:\n{}",
        hits.join("\n")
    );
}
