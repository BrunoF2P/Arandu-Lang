#![allow(clippy::expect_used)]

use arandu_middle::types::{FunctionInstance, Primitive, TypeShape};
use arandu_middle::{hir::HirDecl, SymbolId};
use arandu_query::passes::{declaration_signatures, parse};
use arandu_query::runtime::{
    function_hir, instance_contracts, instance_hir, runtime_raw_unit, runtime_unit, Instance,
};
use arandu_query::{DatabaseImpl, SourceFile};
use salsa::Setter;
use std::sync::Arc;

fn instance<'db>(
    db: &'db DatabaseImpl,
    file: SourceFile,
    name: &str,
    arguments: Vec<TypeShape>,
) -> Instance<'db> {
    Instance::new(
        db,
        file,
        FunctionInstance {
            definition: symbol(db, file, name),
            arguments,
        },
    )
}

#[test]
fn final_unit_uses_imported_borrow_contracts_without_global_lowering() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let library = db.new_file(
        "loans.aru".into(),
        "module loans\npublic func choose(a: ref int, b: ref int): ref int { return a }".into(),
    );
    let file = db.new_file("main.aru".into(), "import loans\nfunc selected(a: ref int, b: ref int): ref int { return loans.choose(a, b) }\nfunc broken(): int { return absent }".into());
    let definition = symbol(&db, file, "selected");
    let key = FunctionInstance {
        definition,
        arguments: Vec::new(),
    };
    {
        let id = Instance::new(&db, file, key.clone());
        let contracts = instance_contracts(&db, id);
        assert!(
            contracts.diagnostics.is_empty(),
            "{:?}",
            contracts.diagnostics
        );
        let own = contracts
            .entries
            .iter()
            .find(|(key, _)| key.definition == definition)
            .expect("contract");
        assert_eq!(own.1.dependencies[0].sources[0].parameter_index, 0);
        let checked = runtime_unit(&db, id);
        assert!(checked.result.is_ok(), "{:?}", checked.result);
    }
    log.clear();
    library.set_text(&mut db).to(Arc::from(
        "module loans\npublic func choose(a: ref int, b: ref int): ref int { return b }",
    ));
    let id = Instance::new(&db, file, key);
    let checked = runtime_unit(&db, id);
    let own = instance_contracts(&db, id)
        .entries
        .iter()
        .find(|(key, _)| key.definition == definition)
        .expect("updated contract");
    assert_eq!(own.1.dependencies[0].sources[0].parameter_index, 1);
    assert!(checked.result.is_ok(), "{:?}", checked.result);
    assert_eq!(
        log.count_executions_matching("runtime_raw_unit"),
        1,
        "only the callee body is lowered: {}",
        log.format_chain(true)
    );
    assert_eq!(log.count_executions_matching("runtime_unit"), 1);
    for forbidden in [
        "prepare_hir",
        "lower_amir",
        "borrow_interfaces",
        "file_typing",
        "module_signatures",
    ] {
        assert_eq!(
            log.count_executions_matching(forbidden),
            0,
            "{forbidden}: {}",
            log.format_chain(true)
        );
    }
}

#[test]
fn recursive_and_generic_instance_contracts_converge_without_query_cycles() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("main.aru".into(), "func identity<T>(value: ref T): ref T { return value }\nfunc selected(depth: int, left: ref int, right: ref int): ref int { if depth == 0 { return identity(left) } return selected(depth - 1, right, left) }".into());
    let id = instance(&db, file, "selected", Vec::new());
    let contracts = instance_contracts(&db, id);
    assert!(
        contracts.diagnostics.is_empty(),
        "{:?}",
        contracts.diagnostics
    );
    let own = contracts
        .entries
        .iter()
        .find(|(key, _)| key.definition == id.key(&db).definition)
        .expect("recursive contract");
    assert_eq!(
        own.1.dependencies[0]
            .sources
            .iter()
            .map(|source| source.parameter_index)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let checked = runtime_unit(&db, id);
    assert!(checked.result.is_ok(), "{:?}", checked.result);
}

#[test]
fn final_unit_rejects_a_borrow_without_formal_origin() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "main.aru".into(),
        "@NoFallback\nfunc selected(): ref int { let local = 42 return ref local }".into(),
    );
    let checked = runtime_unit(&db, instance(&db, file, "selected", Vec::new()));
    assert!(checked.result.as_ref().expect_err("invalid escaping borrow").iter().any(|diagnostic| diagnostic.code == arandu_middle::DiagCode::O010EscapeOfBorrowedValue), "{:?}", checked.result);
}

