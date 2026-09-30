#![allow(clippy::expect_used)]

use super::*;
use arandu_middle::amir::*;
use arandu_middle::layout::{DataLayout, DenseRange};
use arandu_middle::literal_pool::AmirLiteralPool;
use arandu_middle::types::{ArType, Primitive, TypeInterner};
use arandu_middle::{Span, SymbolId};
use std::sync::Arc;

struct Units(Vec<Arc<CtfeFunction>>);
impl FunctionProvider for Units {
    fn function(&self, symbol: SymbolId) -> Result<Arc<CtfeFunction>, EvalErrorKind> {
        self.0
            .iter()
            .find(|unit| unit.function().symbol == symbol)
            .map(Arc::clone)
            .ok_or(EvalErrorKind::MissingFunction(symbol))
    }
}

fn budget() -> Budget {
    Budget {
        fuel: 10_000,
        frames: 16,
        values: 128,
    }
}

fn function(symbol: SymbolId, temp_count: usize) -> (AmirFunc, TypeInterner, AmirLiteralPool) {
    let types = TypeInterner::new();
    let int = types.intern(ArType::Primitive(Primitive::Int));
    let function = AmirFunc {
        symbol,
        return_type: int,
        receiver: None,
        params: vec![],
        locals: vec![],
        temps: (0..temp_count)
            .map(|index| AmirTemp {
                id: TempId::from_usize(index),
                ty: int,
                is_copy: true,
                is_nullable: false,
                span: Span::new(symbol.file_id, 1, 2),
            })
            .collect(),
        blocks: vec![block(0)],
        block_params: vec![],
        stmts: AmirStmtTable::new(),
        cfg: Default::default(),
    };
    (function, types, AmirLiteralPool::default())
}

fn block(index: usize) -> AmirBasicBlock {
    AmirBasicBlock {
        id: BlockId::from_usize(index),
        params: DenseRange::empty(),
        statements: DenseRange::empty(),
        terminator: AmirTerminator::Return,
    }
}

fn unit(function: AmirFunc, types: &TypeInterner, pool: AmirLiteralPool) -> Arc<CtfeFunction> {
    Arc::new(
        CtfeFunction::new(function, pool, types, DataLayout::ptr_width(8)).expect("scalar unit"),
    )
}

fn value(value: i128) -> ConstValue {
    ConstValue::Integer(
        ConstInt::new(
            IntegerType::new(Primitive::Int, DataLayout::ptr_width(8)).expect("int"),
            value,
        )
        .expect("integer"),
    )
}

#[test]
fn calls_share_exact_fuel_and_recover_frame_slots_after_return() {
    let root = SymbolId::new(1, 0);
    let child = SymbolId::new(1, 1);
    let (mut a, types, pool) = function(root, 1);
    a.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Call {
            lhs: Some(TempId(0)),
            callee: AmirOperand::FunctionRef(child),
            args: Default::default(),
            return_borrow: None,
        },
    );
    let (mut b, child_types, mut literals) = function(child, 1);
    let constant = AmirOperand::Constant(AmirConstant::Pool(literals.intern_int("42")));
    b.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Assign {
            lhs: TempId(0),
            rhs: AmirRvalue::Use(constant),
        },
    );
    let units = Units(vec![unit(a, &types, pool), unit(b, &child_types, literals)]);
    let mut exact = budget();
    exact.fuel = 15;
    exact.frames = 2;
    exact.values = 2;
    assert_eq!(evaluate(&units, root, &[], exact, || false), Ok(value(42)));
    exact.fuel = 14;
    assert_eq!(
        evaluate(&units, root, &[], exact, || false)
            .expect_err("one step short")
            .kind,
        EvalErrorKind::FuelExhausted
    );
    exact = budget();
    exact.frames = 1;
    assert_eq!(
        evaluate(&units, root, &[], exact, || false)
            .expect_err("frame bound")
            .kind,
        EvalErrorKind::FrameLimit
    );
    exact = budget();
    exact.values = 1;
    assert_eq!(
        evaluate(&units, root, &[], exact, || false)
            .expect_err("combined slots")
            .kind,
        EvalErrorKind::ValueLimit
    );
    // Two sequential child invocations fit in the same two-frame/two-slot cap.
    let (mut a, types, pool) = function(root, 1);
    for lhs in [None, Some(TempId(0))] {
        a.append_stmt_to_block(
            BlockId(0),
            AmirStmt::Call {
                lhs,
                callee: AmirOperand::FunctionRef(child),
                args: Default::default(),
                return_borrow: None,
            },
        );
    }
    let units = Units(vec![unit(a, &types, pool), Arc::clone(&units.0[1])]);
    exact = budget();
    exact.frames = 2;
    exact.values = 2;
    assert_eq!(evaluate(&units, root, &[], exact, || false), Ok(value(42)));
}

