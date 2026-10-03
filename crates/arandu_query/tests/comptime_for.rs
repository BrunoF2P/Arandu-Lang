#![allow(clippy::expect_used, clippy::panic)]

use arandu_middle::{ctfe::ConstValue, DiagCode, Severity};
use arandu_query::{passes, runtime, DatabaseImpl};
use salsa::Setter;

fn errors(db: &DatabaseImpl, file: arandu_query::SourceFile) -> Vec<arandu_middle::Diagnostic> {
    passes::type_check(db, file)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect()
}

#[test]
fn finite_static_domains_lower_to_fresh_acyclic_iteration_blocks() {
    for source in [
        "func main(): int { let mut sum = 0\ncomptime for i in 0..4 { sum += i }\nreturn sum }",
        "func main(): int { let mut sum = 0\ncomptime for i in 4..4 { sum += i }\nreturn sum }",
        "func main(): int { let mut sum = 0\ncomptime for i in 5..2 { sum += i }\nreturn sum }",
        "func main(): int { let mut sum = 0\ncomptime for i in 0..4 { if i == 1 { continue } if i == 3 { break } sum += i }\nreturn sum }",
        "func main(): int { comptime for i in 0..4 { return i }\nreturn 99 }",
        "func main(): int { let mut sum = 0\ncomptime for i in 0..3 { comptime for j in 0..2 { sum += i + j } }\nreturn sum }",
        "func end(): uint { return 4 }\nfunc main(): int { let mut sum = 0\ncomptime for i in 0..end() { sum += i as int }\nreturn sum }",
        "func main(): int { let mut sum = 0\ncomptime for i in -2..2 { sum += i }\nreturn sum }",
    ] {
        let mut db = DatabaseImpl::new();
        let file = db.new_file("for.aru".into(), source.into());
        assert!(passes::parse(&db, file).is_ok(), "{source}");
        assert!(errors(&db, file).is_empty(), "{source}: {:?}", errors(&db, file));
        let output = runtime::runtime_program(&db, file);
        assert!(output.diagnostics.is_empty(), "{source}: {:?}", output.diagnostics);
        for function in &output.artifacts.amir.funcs {
            let mut incoming: Vec<_> = function.cfg.predecessors.iter().map(Vec::len).collect();
            let mut ready: Vec<_> = incoming.iter().enumerate().filter_map(|(id, &n)| (n == 0).then_some(id)).collect();
            let mut visited = 0;
            while let Some(id) = ready.pop() {
                visited += 1;
                for target in &function.cfg.successors[id] {
                    let target = target.as_usize();
                    incoming[target] -= 1;
                    if incoming[target] == 0 { ready.push(target); }
                }
            }
            assert_eq!(visited, function.blocks.len(), "static expansion retained a CFG cycle: {source}");
            assert!(function.locals.iter().enumerate().all(|(index, local)| local.id.as_usize() == index));
        }
    }
}

#[test]
fn static_domains_use_vm_helpers_and_reject_runtime_captures() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("for.aru".into(), "func end(): int { return 4 }\nfunc main(): int { let mut sum = 0\ncomptime for i in 0..end() { sum += i }\nreturn sum }".into());
    assert!(errors(&db, file).is_empty(), "{:?}", errors(&db, file));
    for source in [
        "func main(n: int): int { comptime for i in 0..n { return i } return 0 }",
        "const n = 4\nfunc main(n: int): int { comptime for i in 0..n { return i } return 0 }",
    ] {
        let mut db = DatabaseImpl::new();
        let file = db.new_file("for.aru".into(), source.into());
        assert!(
            errors(&db, file)
                .iter()
                .any(|d| d.code == DiagCode::T043ComptimeRuntimeCapture),
            "{:?}",
            errors(&db, file)
        );
    }
}

#[test]
fn invalid_and_exponential_domains_fail_without_publishing_partial_amir() {
    for (source, code) in [
        (
            "func main(): void { comptime for i in 0..4097 {} }",
            DiagCode::T045ComptimeLimitExceeded,
        ),
        (
            "func main(): void { comptime for i in 0..65 { comptime for j in 0..65 {} } }",
            DiagCode::T045ComptimeLimitExceeded,
        ),
        (
            "func main(): void { comptime for i in 0..=4 {} }",
            DiagCode::T042UnsupportedComptime,
        ),
        (
            "func main(): void { comptime for mut i in 0..4 {} }",
            DiagCode::T042UnsupportedComptime,
        ),
        (
            "func main(): void { comptime for i in [1, 2] {} }",
            DiagCode::T042UnsupportedComptime,
        ),
    ] {
        let mut db = DatabaseImpl::new();
        let file = db.new_file("for.aru".into(), source.into());
        let output = runtime::runtime_program(&db, file);
        assert!(
            output.diagnostics.iter().any(|d| d.code == code),
            "{source}: {:?}",
            output.diagnostics
        );
        assert!(
            !output
                .diagnostics
                .iter()
                .any(|d| d.code.as_str().starts_with("ICE")),
            "{:?}",
            output.diagnostics
        );
    }
}

#[test]
fn a_static_loop_inside_a_comptime_root_is_executed_by_the_vm() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("for.aru".into(), "func main(): int { return comptime { let mut sum = 0\ncomptime for i in 0..4 { sum += i }\nsum } }".into());
    let output = passes::type_check(&db, file);
    assert!(errors(&db, file).is_empty(), "{:?}", errors(&db, file));
    assert!(output
        .type_info
        .ctfe_values
        .values()
        .any(|(v, _)| matches!(v, ConstValue::Integer(n) if n.value() == 6)));
}

#[test]
fn changing_a_domain_matches_clean_compilation() {
    let mut db = DatabaseImpl::new();
    let source =
        "func main(): int { let mut sum = 0\ncomptime for i in 0..4 { sum += i }\nreturn sum }";
    let file = db.new_file("for.aru".into(), source.into());
    assert!(runtime::runtime_program(&db, file).diagnostics.is_empty());
    let edited = source.replace("0..4", "0..5");
    file.set_text(&mut db).to(edited.clone().into());
    let incremental = runtime::runtime_program(&db, file);
    let mut clean = DatabaseImpl::new();
    let fresh = clean.new_file("for.aru".into(), edited);
    let rebuilt = runtime::runtime_program(&clean, fresh);
    assert!(
        incremental.diagnostics.is_empty(),
        "{:?}",
        incremental.diagnostics
    );
    assert_eq!(
        format!("{:?}", incremental.artifacts.amir),
        format!("{:?}", rebuilt.artifacts.amir)
    );
}
