//! Shared lifetime admission for invocation-owned aggregate scratch storage.
//! Target backends supply their result ABI, not a second ownership checker.
use arandu_middle::amir::{AmirFunc, AmirRvalue, AmirStmt, AmirTerminator};
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
                AmirStmt::Call { callee, .. } => match callee {
                    arandu_middle::amir::AmirOperand::FunctionRef(symbol) => symbols
                        .try_get(*symbol)
                        .is_some_and(|symbol| symbol.kind != SymbolKind::ExternFunc),
                    arandu_middle::amir::AmirOperand::Copy(_)
                    | arandu_middle::amir::AmirOperand::Move(_)
                    | arandu_middle::amir::AmirOperand::Constant(_)
                    | arandu_middle::amir::AmirOperand::GlobalRef(_) => false,
                },
                // Raw allocation pointers are pointees/resources, not an
                // aggregate's hidden backing cell. Never admit Free(aggregate).
                AmirStmt::Free(operand) => match operand {
                    arandu_middle::amir::AmirOperand::Copy(temp)
                    | arandu_middle::amir::AmirOperand::Move(temp) => function
                        .temps
                        .get(temp.as_usize())
                        .is_some_and(|temp| matches!(interner.resolve(temp.ty), ArType::Ptr(_))),
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
mod tests {
    use super::*;
    use arandu_middle::SymbolId;
    use arandu_typeck::TypeInfo;

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
