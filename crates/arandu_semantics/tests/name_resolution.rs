use arandu_lexer::Span;
use arandu_semantics::{DiagCode, SymbolKind, SymbolTable, resolve_for_test};

fn resolve_source(source: &str) -> arandu_semantics::ResolutionResult {
    let program = arandu_parser::parse(source).expect("parser should accept fixture");
    resolve_for_test(0, &program)
}

fn codes(result: &arandu_semantics::ResolutionResult) -> Vec<DiagCode> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn assert_no_diagnostics(source: &str) {
    let result = resolve_source(source);
    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

fn assert_diagnostic_golden(name: &str) {
    let source = arandu_test_support::read_golden_text("ui/semantics", name, "aru");
    let result = resolve_source(&source);
    arandu_test_support::assert_diagnostic_golden("ui/semantics", name, &result.diagnostics);
}

#[test]
fn resolves_forward_function_reference() {
    let result = resolve_source(
        r"
module tests.forward

func main() {
    let value = later()
}

func later(): int {
    return 1
}
",
    );

    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "later" && matches!(symbol.kind, SymbolKind::Func) })
    );
}

#[test]
fn matches_undefined_value_diagnostic_golden() {
    assert_diagnostic_golden("undefined_value");
}

#[test]
fn matches_redeclare_same_scope_diagnostic_golden() {
    assert_diagnostic_golden("redeclare_same_scope");
}

#[test]
fn matches_set_missing_diagnostic_golden() {
    assert_diagnostic_golden("set_missing");
}

#[test]
fn resolves_params_locals_and_set_roots() {
    let result = resolve_source(
        r"
module tests.locals

func add(a: int, b: int): int {
    let mut total = a + b
    total += 1
    return total
}
",
    );

    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "total" && matches!(symbol.kind, SymbolKind::Local) })
    );
}

#[test]
fn resolves_imported_namespace_member_from_prelude() {
    assert_no_diagnostics(
        r#"
module tests.import_namespace

import io

func main() {
    io.println("ok")
}
"#,
    );
}

#[test]
fn reports_namespace_used_as_value() {
    let result = resolve_source(
        r"
module tests.namespace_as_value

import io

func main() {
    let value = io
}
",
    );

    assert_eq!(codes(&result), vec![DiagCode::M003NamespaceUsedAsValue]);
}

#[test]
fn reports_undefined_namespace_member() {
    let result = resolve_source(
        r"
module tests.undefined_member

import io

func main() {
    io.missing()
}
",
    );

    assert_eq!(codes(&result), vec![DiagCode::M002UndefinedNamespaceMember]);
}

#[test]
fn resolves_named_import_aliases_by_identifier_casing() {
    assert_no_diagnostics(
        r#"
module tests.named_imports

from ui import { Button, Window as AppWindow, text as label }

func render(window: AppWindow): Button {
    return label("ok")
}
"#,
    );
}

#[test]
fn resolves_type_qualified_associated_function() {
    assert_no_diagnostics(
        r"
module tests.associated

struct User {
    name: str
}

func User.greet(user: User): str {
    return user.name
}

func main(user: User) {
    let text = User.greet(user)
}
",
    );
}

#[test]
fn reports_undefined_associated_function() {
    let result = resolve_source(
        r"
module tests.associated_missing

struct User {
    name: str
}

func main(user: User) {
    let text = User.missing(user)
}
",
    );

    assert_eq!(
        codes(&result),
        vec![DiagCode::N010UndefinedAssociatedFunction]
    );
}

#[test]
fn reports_undefined_assignment_target_with_set_specific_hint() {
    let result = resolve_source(
        r"
module tests.set_missing

func main() {
    missing = 1
}
",
    );

    assert_eq!(
        codes(&result),
        vec![DiagCode::N007UndefinedAssignmentTarget]
    );
    assert!(
        result.diagnostics[0]
            .hints
            .iter()
            .any(|hint| hint.message.contains("missing =")),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn resolves_type_names_in_params_and_struct_literals() {
    let result = resolve_source(
        r"
module tests.types

struct User {
    name: str
}

func make(name: str): User {
    return User { name: name }
}
",
    );

    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| { symbol.name == "User" && matches!(symbol.kind, SymbolKind::Struct) })
    );
}

#[test]
fn reports_undefined_value_with_suggestion() {
    let result = resolve_source(
        r"
module tests.suggest

func main() {
    let user = 1
    let value = usre
}
",
    );

    assert_eq!(codes(&result), vec![DiagCode::N001UndefinedValue]);
    let diagnostic = &result.diagnostics[0];
    assert!(diagnostic.message.contains("usre"));
    assert!(
        diagnostic
            .hints
            .iter()
            .any(|hint| hint.message.contains("user")),
        "{diagnostic:#?}"
    );
}

#[test]
fn reports_undefined_type() {
    let result = resolve_source(
        r"
module tests.undefined_type

func main(value: MissingType) {
    return
}
",
    );

    assert_eq!(codes(&result), vec![DiagCode::N002UndefinedType]);
}

#[test]
fn reports_redeclare_same_scope_but_allows_nested_shadowing() {
    let result = resolve_source(
        r"
module tests.redeclare

func main() {
    let value = 1
    if value > 0 {
        value = 2
    }
    let value = 3
}
",
    );

    assert_eq!(codes(&result), vec![DiagCode::N003RedefinedName]);
}