#[test]
fn a_recursive_call_alone_cannot_invent_a_formal_borrow_origin() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "main.aru".into(),
        "func selected(value: ref int): ref int { return selected(value) }".into(),
    );
    let id = instance(&db, file, "selected", Vec::new());
    let contracts = instance_contracts(&db, id);
    assert!(
        contracts.diagnostics.is_empty(),
        "{:?}",
        contracts.diagnostics
    );
    assert!(contracts
        .entries
        .iter()
        .all(|(_, summary)| summary.dependencies.is_empty()));
    let checked = runtime_unit(&db, id);
    assert!(checked.result.as_ref().expect_err("unproven recursive borrow").iter().any(|diagnostic| diagnostic.code == arandu_middle::DiagCode::O010EscapeOfBorrowedValue), "{:?}", checked.result);
}

#[test]
fn generic_calls_in_loop_initializer_and_step_are_both_specialized() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("main.aru".into(), "func identity<T>(x: T): T { return x }\nfunc selected(): int { let mut i = 0 for i = identity<int>(0); i < 3; i = identity<int>(i + 1) {} return i }".into());
    let checked = runtime_raw_unit(&db, instance(&db, file, "selected", Vec::new()));
    let raw = checked.result.as_ref().expect("loop unit");
    let identity = symbol(&db, file, "identity");
    let specialized = checked
        .instances
        .iter()
        .find(|(_, key)| key.definition == identity)
        .expect("identity header")
        .0;
    let calls = raw
        .function
        .stmts
        .iter_ids()
        .filter_map(|id| match raw.function.stmts.get(id) {
            Some(arandu_middle::amir::AmirStmt::Call {
                callee: arandu_middle::amir::AmirOperand::FunctionRef(symbol),
                ..
            }) => Some(*symbol),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(calls, vec![specialized, specialized]);
}

#[test]
fn a_generic_instance_lowers_only_its_body_with_concrete_callee_headers() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let file = db.new_file("main.aru".into(),
        "func inner<T>(x: T): T { return x }\nfunc outer<T>(x: T): T { return inner<T>(x) }\nfunc broken(): int { return absent }".into());
    let definition = symbol(&db, file, "outer");
    let helper = symbol(&db, file, "inner");
    let key = FunctionInstance {
        definition,
        arguments: vec![TypeShape::Primitive(Primitive::Int)],
    };
    log.clear();
    let id = Instance::new(&db, file, key.clone());
    let hir = instance_hir(&db, id);
    assert!(
        hir.artifacts.diagnostics.is_empty(),
        "{:?}",
        hir.artifacts.diagnostics
    );
    let lowered = hir.artifacts.hir.as_ref().expect("instance HIR");
    assert_eq!(
        lowered
            .decls
            .iter()
            .filter(
                |&&decl| matches!(lowered.pool.decl(decl), HirDecl::Func(f) if f.body.is_some())
            )
            .count(),
        1
    );
    assert!(hir.instances.iter().any(|(_, key)| key.definition == helper
        && key.arguments == vec![TypeShape::Primitive(Primitive::Int)]));
    let raw = runtime_raw_unit(&db, id);
    let unit = raw.result.as_ref().expect("instance AMIR");
    assert_eq!(
        TypeShape::from_id(
            unit.function.return_type,
            &raw.context.type_info.type_interner
        ),
        Ok(TypeShape::Primitive(Primitive::Int))
    );
    assert_eq!(
        log.count_executions_matching("item_typing"),
        1,
        "{}",
        log.format_chain(true)
    );
    for forbidden in [
        "prepare_hir",
        "lower_amir",
        "borrow_interfaces",
        "file_typing",
        "module_signatures",
    ] {
        assert_eq!(
            log.count_executions_matching(forbidden),
            0,
            "{forbidden}: {}",
            log.format_chain(true)
        );
    }
}

#[test]
fn changing_a_callee_body_does_not_rebuild_the_callers_raw_instance() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let text = "func inner<T>(x: T): int { return 1 }\nfunc outer(): int { return inner<int>(42) }";
    let file = db.new_file("main.aru".into(), text.into());
    let definition = symbol(&db, file, "outer");
    let key = FunctionInstance {
        definition,
        arguments: Vec::new(),
    };
    {
        let raw = runtime_raw_unit(&db, Instance::new(&db, file, key.clone()));
        assert!(raw.result.is_ok(), "{:?}", raw.result);
    }
    log.clear();
    // Same byte length and signature, different callee behavior.
    file.set_text(&mut db)
        .to(Arc::from(text.replace("return 1", "return 2")));
    let raw = runtime_raw_unit(&db, Instance::new(&db, file, key));
    assert!(raw.result.is_ok(), "{:?}", raw.result);
    assert_eq!(
        log.count_executions_matching("runtime_raw_unit"),
        0,
        "{}",
        log.format_chain(true)
    );
}

