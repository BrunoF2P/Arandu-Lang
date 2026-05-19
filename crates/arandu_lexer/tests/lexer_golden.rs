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

fn assert_golden(name: &str) {
    let root = workspace_root();
    let source_path = root.join("tests").join("lexer").join(format!("{name}.aru"));
    let expected_path = root
        .join("tests")
        .join("lexer")
        .join(format!("{name}.tokens"));

    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", source_path.display()));
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", expected_path.display()));

    let actual = lex_to_string(&source).expect("lexer should succeed");
    assert_eq!(actual.trim_end(), expected.trim_end());
}

#[test]
fn lexes_hello_fixture() {
    assert_golden("hello");
}

#[test]
fn lexes_strings_fixture() {
    assert_golden("strings");
}

#[test]
fn lexes_semicolon_fixture() {
    assert_golden("semicolon");
}

#[test]
fn lexes_numbers_fixture() {
    assert_golden("numbers");
}

#[test]
fn lexes_comments_fixture() {
    assert_golden("comments");
}

#[test]
fn lexes_if_else_fixture() {
    assert_golden("if_else");
}

#[test]
fn lexes_nested_interpolation_fixture() {
    assert_golden("interpolation_nested");
}
