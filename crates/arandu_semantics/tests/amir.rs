use arandu_lexer::Span;
use arandu_semantics::DenseRange;
use arandu_semantics::amir::{
    AmirBasicBlock, AmirConstant, AmirFunc, AmirLocal, AmirOperand, AmirPlace, AmirProjection,
    AmirRvalue, AmirStmt, AmirStmtTable, AmirTemp, AmirTerminator, BlockId, Dominators, LocalId,
    TempId, reachable_blocks_dense,
};
use arandu_semantics::literal_pool::AmirLiteralPool;
use arandu_semantics::passes::liveness::analyze_local_liveness;
use arandu_semantics::passes::optimize::optimize_amir_func;
use arandu_semantics::passes::type_checker::types::ArType;
use arandu_semantics::{
    DiagCode, SymbolId, SymbolKind, lower_to_amir, lower_to_hir, resolve, type_check,
    validate_amir_program,
};

#[test]
fn test_amir_golden_files() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixtures_dir = root_dir.join("tests").join("codegen");

    if !fixtures_dir.exists() {
        // No fixtures directory = nothing to test
        return;
    }

    let update_golden = std::env::var("UPDATE_GOLDEN").is_ok();

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&fixtures_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "aru") {
            entries.push(path);
        }
    }

    entries.sort();

    for path in entries {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let src = std::fs::read_to_string(&path).unwrap();

        let program = arandu_parser::parse(&src).unwrap_or_else(|err| {
            panic!("failed to parse {name}: {err:?}");
        });
        let resolution = resolve(&program);
        let tc = type_check(resolution, &program);
        let errors: Vec<_> = tc
            .diagnostics
            .iter()
            .filter(|d| d.severity == arandu_semantics::Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "type check failed for {name}: {errors:?}"
        );
        let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
        hir.validate_invariants(&hir.pool, &tc.symbols)
            .expect("HIR invariant validation failed");
        let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
        let amir_issues = validate_amir_program(&amir, &tc.symbols);
        assert!(
            amir_issues.is_empty(),
            "AMIR validation failed for {name}: {amir_issues:?}"
        );
        let pretty = amir.pretty_print(&tc.symbols, &tc.type_info.type_interner);

        let golden_path = fixtures_dir.join(format!("{name}.amir"));
        if update_golden {
            std::fs::write(&golden_path, &pretty).unwrap();
        } else {
            assert!(
                golden_path.exists(),
                "Golden file missing for {name}. Run with UPDATE_GOLDEN=1 to create it."
            );
            let expected = std::fs::read_to_string(&golden_path).unwrap();
            let expected_normalized = expected.replace("\r\n", "\n");
            let pretty_normalized = pretty.replace("\r\n", "\n");
            assert_eq!(
                pretty_normalized, expected_normalized,
                "AMIR mismatch for {name}. Run with UPDATE_GOLDEN=1 to update."
            );
        }
    }
}

#[test]
fn field_projection_uses_field_symbol_id() {
    let src = r#"
struct Point {
    x: int
    y: int
}

func main() {
    let p: Point = Point { x: 1, y: 2 }
    p.x = 3
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let x_symbol = tc
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Field && symbol.name == "x")
        .map(|symbol| symbol.id)
        .expect("missing field symbol");
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");

    let func = &amir.funcs[0];
    let has_symbol_projection = func.blocks.iter().any(|block| {
        func.block_stmts(block.id).any(|stmt| match stmt {
            AmirStmt::Store { lhs, .. } => lhs
                .projections
                .iter()
                .any(|projection| matches!(projection, AmirProjection::Field(symbol) if *symbol == x_symbol)),
            _ => false,
        })
    });
    assert!(
        has_symbol_projection,
        "expected p.x store to use field SymbolId"
    );
}

#[test]
fn non_copy_local_use_after_move_fails_during_amir_analysis() {
    let src = r#"
struct Boxed {
    value: int
}

func main() {
    let a: Boxed = Boxed { value: 1 }
    let b: Boxed = a
    let c: Boxed = a
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let diagnostics = lower_to_amir(&tc, &hir).expect_err("expected use after move diagnostic");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagCode::O001UseAfterMove),
        "expected O001 use-after-move diagnostic, got {diagnostics:?}"
    );
}