#[test]
fn instances_of_the_same_template_keep_separate_concrete_return_types() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "main.aru".into(),
        "func identity<T>(x: T): T { return x }".into(),
    );
    for primitive in [Primitive::Int, Primitive::Bool] {
        let raw = runtime_raw_unit(
            &db,
            instance(&db, file, "identity", vec![TypeShape::Primitive(primitive)]),
        );
        let unit = raw.result.as_ref().expect("concrete unit");
        assert_eq!(
            TypeShape::from_id(
                unit.function.return_type,
                &raw.context.type_info.type_interner
            ),
            Ok(TypeShape::Primitive(primitive))
        );
    }
}

#[test]
fn an_instance_with_nested_free_or_unknown_type_symbols_is_rejected() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "main.aru".into(),
        "func identity<T>(x: T): T { return x }".into(),
    );
    let definition = symbol(&db, file, "identity");
    let parameter = declaration_signatures(&db, file).type_info.generic_params[&definition][0];
    for symbol in [parameter, SymbolId::new(999_999, 42)] {
        let key = FunctionInstance {
            definition,
            arguments: vec![TypeShape::Option(Box::new(TypeShape::Named(
                symbol,
                Vec::new(),
            )))],
        };
        let raw = runtime_raw_unit(&db, Instance::new(&db, file, key));
        assert!(
            raw.result
                .as_ref()
                .expect_err("unresolved structural argument")
                .iter()
                .any(|diagnostic| diagnostic.code
                    == arandu_middle::DiagCode::G002GenericInstantiationLimit),
            "{:?}",
            raw.result
        );
    }
}

#[test]
fn generic_headers_with_the_same_short_name_retain_distinct_module_identities() {
    let mut db = DatabaseImpl::new();
    let a = db.new_file(
        "a.aru".into(),
        "module a\npublic func identity<T>(x: T): T { return x }".into(),
    );
    let b = db.new_file(
        "b.aru".into(),
        "module b\npublic func identity<T>(x: T): T { return x }".into(),
    );
    let file = db.new_file("main.aru".into(), "import a\nimport b\nfunc selected(): int { return a.identity<int>(20) + b.identity<int>(22) }".into());
    let raw = runtime_raw_unit(&db, instance(&db, file, "selected", Vec::new()));
    assert!(raw.result.is_ok(), "{:?}", raw.result);
    let a_definition = symbol(&db, a, "identity");
    let b_definition = symbol(&db, b, "identity");
    let a_header = raw
        .instances
        .iter()
        .find(|(_, key)| key.definition == a_definition)
        .expect("a header")
        .0;
    let b_header = raw
        .instances
        .iter()
        .find(|(_, key)| key.definition == b_definition)
        .expect("b header")
        .0;
    assert_ne!(a_header, b_header);
    assert_ne!(
        raw.context.symbols.host_function_names[&a_header],
        raw.context.symbols.host_function_names[&b_header]
    );
}

#[test]
fn changing_a_returned_origin_invalidates_only_final_caller_validation() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let library = db.new_file(
        "loans.aru".into(),
        "module loans\npublic func choose(a: ref int, b: ref int): ref int { return a }".into(),
    );
    let file = db.new_file("main.aru".into(), "import loans\nfunc selected(): int { let mut a = 1\nlet mut b = 2\nlet borrowed = loans.choose(a, b)\nb = 3\nreturn *borrowed }".into());
    let definition = symbol(&db, file, "selected");
    let key = FunctionInstance {
        definition,
        arguments: Vec::new(),
    };
    {
        let checked = runtime_unit(&db, Instance::new(&db, file, key.clone()));
        assert!(checked.result.is_ok(), "{:?}", checked.result);
    }
    log.clear();
    library.set_text(&mut db).to(Arc::from(
        "module loans\npublic func choose(a: ref int, b: ref int): ref int { return b }",
    ));
    let checked = runtime_unit(&db, Instance::new(&db, file, key));
    assert!(
        checked
            .result
            .as_ref()
            .expect_err("mutation of the newly borrowed input")
            .iter()
            .any(|diagnostic| diagnostic.code == arandu_middle::DiagCode::O003MutableBorrowConflict),
        "{:?}",
        checked.result
    );
    assert_eq!(
        log.count_executions_matching("runtime_raw_unit"),
        1,
        "{}",
        log.format_chain(true)
    );
    assert_eq!(log.count_executions_matching("runtime_unit"), 1);
}

