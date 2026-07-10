use crate::amir::{
    AmirConstant, AmirFunc, AmirOperand, AmirRvalue, AmirStmt, AmirTerminator, BlockId, InstrId,
};
use crate::literal_pool::{AmirLiteralEntry, AmirLiteralPool};
use crate::ops::{BinaryOp, UnaryOp};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LatticeVal {
    Undefined,
    Constant(AmirConstant),
    Overdefined,
}

/// Sparse Conditional Constant Propagation (Wegman & Zadeck 1991).
///
/// Propagates constants across basic blocks and eliminates dead branches
/// by jointly modeling value constness and CFG reachability in a single
/// fixpoint iteration.  This subsumes intra-block constant folding and
/// adds cross-block propagation + branch pruning.
pub(super) fn sccp(func: &mut AmirFunc, pool: &mut AmirLiteralPool) -> bool {
    let n_temps = func.temps.len();
    let n_blocks = func.blocks.len();
    if n_temps == 0 || n_blocks == 0 {
        return false;
    }

    // Phase 1 – analyse lattice + reachability to fixpoint
    let (lattice, sccp_reachable) = analyse(func, pool);

    // Phase 2 – apply results to the function
    let changed = apply(func, pool, &lattice);

    // If SCCP proved a block dead that is statically reachable from the CFG,
    // the outer pipeline needs to re-run SimplifyCFG even if apply found
    // nothing to rewrite (e.g. the terminator was already a Goto from a
    // prior call).
    if !changed {
        let static_reachable = raw_cfg_reachable(func);
        for i in 0..n_blocks {
            if static_reachable[i] && !sccp_reachable[i] {
                return true;
            }
        }
    }

    changed
}