#[test]
fn backedge_arguments_are_read_simultaneously_and_scratch_counts_toward_budget() {
    let root = SymbolId::new(1, 0);
    let (mut f, types, mut literals) = function(root, 8);
    let int = f.return_type;
    f.temps[7].ty = types.intern(ArType::Primitive(Primitive::Bool));
    f.params = vec![TempId(3), TempId(4)];
    f.blocks.extend([block(1), block(2)]);
    f.block_params = [1, 2, 5]
        .into_iter()
        .map(|index| BlockParam {
            id: TempId(index),
            local: LocalId(index),
            ty: int,
            from: None,
            moved: false,
        })
        .collect();
    f.blocks[1].params = DenseRange::new(0, 3);
    let one = AmirOperand::Constant(AmirConstant::Pool(literals.intern_int("1")));
    let zero = AmirOperand::Constant(AmirConstant::Pool(literals.intern_int("0")));
    f.blocks[0].terminator = AmirTerminator::Goto {
        target: BlockId(1),
        args: vec![
            AmirOperand::Copy(TempId(3)),
            AmirOperand::Copy(TempId(4)),
            one,
        ],
    };
    f.append_stmt_to_block(
        BlockId(1),
        AmirStmt::Assign {
            lhs: TempId(6),
            rhs: AmirRvalue::Binary {
                op: BinaryOp::Sub,
                left: AmirOperand::Copy(TempId(5)),
                right: one,
            },
        },
    );
    f.append_stmt_to_block(
        BlockId(1),
        AmirStmt::Assign {
            lhs: TempId(7),
            rhs: AmirRvalue::Binary {
                op: BinaryOp::Gt,
                left: AmirOperand::Copy(TempId(5)),
                right: zero,
            },
        },
    );
    f.blocks[1].terminator = AmirTerminator::Branch {
        condition: AmirOperand::Copy(TempId(7)),
        if_true: BlockId(1),
        true_args: vec![
            AmirOperand::Copy(TempId(2)),
            AmirOperand::Copy(TempId(1)),
            AmirOperand::Copy(TempId(6)),
        ],
        if_false: BlockId(2),
        false_args: vec![],
    };
    f.append_stmt_to_block(
        BlockId(2),
        AmirStmt::Assign {
            lhs: TempId(0),
            rhs: AmirRvalue::Binary {
                op: BinaryOp::Sub,
                left: AmirOperand::Copy(TempId(1)),
                right: AmirOperand::Copy(TempId(2)),
            },
        },
    );
    let units = Units(vec![unit(f, &types, literals)]);
    let mut limited = budget();
    limited.values = 11;
    assert_eq!(
        evaluate(&units, root, &[value(10), value(20)], limited, || false),
        Ok(value(10))
    );
    limited.values = 10;
    assert_eq!(
        evaluate(&units, root, &[value(10), value(20)], limited, || false)
            .expect_err("edge scratch")
            .kind,
        EvalErrorKind::ValueLimit
    );
}

