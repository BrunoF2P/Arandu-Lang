//! The declaration closure used by both object emission and CGU hashing.
//! Bodies of callees are deliberately not dependencies of an object caller.

use arandu_semantics::amir::{AmirFunc, AmirOperand, AmirProgram, AmirRvalue, AmirStmt};
use arandu_semantics::types::{ArType, TypeShape};
use arandu_semantics::{Diagnostic, EnumPayloadShape, SymbolId, SymbolTable, TypeInfo};
use rustc_hash::FxHashSet;

use crate::jit::isa::codegen_ice;

pub(crate) fn function_dependencies(
    function: &AmirFunc,
    program: &AmirProgram,
    symbols: &SymbolTable,
    info: &TypeInfo,
) -> Result<Vec<SymbolId>, Diagnostic> {
    let mut referenced = FxHashSet::default();
    let mut visit = |operand: &AmirOperand| {
        if let AmirOperand::FunctionRef(symbol) = operand {
            referenced.insert(*symbol);
        }
    };
    let mut types = vec![function.return_type];
    types.extend(function.temps.iter().map(|temp| temp.ty));
    types.extend(function.locals.iter().map(|local| local.ty));
    types.extend(function.block_params.iter().map(|param| param.ty));
    for id in function.stmts.iter_ids() {
        if let Some(statement) = function.stmts.get(id) {
            arandu_semantics::amir::visit::for_each_stmt_operand(statement, &mut visit);
            if let AmirStmt::Assign { rhs, .. } = statement {
                // Backend-generated drop shims are not explicit AMIR calls.
                // Include their payload even when it is not a result temp.
                match rhs {
                    AmirRvalue::GenInsert { payload_ty, .. }
                    | AmirRvalue::GenSet { payload_ty, .. }
                    | AmirRvalue::GenUpsert { payload_ty, .. } => types.push(*payload_ty),
                    AmirRvalue::Use(_)
                    | AmirRvalue::Binary { .. }
                    | AmirRvalue::Unary { .. }
                    | AmirRvalue::FieldAccess { .. }
                    | AmirRvalue::IndexAccess { .. }
                    | AmirRvalue::StructLiteral { .. }
                    | AmirRvalue::Array { .. }
                    | AmirRvalue::Tuple { .. }
                    | AmirRvalue::EnumConstruct { .. }
                    | AmirRvalue::Discriminant { .. }
                    | AmirRvalue::EnumPayload { .. }
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
                    | AmirRvalue::RelativeBorrow { .. }
                    | AmirRvalue::CoroutineReady { .. }
                    | AmirRvalue::GenGet { .. }
                    | AmirRvalue::GenRemove { .. }
                    | AmirRvalue::StringInterp { .. }
                    | AmirRvalue::ToStr { .. }
                    | AmirRvalue::BlackBox { .. } => {}
                }
            }
        }
    }
    for block in &function.blocks {
        arandu_semantics::amir::for_each_terminator_operand(&block.terminator, &mut visit);
    }
    let mut visited = FxHashSet::default();
    while let Some(ty) = types.pop() {
        if !visited.insert(ty) {
            continue;
        }
        if visited.len() > 4096 || TypeShape::from_id(ty, &info.type_interner).is_err() {
            return Err(codegen_ice(
                "CGU type dependencies exceed their structural bounds",
            ));
        }
        if let Some(&destructor) = info.destructor_instances.get(&ty) {
            referenced.insert(destructor);
        }
        match info.type_interner.resolve(ty) {
            nominal @ ArType::Named(symbol, arguments) => {
                let args = info.type_interner.type_args(arguments);
                types.extend_from_slice(&args);
                if let Some(fields) = info.fields_for(symbol, &args) {
                    for field in fields.iter() {
                        if let Some(ty) = arandu_semantics::layout::instantiated_field_type(
                            &nominal,
                            &field.name,
                            &info.type_interner,
                            info,
                        ) {
                            types.push(ty);
                        }
                    }
                }
                let subst = info
                    .generic_params
                    .get(&symbol)
                    .map(|params| {
                        arandu_semantics::types::build_subst_ids(params, &args, &info.type_interner)
                    })
                    .unwrap_or_default();
                for (variant, (parent, _)) in &info.enum_variants {
                    if *parent == symbol
                        && let Some(EnumPayloadShape::Tuple(fields)) =
                            info.variant_payload_for(*variant, &args)
                    {
                        types.extend(fields.iter().map(|&field| {
                            arandu_semantics::types::substitute_type_id(
                                field,
                                &subst,
                                &info.type_interner,
                            )
                        }));
                    }
                }
            }
            ArType::Func(arguments, result) => {
                types.extend(info.type_interner.type_args(arguments));
                types.push(result);
            }
            ArType::Tuple(arguments) => types.extend(info.type_interner.type_args(arguments)),
            ArType::Nullable(inner)
            | ArType::Slice(inner)
            | ArType::Array(_, inner)
            | ArType::ConstArray(_, inner)
            | ArType::Ptr(inner)
            | ArType::Ref(inner)
            | ArType::RefMut(inner)
            | ArType::Option(inner)
            | ArType::Coroutine(inner)
            | ArType::Poll(inner)
            | ArType::Range(inner) => types.push(inner),
            ArType::Result(ok, error) => types.extend([ok, error]),
            ArType::Primitive(_)
            | ArType::FrozenConst(_)
            | ArType::Const(_)
            | ArType::ConstParam(_)
            | ArType::GenRef
            | ArType::Err
            | ArType::Void
            | ArType::IntLiteral
            | ArType::FloatLiteral
            | ArType::Error => {}
        }
    }
    // Namespace aliases must select the actual declaration as well as its
    // call-site identity, just as the translator's function-name map does.
    let aliases = referenced
        .iter()
        .filter_map(|symbol| symbols.try_get(*symbol))
        .filter(|symbol| symbol.kind == arandu_semantics::SymbolKind::NamespaceMember)
        .collect::<Vec<_>>();
    for symbol in program
        .funcs
        .iter()
        .map(|func| func.symbol)
        .chain(program.extern_funcs.keys().copied())
    {
        if let Some(declaration) = symbols.try_get(symbol)
            && aliases.iter().any(|alias| {
                alias.span == declaration.span
                    && alias.name.ends_with(&format!(".{}", declaration.name))
            })
        {
            referenced.insert(symbol);
        }
    }
    let mut sorted = referenced.into_iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        let name = |id| {
            symbols
                .try_get(id)
                .map(|symbol| symbols.host_func_name(symbol))
        };
        name(*a)
            .cmp(&name(*b))
            .then_with(|| (a.file_id, a.local_id.0).cmp(&(b.file_id, b.local_id.0)))
    });
    Ok(sorted)
}

pub(crate) fn declaration_closure(
    program: &AmirProgram,
    symbols: &SymbolTable,
    info: &TypeInfo,
    definitions: &[SymbolId],
) -> Result<FxHashSet<SymbolId>, Diagnostic> {
    let mut declarations = definitions.iter().copied().collect::<FxHashSet<_>>();
    for symbol in definitions {
        let function = program
            .funcs
            .iter()
            .find(|function| function.symbol == *symbol)
            .ok_or_else(|| codegen_ice("CGU definition has no AMIR body"))?;
        declarations.extend(function_dependencies(function, program, symbols, info)?);
    }
    Ok(declarations)
}

/// Stable local object name: composition-local numeric IDs are not ABI names.
pub(crate) fn drop_shim_name(symbols: &SymbolTable, destructor: SymbolId) -> Option<String> {
    symbols
        .try_get(destructor)
        .map(|symbol| format!("__ar_drop_{}", symbols.host_func_name(symbol)))
}
