use arandu_web::compile_source;

#[test]
fn playground_compiles_layout_roots_and_generic_operand_types_for_wasm32() {
    let output = compile_source(
        "func size<T>(): usize { return @sizeOf(T) }\nfunc main(): int { let word = comptime (@sizeOf(usize)); return (word + @alignOf(i64) + size<[3]u16>()) as int }",
    );
    assert!(output.success, "{:?}", output.diagnostics);
    assert!(output.wasm_bytes.is_some());
}

#[test]
fn playground_keeps_invalid_layout_as_a_structured_user_diagnostic() {
    let output = compile_source(
        "func main(): usize { return comptime (@sizeOf([18446744073709551615]u64)) }",
    );
    assert!(!output.success);
    assert!(output.wasm_bytes.is_none());
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("T047"))
    );
    assert!(!output.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("ICE"))
    }));
}