#[test]
fn malformed_dense_ids_and_missing_values_return_errors_without_panics() {
    let root = SymbolId::new(1, 0);
    let (f, types, pool) = function(root, 1);
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        )
        .expect_err("return register uninitialized")
        .kind,
        EvalErrorKind::Uninitialized
    );
    let (mut f, types, pool) = function(root, 1);
    f.blocks[0].statements = DenseRange {
        start: u32::MAX,
        len: u32::MAX,
    };
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        )
        .expect_err("invalid statement range")
        .kind,
        EvalErrorKind::InvalidIr
    );
    let (mut f, types, pool) = function(root, 1);
    f.blocks[0].terminator = AmirTerminator::Goto {
        target: BlockId(u32::MAX),
        args: vec![],
    };
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        )
        .expect_err("invalid block")
        .kind,
        EvalErrorKind::InvalidIr
    );
    let (mut f, types, pool) = function(root, 1);
    f.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Assign {
            lhs: TempId(u32::MAX),
            rhs: AmirRvalue::Use(AmirOperand::Copy(TempId(u32::MAX))),
        },
    );
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        )
        .expect_err("invalid temp")
        .kind,
        EvalErrorKind::InvalidIr
    );
}

#[test]
fn forbidden_effect_in_an_unentered_block_is_rejected_before_execution() {
    let root = SymbolId::new(1, 0);
    let (mut f, types, mut pool) = function(root, 1);
    let forty_two = AmirOperand::Constant(AmirConstant::Pool(pool.intern_int("42")));
    f.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Assign {
            lhs: TempId(0),
            rhs: AmirRvalue::Use(forty_two),
        },
    );
    f.blocks.push(block(1));
    f.append_stmt_to_block(BlockId(1), AmirStmt::Free(forty_two));
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        )
        .expect_err("unreachable effect")
        .kind,
        EvalErrorKind::UnsupportedOperation
    );
}

#[test]
fn checked_arithmetic_failure_retains_the_function_and_span() {
    let root = SymbolId::new(1, 0);
    let (mut f, types, mut pool) = function(root, 2);
    f.params = vec![TempId(1)];
    let one = AmirOperand::Constant(AmirConstant::Pool(pool.intern_int("1")));
    f.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Assign {
            lhs: TempId(0),
            rhs: AmirRvalue::Binary {
                op: BinaryOp::Div,
                left: one,
                right: AmirOperand::Copy(TempId(1)),
            },
        },
    );
    let error = evaluate(
        &Units(vec![unit(f, &types, pool)]),
        root,
        &[value(0)],
        budget(),
        || false,
    )
    .expect_err("division by zero");
    assert_eq!(
        error.kind,
        EvalErrorKind::Arithmetic(ScalarEvalError::DivisionByZero)
    );
    assert_eq!(error.function, root);
    assert_eq!(error.span, Span::new(1, 1, 2));
}

#[test]
fn scalar_locals_are_initialized_and_cleared_by_storage_boundaries() {
    let root = SymbolId::new(1, 0);
    for boundary in [
        None,
        Some(AmirStmt::StorageDead(LocalId(0))),
        Some(AmirStmt::StorageLive(LocalId(0))),
    ] {
        let (mut f, types, mut pool) = function(root, 1);
        f.locals.push(AmirLocal {
            id: LocalId(0),
            ty: f.return_type,
            is_memory: false,
            symbol: None,
            span: Span::new(1, 1, 2),
            use_span: None,
        });
        let place = AmirPlace {
            local: LocalId(0),
            projections: Default::default(),
        };
        f.append_stmt_to_block(BlockId(0), AmirStmt::StorageLive(LocalId(0)));
        f.append_stmt_to_block(
            BlockId(0),
            AmirStmt::Store {
                lhs: place.clone(),
                rhs: AmirOperand::Constant(AmirConstant::Pool(pool.intern_int("42"))),
            },
        );
        let cleared = boundary.is_some();
        if let Some(boundary) = boundary {
            f.append_stmt_to_block(BlockId(0), boundary);
        }
        f.append_stmt_to_block(
            BlockId(0),
            AmirStmt::Assign {
                lhs: TempId(0),
                rhs: AmirRvalue::Load(place),
            },
        );
        let result = evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false,
        );
        if cleared {
            assert_eq!(
                result.expect_err("storage boundary clears the value").kind,
                EvalErrorKind::Uninitialized
            );
        } else {
            assert_eq!(result, Ok(value(42)));
        }
    }
}

