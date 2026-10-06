//! Shared lifetime admission for invocation-owned aggregate scratch storage.
//! Target backends supply their result ABI, not a second ownership checker.
use arandu_middle::amir::{AmirFunc, AmirOperand, AmirRvalue, AmirStmt, AmirTerminator, TempId};
use arandu_middle::layout::{StructLayoutProvider, instantiated_field_type};
use arandu_middle::types::{ArType, TypeInterner};
use arandu_middle::types::{Primitive, TypeId};
use arandu_middle::{SymbolKind, SymbolTable};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScratchResultAbi {
    ScalarsOnly,
    CallerOwnedAggregates,
    /// The backend transfers an owned result into distinct backing before
    /// reclaiming invocation storage; affine payload ownership follows the
    /// already elaborated AMIR moves and drops, not a second destructor.
    TransferredAggregates,
}

/// Admit backing independently of Copy, but never a safe view or a suspended
/// state hidden inside an aggregate. Raw pointers are ownership handles (or
/// unsafe pointers), not safe borrows; their pointees are not this backing.
#[must_use]
pub fn scratch_type_safe(
    ty: TypeId,
    interner: &TypeInterner,
    provider: &dyn StructLayoutProvider,
) -> bool {
    fn visit(
        ty: TypeId,
        interner: &TypeInterner,
        provider: &dyn StructLayoutProvider,
        depth: usize,
    ) -> bool {
        if depth > 64 {
            return false;
        }
        match interner.resolve(ty) {
            ArType::Primitive(p) => p != Primitive::Str,
            ArType::Void | ArType::Ptr(_) => true,
            ArType::Array(_, inner) | ArType::Option(inner) | ArType::Poll(inner) => {
                visit(inner, interner, provider, depth + 1)
            }
            ArType::Result(ok, err) => {
                visit(ok, interner, provider, depth + 1)
                    && visit(err, interner, provider, depth + 1)
            }
            ArType::Tuple(args) => interner
                .type_args(args)
                .iter()
                .all(|&inner| visit(inner, interner, provider, depth + 1)),
            owner @ ArType::Named(symbol, _) => {
                if let Some(fields) = provider.get_struct_fields(symbol) {
                    fields.iter().all(|field| {
                        instantiated_field_type(&owner, field.name.as_str(), interner, provider)
                            .is_some_and(|inner| visit(inner, interner, provider, depth + 1))
                    })
                } else {
                    provider.get_enum_variants(symbol).is_some_and(|variants| {
                        variants.iter().all(|variant| {
                            variant
                                .payload_ty
                                .is_none_or(|inner| visit(inner, interner, provider, depth + 1))
                        })
                    })
                }
            }
            ArType::Slice(_)
            | ArType::Nullable(_)
            | ArType::Ref(_)
            | ArType::RefMut(_)
            | ArType::Coroutine(_)
            | ArType::Range(_)
            | ArType::Func(..)
            | ArType::GenRef
            | ArType::ConstArray(..)
            | ArType::Const(_)
            | ArType::ConstParam(_)
            | ArType::IntLiteral
            | ArType::FloatLiteral
            | ArType::Err
            | ArType::Error => false,
        }
    }
    visit(ty, interner, provider, 0)
}

/// Whether an opaque (extern) callee could observe the address of backing
/// this function promotes.
///
/// A static frame slot is reused on the next call/iteration, so a retained
/// pointer would alias fresh contents. Scalars, raw ownership handles, and
/// static/value operands travel as values and cannot name this frame;
/// by-address aggregates, safe borrows, and slices (whose `data` can alias a
/// promoted array literal) can. A slice would be a fat `{data, len}` pair, but
/// admitting it would let a retained `data` point into reused backing, so it
/// stays with the aggregates.
fn extern_exposes_backing(
    function: &AmirFunc,
    lhs: Option<TempId>,
    args: &[AmirOperand],
    returns_borrow: bool,
    interner: &TypeInterner,
) -> bool {
    fn by_address(ty: TypeId, interner: &TypeInterner) -> bool {
        !matches!(
            interner.resolve(ty),
            ArType::Primitive(_)
                | ArType::Ptr(_)
                | ArType::Void
                | ArType::IntLiteral
                | ArType::FloatLiteral
        )
    }
    fn temp_exposes(function: &AmirFunc, interner: &TypeInterner, temp: TempId) -> bool {
        function
            .temps
            .get(temp.as_usize())
            .is_none_or(|temp| by_address(temp.ty, interner))
    }
    returns_borrow
        || lhs.is_some_and(|temp| temp_exposes(function, interner, temp))
        || args.iter().any(|arg| match arg {
            AmirOperand::Copy(temp) | AmirOperand::Move(temp) => {
                temp_exposes(function, interner, *temp)
            }
            // Function values can carry an environment; the rest name static
            // or literal data, never this frame's storage.
            AmirOperand::FunctionRef(_) => true,
            AmirOperand::Constant(_) | AmirOperand::GlobalRef(_) => false,
        })
}

