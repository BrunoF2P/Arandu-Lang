use crate::ParseErrorCode;
use crate::{parse, parse_recovering, parse_to_string};

fn strip_spans(s: &str) -> String {
    // Remove @line:col-line:col annotations for easier assertion
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' {
            // skip @digits:digits-digits:digits
            while let Some(&n) = chars.peek() {
                if n.is_ascii_digit() || n == ':' || n == '-' {
                    chars.next();
                } else {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn parse_empty_program() {
    let result = parse_to_string("").unwrap();
    assert!(result.starts_with("Program"));
}

#[test]
fn parse_module_only() {
    let result = parse_to_string("module mymod").unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Module"));
    assert!(s.contains("mymod"));
}

#[test]
fn parse_func_no_params_void_return() {
    let result = parse_to_string("module test\nfunc main() {\n    return\n}\n").unwrap();
    let s = strip_spans(&result);
    // Debug: eprintln!("DUMP: {result}");
    // Debug: eprintln!("STRIPPED: {s}");
    assert!(s.contains("Func main") || s.contains("main"));
    assert!(s.contains("Return") || s.contains("return"));
}

#[test]
fn parse_func_with_params_and_return() {
    let source = "module test\nfunc add(a: int, b: int): int {\n    return a + b\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("add") || s.contains("add"));
    assert!(s.contains("a: int") || (s.contains("a") && s.contains("int")));
    assert!(s.contains("Return"));
}

#[test]
fn parse_int_literal() {
    let result = parse_to_string("module test\nfunc main() {\n    let x = 42\n}\n").unwrap();
    assert!(result.contains("42"));
}

#[test]
fn parse_float_literal() {
    let result = parse_to_string("module test\nfunc main() {\n    let x = 3.14\n}\n").unwrap();
    assert!(result.contains("3.14"));
}

#[test]
fn parse_bool_literal() {
    let result =
        parse_to_string("module test\nfunc main() {\n    let a = true\n    let b = false\n}\n")
            .unwrap();
    assert!(result.contains("true"));
    assert!(result.contains("false"));
}

#[test]
fn parse_string_literal() {
    let result = parse_to_string("module test\nfunc main() {\n    let s = \"hello\"\n}\n").unwrap();
    assert!(result.contains("hello"));
}

#[test]
fn parse_nil() {
    let result = parse_to_string("module test\nfunc main() {\n    let x = nil\n}\n").unwrap();
    assert!(result.contains("nil") || result.contains("Nil"));
}

#[test]
fn parse_binary_ops() {
    let result = parse_to_string("module test\nfunc main() {\n    let x = 1 + 2 * 3\n}\n").unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("+") || s.contains("*"));
}

#[test]
fn parse_if_else() {
    let source = "module test\nfunc main() {\n    if true {\n        return 1\n    } else {\n        return 2\n    }\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("If"));
    assert!(s.contains("Else"));
}

#[test]
fn parse_while() {
    let source = "module test\nfunc main() {\n    while true {\n        break\n    }\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("While"));
    assert!(s.contains("Break"));
}

#[test]
fn parse_for_in() {
    let source =
        "module test\nfunc main() {\n    for item in items {\n        io.println(item)\n    }\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("For") || s.contains("In"));
}

#[test]
fn parse_struct_decl() {
    let source = "module test\nstruct Point {\n    x: int\n    y: int\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Struct") && s.contains("Point"));
    // Note: stripped spans leave double spaces: "Field  x", "Type  int"
    assert!(s.contains("Field  x"));
    assert!(s.contains("Field  y"));
    assert!(s.contains("Type  int"));
}

#[test]
fn parse_enum_decl() {
    let source = "module test\nenum Option {\n    Some(int)\n    None\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Enum") && s.contains("Option"));
    assert!(s.contains("Some"));
    assert!(s.contains("None"));
}

#[test]
fn parse_extern_decl() {
    let source = "module test\nextern \"C\" {\n    func puts(s: ptr[u8]): int\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Extern"));
}

#[test]
fn parse_type_alias() {
    let source = "module test\ntype MyInt = int\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("MyInt"));
}

#[test]
fn parse_interface_decl() {
    let source = "module test\ninterface Printable {\n    func print(): void\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Interface") || s.contains("Printable"));
}

#[test]
fn parse_match_int() {
    let source = "module test\nfunc main() {\n    match x {\n        1 => \"one\"\n        _ => \"other\"\n    }\n}\n";
    let result = parse_to_string(source).unwrap();
    assert!(result.contains("Match") || result.contains("Arm"));
}

#[test]
fn parse_import() {
    let source = "module test\nimport io\nfunc main() { }\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Import"));
}

#[test]
fn parse_var_decl_with_type() {
    let source = "module test\nfunc main() {\n    let x: int = 42\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("x: int") || (s.contains("x") && s.contains("int")));
}

#[test]
fn parse_unary_minus() {
    let source = "module test\nfunc main() {\n    let x = -5\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("-"));
}

#[test]
fn parse_call() {
    let source = "module test\nfunc main() {\n    io.println(\"hi\")\n}\n";
    let result = parse_to_string(source).unwrap();
    assert!(result.contains("Call") || result.contains("println"));
}

#[test]
fn parse_generic_func() {
    let source = "module test\nfunc identity<T>(x: T): T {\n    return x\n}\n";
    let result = parse_to_string(source).unwrap();
    assert!(result.contains("identity") || result.contains("T"));
}

#[test]
fn parse_multi_module_func() {
    let source = "module test\nfunc foo() { }\nfunc bar() { }\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.contains("Func foo") || s.contains("foo"));
    assert!(s.contains("Func bar") || s.contains("bar"));
}

#[test]
fn parse_rejects_missing_rbrace() {
    let err = parse("module test\nfunc main() {\n    return\n").unwrap_err();
    assert!(err.code == ParseErrorCode::ExpectedToken);
}

#[test]
fn parse_recovery_continues_after_error() {
    let output = parse_recovering("module test\nfunc main() {\n    let x = \n    return x\n}\n");
    assert!(!output.diagnostics.is_empty());
    assert!(!output.program.decls.is_empty());
}

#[test]
fn parse_nested_if() {
    let source = "module test\nfunc main() {\n    if a {\n        if b {\n            return 1\n        }\n    }\n}\n";
    let result = parse_to_string(source).unwrap();
    let s = strip_spans(&result);
    assert!(s.matches("If").count() >= 2);
}
