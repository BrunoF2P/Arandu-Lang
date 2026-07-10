#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;
use std::path::{Path, PathBuf};

use arandu_lexer::lex;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should be under workspace/crates")
        .to_path_buf()
}

fn collect_aru_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to read {}: {err}", dir.display()))
    {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_aru_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "aru") {
            files.push(path);
        }
    }
}

#[test]
fn lexes_required_example_corpus() {
    let root = workspace_root();
    let mut files = Vec::new();
    for dir in [
        root.join("examples").join("stable").join("syntax"),
        root.join("examples").join("stable").join("semantics"),
        root.join("examples").join("stable").join("interop"),
        root.join("examples").join("invalid").join("syntax"),
        root.join("examples").join("invalid").join("semantics"),
    ] {
        collect_aru_files(&dir, &mut files);
    }
    files.sort();

    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        lex(&source).unwrap_or_else(|err| panic!("failed to lex {}: {err}", path.display()));
    }
}
