use std::fs;
use std::path::PathBuf;

use arandu_lexer::lex_to_string;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should be under workspace/crates")
        .to_path_buf()
}

fn assert_contract(name: &str) {
    let root = workspace_root();
    let source_path = root
        .join("tests")
        .join("lexer_contract")
        .join(format!("{name}.aru"));
    let expected_path = root
        .join("tests")
        .join("lexer_contract")
        .join(format!("{name}.tokens"));

    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", source_path.display()));
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", expected_path.display()));

    let actual = lex_to_string(&source).expect("lexer should succeed");
    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn doc_comments() {
    assert_contract("doc_comments");
}

#[test]
fn semicolon_before_rbrace() {
    assert_contract("semicolon_before_rbrace");
}

#[test]
fn semicolon_before_else() {
    assert_contract("semicolon_before_else");
}
