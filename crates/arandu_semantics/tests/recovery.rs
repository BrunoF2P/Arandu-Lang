use arandu_lexer::lex_recovering;
use arandu_parser::parse_recovering;
use arandu_semantics::resolve_for_test;
use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should be under workspace/crates")
        .to_path_buf()
}

#[test]
fn test_recovery_lexer() {
    let root = workspace_root();
    let lexer_dir = root.join("tests").join("recovery").join("lexer");

    // Case 1: invalid_char.aru
    let src_char = fs::read_to_string(lexer_dir.join("invalid_char.aru")).unwrap();
    let lexed_char = lex_recovering(&src_char);
    assert!(
        !lexed_char.diagnostics.is_empty(),
        "invalid_char should have diagnostics"
    );
    // Verify we still got tokens (meaning it recovered and continued tokenizing)
    assert!(
        !lexed_char.tokens.is_empty(),
        "should recover and produce tokens"
    );

    // Case 2: unterminated_string.aru
    let src_str = fs::read_to_string(lexer_dir.join("unterminated_string.aru")).unwrap();
    let lexed_str = lex_recovering(&src_str);
    assert!(
        !lexed_str.diagnostics.is_empty(),
        "unterminated_string should have diagnostics"
    );
    assert!(
        !lexed_str.tokens.is_empty(),
        "should recover and produce tokens"
    );

    // Case 3: invalid_escape.aru
    let src_esc = fs::read_to_string(lexer_dir.join("invalid_escape.aru")).unwrap();
    let lexed_esc = lex_recovering(&src_esc);
    assert!(
        !lexed_esc.diagnostics.is_empty(),
        "invalid_escape should have diagnostics"
    );
    assert!(
        !lexed_esc.tokens.is_empty(),
        "should recover and produce tokens"
    );
}

#[test]
fn test_recovery_parser() {
    let root = workspace_root();
    let parser_dir = root.join("tests").join("recovery").join("parser");

    // Case 1: missing_rparen.aru
    let src_rparen = fs::read_to_string(parser_dir.join("missing_rparen.aru")).unwrap();
    let output_rparen = parse_recovering(&src_rparen);
    assert!(
        !output_rparen.diagnostics.is_empty(),
        "missing_rparen should have diagnostics"
    );
    // Should have parsed some declarations / function
    assert!(
        !output_rparen.program.decls.is_empty(),
        "should have parsed declarations"
    );

    // Case 2: missing_rbrace.aru
    let src_rbrace = fs::read_to_string(parser_dir.join("missing_rbrace.aru")).unwrap();
    let output_rbrace = parse_recovering(&src_rbrace);
    assert!(
        !output_rbrace.diagnostics.is_empty(),
        "missing_rbrace should have diagnostics"
    );
    assert!(
        !output_rbrace.program.decls.is_empty(),
        "should have parsed declarations"
    );

    // Case 3: invalid_statement.aru
    let src_stmt = fs::read_to_string(parser_dir.join("invalid_statement.aru")).unwrap();
    let output_stmt = parse_recovering(&src_stmt);
    assert!(
        !output_stmt.diagnostics.is_empty(),
        "invalid_statement should have diagnostics"
    );
    assert!(
        !output_stmt.program.decls.is_empty(),
        "should have parsed declarations"
    );

    // Case 4: invalid_expr.aru
    let src_expr = fs::read_to_string(parser_dir.join("invalid_expr.aru")).unwrap();
    let output_expr = parse_recovering(&src_expr);
    assert!(
        !output_expr.diagnostics.is_empty(),
        "invalid_expr should have diagnostics"
    );
    assert!(
        !output_expr.program.decls.is_empty(),
        "should have parsed declarations"
    );
}

#[test]
fn test_recovery_name_resolution() {
    let root = workspace_root();
    let nr_dir = root.join("tests").join("recovery").join("name_resolution");

    // Case 1: undefined_value_suggestion.aru
    let src_val = fs::read_to_string(nr_dir.join("undefined_value_suggestion.aru")).unwrap();
    let output_val = parse_recovering(&src_val);
    let res_val = resolve_for_test(0, &output_val.program);
    assert!(
        !res_val.diagnostics.is_empty(),
        "undefined value should have diagnostics"
    );
    let codes: Vec<_> = res_val.diagnostics.iter().map(|d| d.code).collect();
    assert!(
        codes.contains(&arandu_semantics::DiagCode::N001UndefinedValue),
        "should report undefined value"
    );
    // Verify Levenshtein suggestion hint
    let diag = res_val
        .diagnostics
        .iter()
        .find(|d| d.code == arandu_semantics::DiagCode::N001UndefinedValue)
        .unwrap();
    assert!(
        diag.hints.iter().any(|h| h.message.contains("my_variable")),
        "should suggest 'my_variable' in hints, got: {:?}",
        diag.hints
    );

    // Case 2: undefined_type_suggestion.aru
    let src_ty = fs::read_to_string(nr_dir.join("undefined_type_suggestion.aru")).unwrap();
    let output_ty = parse_recovering(&src_ty);
    let res_ty = resolve_for_test(0, &output_ty.program);
    assert!(
        !res_ty.diagnostics.is_empty(),
        "undefined type should have diagnostics"
    );
    let codes_ty: Vec<_> = res_ty.diagnostics.iter().map(|d| d.code).collect();
    assert!(
        codes_ty.contains(&arandu_semantics::DiagCode::N002UndefinedType),
        "should report undefined type"
    );
    let diag_ty = res_ty
        .diagnostics
        .iter()
        .find(|d| d.code == arandu_semantics::DiagCode::N002UndefinedType)
        .unwrap();
    assert!(
        diag_ty.hints.iter().any(|h| h.message.contains("Person")),
        "should suggest 'Person' in hints, got: {:?}",
        diag_ty.hints
    );
}
