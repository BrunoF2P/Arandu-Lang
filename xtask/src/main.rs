//! Arandu workspace automation (xtask pattern).
//!
//! ```text
//! cargo run -p xtask -- check-diag-docs
//! cargo run -p xtask -- help
//! ```

use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    let mut args = env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "help".into());
    let code = match cmd.as_str() {
        "check-diag-docs" => cmd_check_diag_docs(),
        "help" | "-h" | "--help" => {
            print_help();
            0
        }
        other => {
            eprintln!("unknown xtask command: {other}");
            print_help();
            2
        }
    };
    process::exit(code);
}

fn print_help() {
    eprintln!(
        "\
xtask — Arandu workspace tasks

Commands:
  check-diag-docs   Bijection: DiagCode (user-facing) ↔ docs/errors/*.md
  help              This message

Examples:
  cargo run -p xtask -- check-diag-docs
  ./scripts/check-diag-docs.sh
"
    );
}

/// Source of truth = `DiagCode` enum; docs must match exactly (no manual code list).
fn cmd_check_diag_docs() -> i32 {
    let root = workspace_root();
    let docs_dir = root.join("docs/errors");
    let (missing, orphaned) = arandu_diagnostics::diag_doc_diff(&docs_dir);

    if missing.is_empty() && orphaned.is_empty() {
        let n = arandu_diagnostics::DiagCode::ALL
            .iter()
            .filter(|c| c.requires_error_doc())
            .count();
        println!("check-diag-docs: ok ({n} user-facing DiagCode(s) ↔ docs/errors)");
        return 0;
    }

    if !missing.is_empty() {
        eprintln!("error: missing docs/errors/{{CODE}}.md for DiagCode variants:");
        for code in &missing {
            eprintln!("  - docs/errors/{code}.md   (add doc when declaring DiagCode)");
        }
    }
    if !orphaned.is_empty() {
        eprintln!("error: orphaned docs (no matching DiagCode / not user-facing):");
        for doc in &orphaned {
            eprintln!("  - docs/errors/{doc}.md   (remove or rename after code change)");
        }
    }
    eprintln!();
    eprintln!("DiagCode is the single source of truth — do not maintain a parallel list.");
    1
}

fn workspace_root() -> PathBuf {
    // xtask lives at $ROOT/xtask — walk up from CARGO_MANIFEST_DIR.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("xtask parent = workspace root")
        .to_path_buf()
}