#[test]
fn symbol_table_keeps_value_and_type_namespaces_distinguishable() {
    let mut symbols = SymbolTable::new(0);
    let scope = symbols.global_scope();
    let span = Span::new(0, 4, 5);

    symbols
        .define(scope, "User", SymbolKind::Struct, span)
        .expect("type symbol should define");
    symbols
        .define(scope, "value", SymbolKind::Local, span)
        .expect("value symbol should define");

    assert!(symbols.lookup_type(scope, "User").is_some());
    assert!(symbols.lookup_value(scope, "User").is_none());
    assert!(symbols.lookup_value(scope, "value").is_some());
    assert!(symbols.lookup_type(scope, "value").is_none());
}

#[test]
fn resolves_match_pattern_bindings_in_arm_scope() {
    let result = resolve_source(
        r"
module tests.match_bindings

enum Token {
    Word(str)
}

func describe(token: Token): str {
    return match token {
        Token.Word(text) => text
    }
}
",
    );

    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn resolves_match_statement_pattern_bindings_in_arm_scope() {
    let result = resolve_source(
        r"
module tests.match_statement_bindings

enum Token {
    Word(str)
}

func sink(value: str) {
    return
}

func describe(token: Token) {
    match token {
        Token.Word(text) => sink(text)
    }
}
",
    );

    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn resolves_for_bindings_in_loop_scope() {
    let result = resolve_source(
        r"
module tests.forBindings

func main(items: []int) {
    for item in items {
        let value = item
    }
}
",
    );

    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn resolves_module_qualified_type_names() {
    let result = resolve_source(
        r"
        module tests.qualifiedType
        import myModule
        func main() {
            let x: myModule.SomeType = 0
        }
        ",
    );
    let diagnostics = codes(&result);
    // myModule is a module, not a type. Checking a qualified member type should report N009
    assert_eq!(diagnostics, vec![DiagCode::M002UndefinedNamespaceMember]);
}

#[test]
fn reports_undefined_associated_function_with_suggestion() {
    let result = resolve_source(
        r"
module tests.associated_suggest

struct User {
    name: str
}

func User.greet(user: User): str {
    return user.name
}

func main(user: User) {
    let text = User.grte(user)
}
",
    );

    assert_eq!(
        codes(&result),
        vec![DiagCode::N010UndefinedAssociatedFunction]
    );
    let diag = &result.diagnostics[0];
    assert!(
        diag.hints.iter().any(|h| h.message.contains("greet")),
        "should suggest 'greet' in hints, got: {:?}",
        diag.hints
    );
}

#[test]
fn test_case_insensitive_suggestion_priority() {
    let result = resolve_source(
        r"
module tests.suggest_priority

func main() {
    let myva = 1
    let myVar = 2
    // 'myvar' has distance 1 from 'myva' (c -> a) and case-insensitive distance 0 from 'myVar'.
    // We want 'myVar' to be suggested because case-insensitive matches are prioritized (dist = 0).
    let value = myvar
}
",
    );

    assert_eq!(codes(&result), vec![DiagCode::N001UndefinedValue]);
    let diag = &result.diagnostics[0];
    assert!(
        diag.hints.iter().any(|h| h.message.contains("myVar")),
        "should suggest 'myVar' due to case-insensitivity priority, got: {:?}",
        diag.hints
    );
}

#[test]
fn resolves_external_import_alias() {
    let program = arandu_parser::parse(
        r#"
        module tests.external
        import "github.com/empresa/auth" as auth
        func main() {
            auth.userService.login()
        }
        "#,
    )
    .unwrap();

    let (mut symbols, resolved, docs, diagnostics) =
        arandu_resolve::name_resolution::collect_symbols(&program);
    let span = Span::new(0, 0, 0);
    symbols
        .define_module_member("github.com/empresa/auth.userService", "login", span)
        .unwrap();

    let result = arandu_resolve::name_resolution::resolve_with_symbols(
        symbols,
        resolved,
        docs,
        diagnostics,
        &program,
    );
    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn resolves_external_import_alias_type() {
    let program = arandu_parser::parse(
        r#"
        module tests.external_type
        import "github.com/empresa/auth" as auth
        func main() {
            let x: auth.userService.User = 0
        }
        "#,
    )
    .unwrap();

    let (mut symbols, resolved, docs, diagnostics) =
        arandu_resolve::name_resolution::collect_symbols(&program);
    let span = Span::new(0, 0, 0);
    symbols
        .define_module_member("github.com/empresa/auth.userService", "User", span)
        .unwrap();

    let result = arandu_resolve::name_resolution::resolve_with_symbols(
        symbols,
        resolved,
        docs,
        diagnostics,
        &program,
    );
    assert!(result.diagnostics.is_empty(), "{:#?}", result.diagnostics);
}

#[test]
fn reports_unused_import_warning() {
    let result = resolve_source(
        r"
        module tests.unused
        import unusedModule
        func main() {
            let x = 42
        }
        ",
    );
    let diagnostics = codes(&result);
    assert_eq!(diagnostics, vec![DiagCode::W007UnusedImport]);
}
