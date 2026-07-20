//! Bare Ok/Some patterns in match arms.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use arandu_parser::syntax::{lower_syntax_to_program_rd_only, parse_syntax};
use arandu_parser::parse;

const SRC_CALL: &str = r#"
module t
func f(): Option<int> {
    return Option.Some(3)
}
func main(): int {
    match f() {
        Some(x) => { return x }
        None => { return 0 }
    }
}
"#;

const SRC_LOCAL: &str = r#"
module t
func main(): int {
    let g0: Option<int> = Option.Some(3)
    match g0 {
        Some(x) => { return x }
        None => { return 0 }
    }
}
"#;

#[test]
fn bare_some_match_on_local_parses() {
    parse(SRC_LOCAL).expect("local scrutinee");
}

#[test]
fn bare_some_match_on_call_parses() {
    parse(SRC_CALL).expect("call scrutinee");
}

#[test]
fn bare_some_match_on_call_rd_only() {
    let tree = parse_syntax(SRC_CALL);
    lower_syntax_to_program_rd_only(&tree, 0).expect("RD-only call scrutinee");
}

#[test]
fn bare_ok_result_match_parses() {
    let src = r#"
module t
enum E { A }
func f(): Result<int, E> {
    return Result.Ok(7)
}
func main(): int {
    match f() {
        Ok(x) => { return x }
        Err(_) => { return 9 }
    }
}
"#;
    parse(src).expect("Result Ok/Err bare");
}