/// BFS from entry block following `func.cfg` successor edges (ignoring
/// condition values).  Every block that has at least one predecessor in
/// the static CFG is considered statically reachable.
fn raw_cfg_reachable(func: &AmirFunc) -> Vec<bool> {
    let n = func.blocks.len();
    let mut reachable = vec![false; n];
    if n == 0 {
        return reachable;
    }
    let mut queue = VecDeque::new();
    reachable[0] = true;
    queue.push_back(BlockId::from_usize(0));
    while let Some(bid) = queue.pop_front() {
        for succ in func.successors(bid) {
            let sidx = succ.as_usize();
            if sidx < n && !reachable[sidx] {
                reachable[sidx] = true;
                queue.push_back(*succ);
            }
        }
    }
    reachable
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

fn analyse(func: &AmirFunc, pool: &mut AmirLiteralPool) -> (Vec<LatticeVal>, Vec<bool>) {
    let n_temps = func.temps.len();
    let n_blocks = func.blocks.len();
    let mut lattice = vec![LatticeVal::Undefined; n_temps];
    let mut reachable = vec![false; n_blocks];
    reachable[0] = true;

    // RPO once – covers all statically reachable blocks.
    let rpo = crate::amir::reverse_post_order(func);

    loop {
        let mut changed = false;

        for &bid in &rpo {
            if !reachable[bid.as_usize()] {
                continue;
            }

            // --- statements -------------------------------------------------
            for stmt_id in func.block_stmt_ids(bid) {
                let stmt = func.stmt(stmt_id);
                if let AmirStmt::Assign { lhs, rhs } = stmt {
                    let old = lattice[lhs.as_usize()];
                    // `T?` is a null-or-pointer handle. Scalar constants like `0`
                    // are *not* the same as `nil` at runtime (they get boxed).
                    // Never fold a non-Nil constant into a Nullable temp.
                    let new = if temp_is_nullable(func, *lhs) {
                        match eval_rvalue(rhs, &lattice, pool) {
                            LatticeVal::Constant(AmirConstant::Nil) => {
                                LatticeVal::Constant(AmirConstant::Nil)
                            }
                            LatticeVal::Undefined => LatticeVal::Undefined,
                            _ => LatticeVal::Overdefined,
                        }
                    } else {
                        eval_rvalue(rhs, &lattice, pool)
                    };
                    let merged = meet(old, new);
                    if merged != old {
                        lattice[lhs.as_usize()] = merged;
                        changed = true;
                    }
                }
            }

            // --- terminator (may mark more blocks reachable) -----------------
            let term = &func.block(bid).terminator;
            changed |= propagate_branches(term, &lattice, pool, &mut reachable);
        }

        if !changed {
            break;
        }
    }

    (lattice, reachable)
}

// ---------------------------------------------------------------------------
// Transformation
// ---------------------------------------------------------------------------

fn apply(func: &mut AmirFunc, pool: &mut AmirLiteralPool, lattice: &[LatticeVal]) -> bool {
    let mut changed = false;
    for bi in 0..func.blocks.len() {
        let bid = BlockId::from_usize(bi);

        // Fold every assign whose lhs is a proven constant.
        let stmt_ids: Vec<InstrId> = func.block_stmt_ids(bid).collect();
        for stmt_id in stmt_ids {
            let stmt = func.stmt_mut(stmt_id);
            if let AmirStmt::Assign { lhs, rhs } = stmt
                && let LatticeVal::Constant(c) = lattice[lhs.as_usize()]
                && !matches!(rhs, AmirRvalue::Use(AmirOperand::Constant(c2)) if *c2 == c)
            {
                *rhs = AmirRvalue::Use(AmirOperand::Constant(c));
                changed = true;
            }
        }

        // Simplify terminators whose condition is a known constant.
        let term = &func.block(bid).terminator;
        if !matches!(
            term,
            AmirTerminator::Branch { .. } | AmirTerminator::SwitchInt { .. }
        ) {
            continue;
        }
        if let Some(new_term) = try_simplify_terminator(term, lattice, pool) {
            func.block_mut(bid).terminator = new_term;
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// Lattice helpers
// ---------------------------------------------------------------------------

/// Greatest-lower-bound (meet) in the constant-propagation lattice.
///
/// Undefined ⊓ x  = x
/// x ⊓ Undefined  = x
/// Overdefined ⊓ _ = Overdefined
/// _ ⊓ Overdefined = Overdefined
/// Constant(a) ⊓ Constant(b) = Constant(a) when a == b else Overdefined
fn meet(a: LatticeVal, b: LatticeVal) -> LatticeVal {
    use LatticeVal::*;
    match (a, b) {
        (Undefined, x) | (x, Undefined) => x,
        (Overdefined, _) | (_, Overdefined) => Overdefined,
        (Constant(a), Constant(b)) if a == b => Constant(a),
        (Constant(_), Constant(_)) => Overdefined,
    }
}

// ---------------------------------------------------------------------------
// Rvalue evaluation
// ---------------------------------------------------------------------------

fn temp_is_nullable(func: &AmirFunc, temp: crate::amir::TempId) -> bool {
    func.temps[temp.as_usize()].is_nullable
}

fn eval_rvalue(
    rvalue: &AmirRvalue,
    lattice: &[LatticeVal],
    pool: &mut AmirLiteralPool,
) -> LatticeVal {
    match rvalue {
        AmirRvalue::Use(op) => operand_lattice(op, lattice),
        AmirRvalue::Unary { op, operand } => match operand_lattice(operand, lattice) {
            LatticeVal::Constant(c) => fold_unary(*op, c, pool)
                .map(LatticeVal::Constant)
                .unwrap_or(LatticeVal::Overdefined),
            LatticeVal::Overdefined => LatticeVal::Overdefined,
            LatticeVal::Undefined => LatticeVal::Undefined,
        },
        AmirRvalue::Binary { op, left, right } => {
            let l = operand_lattice(left, lattice);
            let r = operand_lattice(right, lattice);
            match (l, r) {
                (LatticeVal::Constant(lc), LatticeVal::Constant(rc)) => {
                    fold_binary(*op, lc, rc, pool)
                        .map(LatticeVal::Constant)
                        .unwrap_or(LatticeVal::Overdefined)
                }
                (LatticeVal::Overdefined, _) | (_, LatticeVal::Overdefined) => {
                    LatticeVal::Overdefined
                }
                _ => LatticeVal::Undefined,
            }
        }
        // Everything else (Load, Borrow, FieldAccess, Alloc, Call, etc.)
        // is treated as non-foldable.
        _ => LatticeVal::Overdefined,
    }
}

fn operand_lattice(op: &AmirOperand, lattice: &[LatticeVal]) -> LatticeVal {
    match op {
        AmirOperand::Constant(c) => LatticeVal::Constant(*c),
        AmirOperand::Copy(t) | AmirOperand::Move(t) => lattice[t.as_usize()],
        AmirOperand::FunctionRef(_) | AmirOperand::GlobalRef(_) => LatticeVal::Overdefined,
    }
}

// ---------------------------------------------------------------------------
// Branch propagation
// ---------------------------------------------------------------------------

/// Marks successors as reachable based on the terminator and known lattice values.
/// Returns `true` if any block was newly marked.
fn propagate_branches(
    terminator: &AmirTerminator,
    lattice: &[LatticeVal],
    pool: &AmirLiteralPool,
    reachable: &mut [bool],
) -> bool {
    let mut changed = false;
    match terminator {
        AmirTerminator::Branch {
            condition,
            if_true,
            if_false,
            ..
        } => match operand_lattice(condition, lattice) {
            LatticeVal::Constant(AmirConstant::Bool(true)) => {
                changed |= set_reachable(*if_true, reachable);
            }
            LatticeVal::Constant(AmirConstant::Bool(false)) => {
                changed |= set_reachable(*if_false, reachable);
            }
            _ => {
                changed |= set_reachable(*if_true, reachable);
                changed |= set_reachable(*if_false, reachable);
            }
        },
        AmirTerminator::Goto { target, .. } => {
            changed |= set_reachable(*target, reachable);
        }
        AmirTerminator::SwitchInt {
            discriminant,
            targets,
            otherwise,
        } => match operand_lattice(discriminant, lattice) {
            LatticeVal::Constant(c) => {
                if let Some(val) = const_as_i128(&c, pool) {
                    let mut matched = false;
                    for (tv, tb, _) in targets {
                        if *tv == val {
                            changed |= set_reachable(*tb, reachable);
                            matched = true;
                            break;
                        }
                    }
                    if !matched {
                        changed |= set_reachable(otherwise.0, reachable);
                    }
                } else {
                    for (_, tb, _) in targets {
                        changed |= set_reachable(*tb, reachable);
                    }
                    changed |= set_reachable(otherwise.0, reachable);
                }
            }
            _ => {
                for (_, tb, _) in targets {
                    changed |= set_reachable(*tb, reachable);
                }
                changed |= set_reachable(otherwise.0, reachable);
            }
        },
        AmirTerminator::Return | AmirTerminator::Unreachable => {}
    }
    changed
}

fn set_reachable(block: BlockId, reachable: &mut [bool]) -> bool {
    let idx = block.as_usize();
    if idx < reachable.len() && !reachable[idx] {
        reachable[idx] = true;
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Terminator simplification
// ---------------------------------------------------------------------------

/// When a branch condition (or switch discriminant) is a known constant,
/// return a single `Goto`. Otherwise `None` (no clone of the original).
fn try_simplify_terminator(
    terminator: &AmirTerminator,
    lattice: &[LatticeVal],
    pool: &AmirLiteralPool,
) -> Option<AmirTerminator> {
    match terminator {
        AmirTerminator::Branch {
            condition,
            if_true,
            true_args,
            if_false,
            false_args,
        } => match operand_lattice(condition, lattice) {
            LatticeVal::Constant(AmirConstant::Bool(true)) => Some(AmirTerminator::Goto {
                target: *if_true,
                args: true_args.clone(),
            }),
            LatticeVal::Constant(AmirConstant::Bool(false)) => Some(AmirTerminator::Goto {
                target: *if_false,
                args: false_args.clone(),
            }),
            _ => None,
        },
        AmirTerminator::SwitchInt {
            discriminant,
            targets,
            otherwise,
        } => match operand_lattice(discriminant, lattice) {
            LatticeVal::Constant(c) => {
                let val = const_as_i128(&c, pool)?;
                for (tv, tb, args) in targets {
                    if *tv == val {
                        return Some(AmirTerminator::Goto {
                            target: *tb,
                            args: args.clone(),
                        });
                    }
                }
                Some(AmirTerminator::Goto {
                    target: otherwise.0,
                    args: otherwise.1.clone(),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Constant-folding helpers (adapted from optimize.rs)
// ---------------------------------------------------------------------------

fn fold_unary(
    op: UnaryOp,
    value: AmirConstant,
    pool: &mut AmirLiteralPool,
) -> Option<AmirConstant> {
    match (op, value) {
        (UnaryOp::Not, AmirConstant::Bool(b)) => Some(AmirConstant::Bool(!b)),
        (UnaryOp::Neg, v) => {
            let val = const_as_i128(&v, pool)?;
            Some(AmirConstant::Pool(pool.intern_int((-val).to_string())))
        }
        (UnaryOp::BitNot, v) => {
            let val = const_as_i128(&v, pool)?;
            Some(AmirConstant::Pool(pool.intern_int((!val).to_string())))
        }
        (UnaryOp::Await | UnaryOp::Ref | UnaryOp::RefMut | UnaryOp::Deref, _) => None,
        _ => None,
    }
}

fn fold_binary(
    op: BinaryOp,
    left: AmirConstant,
    right: AmirConstant,
    pool: &mut AmirLiteralPool,
) -> Option<AmirConstant> {
    match (left, right) {
        (AmirConstant::Bool(a), AmirConstant::Bool(b)) => match op {
            BinaryOp::And => Some(AmirConstant::Bool(a && b)),
            BinaryOp::Or => Some(AmirConstant::Bool(a || b)),
            BinaryOp::Equal => Some(AmirConstant::Bool(a == b)),
            BinaryOp::NotEqual => Some(AmirConstant::Bool(a != b)),
            _ => None,
        },
        (l, r) => {
            let lv = const_as_i128(&l, pool)?;
            let rv = const_as_i128(&r, pool)?;
            match op {
                BinaryOp::Add => checked_int(lv.checked_add(rv), pool),
                BinaryOp::Sub => checked_int(lv.checked_sub(rv), pool),
                BinaryOp::Mul => checked_int(lv.checked_mul(rv), pool),
                BinaryOp::Div if rv != 0 => checked_int(lv.checked_div(rv), pool),
                BinaryOp::Mod if rv != 0 => checked_int(lv.checked_rem(rv), pool),
                BinaryOp::BitOr => Some(int_const(lv | rv, pool)),
                BinaryOp::BitXor => Some(int_const(lv ^ rv, pool)),
                BinaryOp::BitAnd => Some(int_const(lv & rv, pool)),
                BinaryOp::ShiftLeft if (0..128).contains(&rv) => {
                    checked_int(lv.checked_shl(rv as u32), pool)
                }
                BinaryOp::ShiftRight if (0..128).contains(&rv) => {
                    checked_int(lv.checked_shr(rv as u32), pool)
                }
                BinaryOp::Equal => Some(AmirConstant::Bool(lv == rv)),
                BinaryOp::NotEqual => Some(AmirConstant::Bool(lv != rv)),
                BinaryOp::Lt => Some(AmirConstant::Bool(lv < rv)),
                BinaryOp::Gt => Some(AmirConstant::Bool(lv > rv)),
                BinaryOp::LtEqual => Some(AmirConstant::Bool(lv <= rv)),
                BinaryOp::GtEqual => Some(AmirConstant::Bool(lv >= rv)),
                _ => None,
            }
        }
    }
}

fn const_as_i128(c: &AmirConstant, pool: &AmirLiteralPool) -> Option<i128> {
    match c {
        AmirConstant::Pool(id) => match pool.get(*id) {
            AmirLiteralEntry::Int(val) => arandu_middle::literal_pool::parse_int_literal(val),
            AmirLiteralEntry::Float(_) | AmirLiteralEntry::Str(_) | AmirLiteralEntry::Char(_) => {
                None
            }
        },
        AmirConstant::Bool(_) | AmirConstant::Nil => None,
    }
}

fn int_const(val: i128, pool: &mut AmirLiteralPool) -> AmirConstant {
    AmirConstant::Pool(pool.intern_int(val.to_string()))
}

fn checked_int(val: Option<i128>, pool: &mut AmirLiteralPool) -> Option<AmirConstant> {
    val.map(|v| int_const(v, pool))
}