#[must_use]
pub fn function_scratch_safe(
    function: &AmirFunc,
    symbols: &SymbolTable,
    interner: &TypeInterner,
    result_abi: ScratchResultAbi,
) -> bool {
    // Back-edge aggregate phi values may retain a previous iteration's
    // materialization. Without slot liveness/coloring, a static site must not
    // overwrite or reclaim that backing while the phi still references it.
    if function.block_params.iter().any(|parameter| {
        !matches!(interner.resolve(parameter.ty),
        ArType::Primitive(p) if p != Primitive::Str)
    }) {
        return false;
    }
    // A called async function can construct the state on behalf of this
    // function; detecting only local CoroutineReady instructions is too weak.
    let scalar_ready_result = match interner.resolve(function.return_type) {
        ArType::Coroutine(inner) if result_abi == ScratchResultAbi::CallerOwnedAggregates => {
            matches!(interner.resolve(inner), ArType::Void)
                || matches!(interner.resolve(inner), ArType::Primitive(p) if p != Primitive::Str)
        }
        _ => false,
    };
    if function.temps.iter().any(|temp| {
        matches!(interner.resolve(temp.ty), ArType::Coroutine(_))
            && (!scalar_ready_result || temp.ty != function.return_type)
    }) {
        return false;
    }
    // Pointer-returning carriers still use their legacy backing-pointer ABI.
    // Only scalars and the aggregates with caller-owned/direct returns qualify.
    match interner.resolve(function.return_type) {
        ArType::Primitive(Primitive::Str) => return false,
        ArType::Primitive(_) | ArType::Void | ArType::IntLiteral | ArType::FloatLiteral => {}
        ArType::Coroutine(_) if scalar_ready_result => {}
        ArType::Named(..)
        | ArType::Array(..)
        | ArType::Tuple(..)
        | ArType::Option(_)
        | ArType::Result(..)
        | ArType::Poll(_) => {
            if result_abi == ScratchResultAbi::ScalarsOnly {
                return false;
            }
        }
        ArType::Slice(_)
        | ArType::Nullable(_)
        | ArType::Ptr(_)
        | ArType::Ref(_)
        | ArType::RefMut(_)
        | ArType::Coroutine(_)
        | ArType::Range(_)
        | ArType::Func(..)
        | ArType::GenRef
        | ArType::ConstArray(..)
        | ArType::Const(_)
        | ArType::ConstParam(_)
        | ArType::Err
        | ArType::Error => return false,
    }
    for block in &function.blocks {
        if matches!(block.terminator, AmirTerminator::Suspend { .. }) {
            return false;
        }
        for statement in function.block_stmts(block.id) {
            let safe = match statement {
                AmirStmt::Assign { rhs, .. } => match rhs {
                    AmirRvalue::CoroutineReady { payload_ty, .. } => {
                        scalar_ready_result
                            && (matches!(interner.resolve(*payload_ty), ArType::Void)
                                || matches!(interner.resolve(*payload_ty), ArType::Primitive(p) if p != Primitive::Str))
                    }
                    AmirRvalue::RelativeBorrow { .. }
                    | AmirRvalue::GenInsert { .. }
                    | AmirRvalue::GenGet { .. }
                    | AmirRvalue::GenSet { .. }
                    | AmirRvalue::GenUpsert { .. }
                    | AmirRvalue::GenRemove { .. } => false,
                    AmirRvalue::Use(_)
                    | AmirRvalue::Binary { .. }
                    | AmirRvalue::Unary { .. }
                    | AmirRvalue::FieldAccess { .. }
                    | AmirRvalue::IndexAccess { .. }
                    | AmirRvalue::StructLiteral { .. }
                    | AmirRvalue::Array { .. }
                    | AmirRvalue::Tuple { .. }
                    | AmirRvalue::Discriminant { .. }
                    | AmirRvalue::EnumPayload { .. }
                    | AmirRvalue::EnumConstruct { .. }
                    | AmirRvalue::Len(_)
                    | AmirRvalue::SliceData(_)
                    | AmirRvalue::SliceView { .. }
                    | AmirRvalue::SliceSubslice { .. }
                    | AmirRvalue::StrBytes { .. }
                    | AmirRvalue::StrView { .. }
                    | AmirRvalue::Alloc(_)
                    | AmirRvalue::Load(_)
                    | AmirRvalue::Borrow(_)
                    | AmirRvalue::BorrowMut(_)
                    | AmirRvalue::ToStr { .. }
                    | AmirRvalue::BlackBox { .. }
                    | AmirRvalue::StringInterp { .. } => true,
                },
                AmirStmt::Call {
                    lhs,
                    callee,
                    args,
                    return_borrow,
                    ..
                } => match callee {
                    // Intrinsics are compiler-defined operations that
                    // translate to loads/stores/arithmetic or to runtime
                    // helpers with fixed semantics: they cannot retain
                    // caller storage. Other externs may retain what they are
                    // handed, so only value-passed boundary types stay
                    // admitted.
                    AmirOperand::FunctionRef(symbol) => match symbols.try_get(*symbol) {
                        Some(definition) if definition.kind == SymbolKind::ExternFunc => {
                            arandu_middle::intrinsics::IntrinsicKind::from_name(&definition.name)
                                .is_some()
                                || !extern_exposes_backing(
                                    function,
                                    *lhs,
                                    args,
                                    return_borrow.is_some(),
                                    interner,
                                )
                        }
                        Some(_) => true,
                        None => false,
                    },
                    AmirOperand::Copy(_)
                    | AmirOperand::Move(_)
                    | AmirOperand::Constant(_)
                    | AmirOperand::GlobalRef(_) => false,
                },
                // Raw allocation pointers and string buffers are
                // pointees/resources, not an aggregate's hidden backing cell.
                // `str` is never admitted backing, so `Free(str)` cannot
                // release this frame's slot. Never admit Free(aggregate).
                AmirStmt::Free(operand) => match operand {
                    AmirOperand::Copy(temp) | AmirOperand::Move(temp) => {
                        function.temps.get(temp.as_usize()).is_some_and(|temp| {
                            matches!(
                                interner.resolve(temp.ty),
                                ArType::Ptr(_) | ArType::Primitive(Primitive::Str)
                            )
                        })
                    }
                    _ => false,
                },
                AmirStmt::Store { .. }
                | AmirStmt::Destroy(_)
                | AmirStmt::StorageLive(_)
                | AmirStmt::StorageDead(_)
                | AmirStmt::Nop => true,
            };
            if !safe {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::cfg::compute_cfg_edges;
    use crate::layout::DenseRange;
    use arandu_middle::Span;
    use arandu_middle::SymbolId;
    use arandu_middle::amir::{AmirBasicBlock, AmirStmtTable, AmirTemp};
    use arandu_typeck::TypeInfo;
    use smallvec::smallvec;

    fn temp(id: usize, ty: TypeId) -> AmirTemp {
        AmirTemp {
            id: TempId::from_usize(id),
            ty,
            is_copy: true,
            is_nullable: false,
            span: Span::new(0, 10 + id as u32, 11 + id as u32),
        }
    }

    /// One-block function whose statements are built around an opaque
    /// (extern) callee, so admission rules can be exercised per call shape.
    fn shape(
        return_type: TypeId,
        temps: Vec<AmirTemp>,
        stmts: impl Fn(SymbolId) -> Vec<AmirStmt>,
    ) -> (AmirFunc, SymbolTable) {
        shape_named(return_type, "opaque", temps, stmts)
    }

    fn shape_named(
        return_type: TypeId,
        extern_name: &str,
        temps: Vec<AmirTemp>,
        stmts: impl Fn(SymbolId) -> Vec<AmirStmt>,
    ) -> (AmirFunc, SymbolTable) {
        let mut symbols = SymbolTable::new(0);
        let scope = symbols.global_scope();
        let caller = symbols
            .define(scope, "caller", SymbolKind::Func, Span::new(0, 0, 0))
            .expect("define caller");
        let opaque = symbols
            .define(
                scope,
                extern_name,
                SymbolKind::ExternFunc,
                Span::new(0, 0, 0),
            )
            .expect("define extern");
        let mut table = AmirStmtTable::new();
        let built = stmts(opaque);
        let count = built.len();
        for statement in built {
            table.push(statement);
        }
        let blocks = vec![AmirBasicBlock {
            id: arandu_middle::amir::BlockId::from_usize(0),
            statements: DenseRange::new(0, count),
            params: DenseRange::empty(),
            terminator: AmirTerminator::Return,
        }];
        let cfg = compute_cfg_edges(&blocks);
        let function = AmirFunc {
            symbol: caller,
            return_type,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps,
            blocks,
            block_params: Vec::new(),
            stmts: table,
            cfg,
        };
        (function, symbols)
    }

    fn safe(function: &AmirFunc, symbols: &SymbolTable, interner: &TypeInterner) -> bool {
        function_scratch_safe(
            function,
            symbols,
            interner,
            ScratchResultAbi::CallerOwnedAggregates,
        )
    }

    /// `Free(str)` releases a runtime buffer — never admitted backing, since
    /// `str` itself is rejected as a scratch type — and a value-passed extern
    /// boundary (`ptr` + scalars) cannot name this frame's storage. Neither
    /// may disqualify the function's aggregate scratch.
    #[test]
    fn string_frees_and_value_passed_externs_stay_admitted() {
        let interner = TypeInterner::new();
        let int = interner.intern(ArType::Primitive(Primitive::Int));
        let string = interner.intern(ArType::Primitive(Primitive::Str));
        let byte = interner.intern(ArType::Primitive(Primitive::U8));
        let pointer = interner.intern(ArType::Ptr(byte));
        let (function, symbols) = shape(
            int,
            vec![
                temp(0, string),
                temp(1, pointer),
                temp(2, int),
                temp(3, interner.intern(ArType::Primitive(Primitive::Bool))),
            ],
            |opaque| {
                vec![
                    AmirStmt::Free(AmirOperand::Copy(TempId::from_usize(0))),
                    AmirStmt::Call {
                        lhs: None,
                        callee: AmirOperand::FunctionRef(opaque),
                        args: smallvec![
                            AmirOperand::Copy(TempId::from_usize(1)),
                            AmirOperand::Copy(TempId::from_usize(2)),
                            AmirOperand::Copy(TempId::from_usize(3)),
                        ],
                        return_borrow: None,
                    },
                ]
            },
        );
        assert!(
            safe(&function, &symbols, &interner),
            "Free(str) and value-passed extern arguments must stay admitted"
        );
    }

    /// The flip side: anything that can carry the address of this frame's
    /// backing into opaque code — an aggregate argument, a safe borrow, or
    /// an aggregate result written through an sret pointer — keeps the
    /// function off scratch promotion.
    #[test]
    fn backing_handed_to_opaque_code_disables_scratch() {
        let interner = TypeInterner::new();
        let int = interner.intern(ArType::Primitive(Primitive::Int));
        let byte = interner.intern(ArType::Primitive(Primitive::U8));
        let array = interner.intern(ArType::Array(4, byte));
        let borrow = interner.intern(ArType::Ref(byte));

        let (function, symbols) = shape(int, vec![temp(0, array)], |opaque| {
            vec![AmirStmt::Call {
                lhs: None,
                callee: AmirOperand::FunctionRef(opaque),
                args: smallvec![AmirOperand::Copy(TempId::from_usize(0))],
                return_borrow: None,
            }]
        });
        assert!(
            !safe(&function, &symbols, &interner),
            "an aggregate argument hands opaque code our backing address"
        );

        let (function, symbols) = shape(int, vec![temp(0, borrow)], |opaque| {
            vec![AmirStmt::Call {
                lhs: None,
                callee: AmirOperand::FunctionRef(opaque),
                args: smallvec![AmirOperand::Copy(TempId::from_usize(0))],
                return_borrow: None,
            }]
        });
        assert!(
            !safe(&function, &symbols, &interner),
            "a safe borrow argument can be retained past slot reuse"
        );

        let (function, symbols) = shape(int, vec![temp(0, array)], |opaque| {
            vec![AmirStmt::Call {
                lhs: Some(TempId::from_usize(0)),
                callee: AmirOperand::FunctionRef(opaque),
                args: smallvec![],
                return_borrow: None,
            }]
        });
        assert!(
            !safe(&function, &symbols, &interner),
            "an aggregate result exposes the sret destination to the callee"
        );
    }

    /// The backend lowers intrinsics to fixed operations — `ptrWrite`
    /// becomes a store through the pointer, `ptrOffset` an add — so an
    /// aggregate at an intrinsic boundary never reaches opaque code. This is
    /// what keeps `vec.tryPush<T>` on scratch instead of a per-push malloc.
    #[test]
    fn intrinsic_boundaries_stay_admitted_with_aggregate_arguments() {
        let interner = TypeInterner::new();
        let int = interner.intern(ArType::Primitive(Primitive::Int));
        let byte = interner.intern(ArType::Primitive(Primitive::U8));
        let pointer = interner.intern(ArType::Ptr(byte));
        let array = interner.intern(ArType::Array(4, byte));
        let (function, symbols) = shape_named(
            int,
            "ptrWrite",
            vec![temp(0, pointer), temp(1, array)],
            |intrinsic| {
                vec![AmirStmt::Call {
                    lhs: None,
                    callee: AmirOperand::FunctionRef(intrinsic),
                    args: smallvec![
                        AmirOperand::Copy(TempId::from_usize(0)),
                        AmirOperand::Copy(TempId::from_usize(1)),
                    ],
                    return_borrow: None,
                }]
            },
        );
        assert!(
            safe(&function, &symbols, &interner),
            "an intrinsic boundary must not disqualify scratch"
        );
    }

    #[test]
    fn view_results_are_not_admitted_by_any_backing_abi() {
        let interner = TypeInterner::new();
        let byte = interner.intern(ArType::Primitive(Primitive::U8));
        let mut function = AmirFunc {
            symbol: SymbolId::new(0, 0),
            return_type: byte,
            receiver: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: Vec::new(),
            block_params: Vec::new(),
            stmts: Default::default(),
            cfg: Default::default(),
        };
        let symbols = SymbolTable::new(0);
        for abi in [
            ScratchResultAbi::ScalarsOnly,
            ScratchResultAbi::CallerOwnedAggregates,
            ScratchResultAbi::TransferredAggregates,
        ] {
            assert!(function_scratch_safe(&function, &symbols, &interner, abi));
            for ty in [
                ArType::Primitive(Primitive::Str),
                ArType::Slice(byte),
                ArType::Ref(byte),
                ArType::RefMut(byte),
                ArType::Ptr(byte),
            ] {
                function.return_type = interner.intern(ty);
                assert!(!function_scratch_safe(&function, &symbols, &interner, abi));
            }
            function.return_type = byte;
        }
    }

    #[test]
    fn nested_safe_views_are_excluded_but_raw_resource_handles_are_not() {
        let interner = TypeInterner::new();
        let info = TypeInfo::default();
        let byte = interner.intern(ArType::Primitive(Primitive::U8));
        for view in [
            ArType::Primitive(Primitive::Str),
            ArType::Slice(byte),
            ArType::Ref(byte),
            ArType::RefMut(byte),
            ArType::Coroutine(byte),
        ] {
            let inner = interner.intern(view);
            let array = interner.intern(ArType::Array(4, inner));
            let option = interner.intern(ArType::Option(array));
            assert!(!scratch_type_safe(option, &interner, &info));
        }
        let pointer = interner.intern(ArType::Ptr(byte));
        let array = interner.intern(ArType::Array(4, pointer));
        assert!(scratch_type_safe(array, &interner, &info));
    }
}