#[test]
fn copy_local_can_be_reused_during_amir_lowering() {
    let src = r#"
func main() {
    let a: int = 1
    let b: int = a
    let c: int = a
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
    let pretty = amir.pretty_print(&tc.symbols, &tc.type_info.type_interner);
    assert!(
        !pretty.contains("move _"),
        "copy types must not emit move operands"
    );
}

#[test]
fn branch_move_mismatch_reports_o007() {
    let src = r#"
struct Boxed {
    value: int
}

func main(cond: bool) {
    let a: Boxed = Boxed { value: 1 }
    if cond {
        let b: Boxed = a
    }
    let c: Boxed = a
}
"#;
    let program = arandu_parser::parse(src).expect("parse failed");
    let resolution = resolve(&program);
    let tc = type_check(resolution, &program);
    let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
    let diagnostics = lower_to_amir(&tc, &hir).expect_err("expected branch move diagnostic");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagCode::O007InconsistentMoveBetweenBranches),
        "expected O007 inconsistent move diagnostic, got {diagnostics:?}"
    );
}

fn empty_block(id: usize, _predecessors: &[usize], successors: &[usize]) -> AmirBasicBlock {
    let term = match successors {
        [] => AmirTerminator::Unreachable,
        &[s] => AmirTerminator::Goto(BlockId::from_usize(s)),
        &[t, f] => AmirTerminator::Branch {
            condition: AmirOperand::Constant(AmirConstant::Bool(true)),
            if_true: BlockId::from_usize(t),
            if_false: BlockId::from_usize(f),
        },
        _ => panic!("too many successors in test"),
    };
    AmirBasicBlock {
        id: BlockId::from_usize(id),
        statements: DenseRange::empty(),
        terminator: term,
    }
}

fn temp(id: usize) -> TempId {
    TempId::from_usize(id)
}

fn local(id: usize) -> LocalId {
    LocalId::from_usize(id)
}

fn place(id: usize) -> AmirPlace {
    AmirPlace {
        local: local(id),
        projections: Default::default(),
    }
}

fn symbol(id: u32) -> SymbolId {
    SymbolId(id)
}

fn dummy_span() -> Span {
    Span::new(0, 0, 0)
}

fn test_local(id: usize, symbol_id: u32) -> AmirLocal {
    AmirLocal {
        id: local(id),
        symbol: Some(symbol(symbol_id)),
        ty: ArType::Void,
        span: dummy_span(),
        use_span: None,
    }
}

fn test_temp(id: usize) -> AmirTemp {
    AmirTemp {
        id: temp(id),
        ty: ArType::Void,
        span: dummy_span(),
    }
}

fn test_func(
    locals: Vec<AmirLocal>,
    temps: Vec<AmirTemp>,
    blocks: Vec<AmirBasicBlock>,
    stmts: AmirStmtTable,
) -> AmirFunc {
    let cfg = arandu_semantics::cfg::compute_cfg_edges(&blocks);
    AmirFunc {
        symbol: symbol(0),
        return_type: ArType::Void,
        receiver: None,
        params: Vec::new(),
        locals,
        temps,
        blocks,
        stmts,
        cfg,
    }
}

#[test]
fn dense_reachability_tracks_cfg_without_hash_sets() {
    let mut func = test_func(
        Vec::new(),
        Vec::new(),
        vec![
            empty_block(0, &[], &[1]),
            empty_block(1, &[0], &[2]),
            empty_block(2, &[1], &[]),
            empty_block(3, &[], &[]),
        ],
        AmirStmtTable::new(),
    );
    func.blocks[0].terminator = AmirTerminator::Goto(BlockId::from_usize(1));
    func.blocks[1].terminator = AmirTerminator::Goto(BlockId::from_usize(2));
    func.blocks[2].terminator = AmirTerminator::Return;

    let reachable = reachable_blocks_dense(&func);

    assert!(reachable.contains(BlockId::from_usize(0)));
    assert!(reachable.contains(BlockId::from_usize(1)));
    assert!(reachable.contains(BlockId::from_usize(2)));
    assert!(!reachable.contains(BlockId::from_usize(3)));
}