#[test]
fn switch_selects_only_the_matching_edge_or_otherwise() {
    let root = SymbolId::new(1, 0);
    let (mut f, types, pool) = function(root, 2);
    f.params = vec![TempId(1)];
    f.blocks.extend([block(1), block(2)]);
    f.block_params.push(BlockParam {
        id: TempId(0),
        local: LocalId(0),
        ty: f.return_type,
        from: None,
        moved: false,
    });
    for index in [1, 2] {
        f.blocks[index].params = DenseRange::new(0, 1);
    }
    f.blocks[0].terminator = AmirTerminator::SwitchInt {
        discriminant: AmirOperand::Copy(TempId(1)),
        targets: vec![(7, BlockId(1), vec![AmirOperand::Copy(TempId(1))])],
        // This argument is uninitialized and must not be read on the first edge.
        otherwise: (BlockId(2), vec![AmirOperand::Copy(TempId(0))]),
    };
    let units = Units(vec![unit(f, &types, pool)]);
    assert_eq!(
        evaluate(&units, root, &[value(7)], budget(), || false),
        Ok(value(7))
    );
    assert_eq!(
        evaluate(&units, root, &[value(8)], budget(), || false)
            .expect_err("otherwise argument")
            .kind,
        EvalErrorKind::Uninitialized
    );
}

#[test]
fn bool_void_and_target_mismatch_are_handled_without_host_assumptions() {
    let root = SymbolId::new(1, 0);
    let (mut f, types, pool) = function(root, 1);
    let boolean = types.intern(ArType::Primitive(Primitive::Bool));
    f.return_type = boolean;
    f.temps[0].ty = boolean;
    f.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Assign {
            lhs: TempId(0),
            rhs: AmirRvalue::Use(AmirOperand::Constant(AmirConstant::Bool(true))),
        },
    );
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        ),
        Ok(ConstValue::Bool(true))
    );
    let (mut f, types, pool) = function(root, 0);
    f.return_type = types.intern(ArType::Void);
    assert_eq!(
        evaluate(
            &Units(vec![unit(f, &types, pool)]),
            root,
            &[],
            budget(),
            || false
        ),
        Ok(ConstValue::Void)
    );

    let child = SymbolId::new(1, 1);
    let (mut f, types, pool) = function(root, 1);
    f.append_stmt_to_block(
        BlockId(0),
        AmirStmt::Call {
            lhs: Some(TempId(0)),
            callee: AmirOperand::FunctionRef(child),
            args: Default::default(),
            return_borrow: None,
        },
    );
    let (g, child_types, child_pool) = function(child, 1);
    let child = Arc::new(
        CtfeFunction::new(g, child_pool, &child_types, DataLayout::ptr_width(4))
            .expect("ptr4 unit"),
    );
    let units = Units(vec![unit(f, &types, pool), child]);
    assert_eq!(
        evaluate(&units, root, &[], budget(), || false)
            .expect_err("different callee target")
            .kind,
        EvalErrorKind::TargetMismatch
    );
}

#[test]
fn repeated_literal_decoding_is_charged_and_can_be_cancelled_mid_spelling() {
    let root = SymbolId::new(1, 0);
    let (mut f, types, mut pool) = function(root, 1);
    let spelling = "0000000000000042";
    let constant = AmirOperand::Constant(AmirConstant::Pool(pool.intern_int(spelling)));
    for _ in 0..2 {
        f.append_stmt_to_block(
            BlockId(0),
            AmirStmt::Assign {
                lhs: TempId(0),
                rhs: AmirRvalue::Use(constant),
            },
        );
    }
    let units = Units(vec![unit(f, &types, pool)]);
    let mut exact = budget();
    // Root lookup, block/statements/temp admission, two instructions, return,
    // and three scans of the spelling (admission plus each decoding).
    exact.fuel = 8 + 3 * spelling.len() as u64;
    assert_eq!(evaluate(&units, root, &[], exact, || false), Ok(value(42)));
    exact.fuel -= 1;
    assert_eq!(
        evaluate(&units, root, &[], exact, || false)
            .expect_err("one step short")
            .kind,
        EvalErrorKind::FuelExhausted
    );
    let mut polls = 0;
    assert_eq!(
        evaluate(&units, root, &[], budget(), || {
            polls += 1;
            polls == 5 + spelling.len() + 5
        })
        .expect_err("cancel during first decoding")
        .kind,
        EvalErrorKind::Cancelled
    );
    assert!(polls > spelling.len());
}
