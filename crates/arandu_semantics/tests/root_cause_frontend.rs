//! Regression tests for root-cause frontend fixes (RC-SET, RC-GUARD, RC-NEST,
//! RC-F64, RC-ERR-NIL). Each case previously required example workarounds.

use arandu_semantics::{lower_to_hir, resolve_for_test, type_check};

fn check_ok(src: &str) {
    let program = arandu_parser::parse(src).expect("parse");
    let resolution = resolve_for_test(0, &program);
    let mut tc = type_check(resolution, &program);
    assert!(
        tc.diagnostics
            .iter()
            .all(|d| d.severity != arandu_middle::Severity::Error),
        "typeck errors: {:?}",
        tc.diagnostics
    );
    let hir = lower_to_hir(&mut tc, &program).expect("lower");
    hir.validate_invariants(&hir.pool, &tc.symbols)
        .expect("hir invariants");
}

#[test]
fn rc_set_keyword_assignment() {
    check_ok(
        r#"
func main() {
    let mut x: int = 0
    set x = 1
    set x = x + 2
}
"#,
    );
}

#[test]
fn rc_match_guard() {
    check_ok(
        r#"
enum Token {
    Number(int),
    Word(str),
    End,
}
func describe(token: Token): str {
    return match token {
        Token.Number(value) if value > 0 => "positive number"
        Token.Number(_) => "number"
        Token.Word(text) => "word ${text}"
        Token.End => "end"
    }
}
"#,
    );
}

#[test]
fn rc_nested_method_into_namespace_call() {
    check_ok(
        r#"
import io
struct User {
    name: str
}
func User.greet(shared self): str {
    return self.name
}
func main() {
    let user = User { name: "Ana" }
    io.println(user.greet())
}
"#,
    );
}

#[test]
fn rc_extern_f64_in_unsafe_stmt() {
    check_ok(
        r#"
extern "C" {
    func cos(value: f64): f64
    func sin(value: f64): f64
}
func length(x: f64, y: f64): f64 {
    unsafe {
        let a: f64 = cos(x)
        let b: f64 = sin(y)
        return a + b
    }
}
"#,
    );
}

#[test]
fn rc_result_err_nil_compare() {
    check_ok(
        r#"
import err
import io
func readName(path: str): Result<str, Err> {
    if path == "" {
        return Result.Err(err.new("missing"))
    }
    return Result.Ok("ok")
}
func main() {
    let name, e = readName("p")
    if e != nil {
        io.println("error")
        return
    }
    io.println(name)
}
"#,
    );
}

#[test]
fn rc_interp_rejects_non_str() {
    let src = r#"
func main() {
    let n: int = 1
    let s = "n=${n}"
}
"#;
    let program = arandu_parser::parse(src).expect("parse");
    let resolution = resolve_for_test(0, &program);
    let tc = type_check(resolution, &program);
    assert!(
        tc.diagnostics.iter().any(|d| {
            d.message.contains("string interpolation requires `str`")
        }),
        "expected str-only interp diagnostic, got {:?}",
        tc.diagnostics
    );
}