#[test]
fn dominance_frontiers_are_represented_as_dense_bit_matrix() {
    let func = test_func(
        Vec::new(),
        Vec::new(),
        vec![
            empty_block(0, &[], &[1, 2]),
            empty_block(1, &[0], &[3]),
            empty_block(2, &[0], &[3]),
            empty_block(3, &[1, 2], &[]),
        ],
        AmirStmtTable::new(),
    );
    let doms = Dominators::new(&func);
    let frontiers = doms.frontiers(&func);

    assert!(frontiers.contains(BlockId::from_usize(1), BlockId::from_usize(3)));
    assert!(frontiers.contains(BlockId::from_usize(2), BlockId::from_usize(3)));
    assert!(!frontiers.contains(BlockId::from_usize(0), BlockId::from_usize(3)));
}

#[test]
fn local_liveness_uses_dense_bitsets() {
    let mut stmts = AmirStmtTable::new();
    let first = stmts.push(AmirStmt::Assign {
        lhs: temp(0),
        rhs: AmirRvalue::Load(place(0)),
    });
    let second = stmts.push(AmirStmt::Store {
        lhs: place(1),
        rhs: AmirOperand::Copy(temp(0)),
    });
    let func = test_func(
        vec![test_local(0, 1), test_local(1, 2)],
        vec![test_temp(0)],
        vec![AmirBasicBlock {
            id: BlockId::from_usize(0),
            statements: DenseRange::new(first.as_usize(), second.as_usize() - first.as_usize() + 1),
            terminator: AmirTerminator::Return,
        }],
        stmts,
    );

    let liveness = analyze_local_liveness(&func);

    assert!(liveness.live_in(BlockId::from_usize(0)).contains(local(0)));
    assert!(!liveness.live_in(BlockId::from_usize(0)).contains(local(1)));
}

#[test]
fn dce_tracks_used_temps_with_dense_bitsets() {
    let mut stmts = AmirStmtTable::new();
    let first = stmts.push(AmirStmt::Assign {
        lhs: temp(0),
        rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(true))),
    });
    stmts.push(AmirStmt::Assign {
        lhs: temp(1),
        rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(false))),
    });
    let func_block = AmirBasicBlock {
        id: BlockId::from_usize(0),
        statements: DenseRange::new(first.as_usize(), 2),
        terminator: AmirTerminator::Return,
    };
    let mut func = test_func(
        Vec::new(),
        vec![test_temp(0), test_temp(1)],
        vec![func_block],
        stmts,
    );
    let mut literal_pool = AmirLiteralPool::default();

    optimize_amir_func(&mut func, &mut literal_pool);

    let remaining: Vec<_> = func.block_stmt_ids(BlockId::from_usize(0)).collect();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0], first);
}

#[test]
fn validate_amir_rejects_poison_temp_with_icegen002() {
    use arandu_semantics::DiagCode;

    let func_block = AmirBasicBlock {
        id: BlockId::from_usize(0),
        statements: DenseRange::new(0, 0),
        terminator: AmirTerminator::Return,
    };
    let mut poison_temp = test_temp(0);
    poison_temp.ty = arandu_middle::types::ArType::Error;
    let func = test_func(
        Vec::new(),
        vec![poison_temp],
        vec![func_block],
        AmirStmtTable::new(),
    );
    let mut symbols = arandu_semantics::SymbolTable::new();
    symbols
        .define(
            symbols.global_scope(),
            "test_fn",
            SymbolKind::Func,
            dummy_span(),
        )
        .unwrap();
    let issues = arandu_middle::amir_validate::validate_amir_func(&func, &symbols);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, DiagCode::ICEGEN002);
}