fn symbol(db: &DatabaseImpl, file: SourceFile, name: &str) -> SymbolId {
    let declarations = declaration_signatures(db, file);
    arandu_semantics::body_item_symbols(
        parse(db, file).as_ref().expect("source"),
        &declarations.resolved,
    )
    .into_iter()
    .find(|id| declarations.symbols.get(*id).name == name)
    .expect("function")
}

#[test]
fn function_hir_lowers_only_the_selected_body_and_declaration_context() {
    let (mut db, log) = DatabaseImpl::with_rebuild_log();
    let library = db.new_file("math.aru".into(), "module math\npublic func helper(x: int): int { return x + 1 }\nfunc bad(): int { return absent }".into());
    let entry = db.new_file("main.aru".into(), "import math\nfunc selected(): int { return math.helper(41) }\nfunc sibling(): int { return absent }".into());
    let selected = symbol(&db, entry, "selected");
    let helper = symbol(&db, library, "helper");
    log.clear();
    let lowered = function_hir(&db, entry, selected);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let hir = lowered.hir.as_ref().expect("selected HIR");
    let bodies = hir
        .decls
        .iter()
        .filter_map(|&id| match hir.pool.decl(id) {
            HirDecl::Func(function) if function.body.is_some() => Some(function.symbol),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(bodies, vec![selected]);
    assert!(hir.decls.iter().any(|&id| matches!(hir.pool.decl(id), HirDecl::Func(function) if function.symbol == helper && function.body.is_none())));
    assert_eq!(log.count_executions_matching("item_typing"), 1);
    for forbidden in [
        "prepare_hir",
        "file_typing",
        "module_signatures",
        "borrow_interfaces",
        "lower_amir",
    ] {
        assert_eq!(
            log.count_executions_matching(forbidden),
            0,
            "{forbidden}: {}",
            log.format_chain(true)
        );
    }
}

#[test]
fn a_selected_function_with_own_error_does_not_get_a_dummy_hir() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file(
        "main.aru".into(),
        "func selected(): int { return absent }\nfunc sibling(): int { return 42 }".into(),
    );
    let selected = symbol(&db, file, "selected");
    let lowered = function_hir(&db, file, selected);
    assert!(lowered.hir.is_none());
    assert!(lowered
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == arandu_middle::DiagCode::N001UndefinedValue));
}

#[test]
fn declaration_context_preserves_constants_and_consuming_receiver_modes() {
    let mut db = DatabaseImpl::new();
    let file = db.new_file("main.aru".into(), "const ANSWER = 42\nstruct Owner { value: int }\nfunc Owner.consume(self: own Owner): int { return self.value }\nfunc selected(): int { return ANSWER }".into());
    let selected = symbol(&db, file, "selected");
    let lowered = function_hir(&db, file, selected);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let hir = lowered.hir.as_ref().expect("selected HIR");
    assert!(hir
        .decls
        .iter()
        .any(|&id| matches!(hir.pool.decl(id), HirDecl::Const(_))));
    assert!(hir.decls.iter().any(|&id| match hir.pool.decl(id) {
        HirDecl::Func(function)
            if lowered
                .type_check
                .symbols
                .get(function.symbol)
                .name
                .ends_with("consume") =>
        {
            hir.pool.params_list(function.params)[0].receiver_kind
                == Some(arandu_middle::hir::ReceiverKind::Own)
        }
        _ => false,
    }));
    let function = hir
        .decls
        .iter()
        .find_map(|&id| match hir.pool.decl(id) {
            HirDecl::Func(function) if function.symbol == selected && function.body.is_some() => {
                Some(function)
            }
            _ => None,
        })
        .expect("function");
    let unit =
        arandu_mir::lower_function_unit(&lowered.type_check, hir, function, 8).expect("MIR unit");
    assert_eq!(unit.literals.entries.len(), 1);
    assert_eq!(
        arandu_middle::literal_pool::parse_int_literal(match &unit.literals.entries[0] {
            arandu_middle::literal_pool::AmirLiteralEntry::Int(value) => value,
            other => panic!("unexpected literal: {other:?}"),
        }),
        Some(42)
    );
}
