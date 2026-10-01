//! Function-local runtime staging. Declaration HIR carries call modes,
//! constants and nominal metadata; it never lowers a sibling function body.

use crate::db::HashEq;
use crate::passes::{declaration_signatures, item_source_input, item_typing, parse, PreparedHir};
use crate::{ArandCompilerDb, SourceFile, StableHash};
use arandu_middle::types::{FunctionInstance, TypeShape};
use arandu_middle::{Diagnostic, Severity, SymbolId};

mod contracts;
mod program;
pub use contracts::{instance_contracts, runtime_unit, InstanceContracts};
pub use program::{runtime_program, RuntimeProgram};

#[derive(Debug)]
pub struct DeclarationHir {
    pub artifacts: PreparedHir,
    pub(crate) fingerprint: blake3::Hash,
}

#[salsa::interned]
#[derive(Debug)]
pub struct Instance<'db> {
    pub file: SourceFile,
    #[returns(ref)]
    pub key: FunctionInstance,
}

#[derive(Debug)]
pub struct InstanceHir {
    pub artifacts: PreparedHir,
    pub function: Option<SymbolId>,
    pub instances: Vec<(SymbolId, FunctionInstance)>,
    pub generated_symbols: Vec<SymbolId>,
}

#[derive(Debug)]
pub struct RuntimeUnit {
    pub result: Result<arandu_mir::FunctionUnit, Vec<Diagnostic>>,
    pub context: std::sync::Arc<arandu_semantics::TypeCheckResult>,
    pub instances: Vec<(SymbolId, FunctionInstance)>,
    pub generated_symbols: Vec<SymbolId>,
    pub borrow_summary: Option<arandu_middle::types::ReturnBorrowSummary>,
    /// Retains annotated flow for IDE diagnostics even when final validation
    /// fails. It is not publishable executable code; `result` owns that gate.
    pub analysis_function: Option<std::sync::Arc<arandu_middle::amir::AmirFunc>>,
}

impl StableHash for FunctionInstance {
    fn stable_hash(&self) -> blake3::Hash {
        // A canonical fresh interner is not sufficient: identical outer ranges
        // may contain different children. Hash the full structural arguments.
        let mut hash = blake3::Hasher::new();
        hash.update(b"FunctionInstance/v1");
        hash.update(&self.definition.file_id.to_le_bytes());
        hash.update(&self.definition.local_id.0.to_le_bytes());
        hash.update(&(self.arguments.len() as u64).to_le_bytes());
        for argument in &self.arguments {
            hash_shape(&mut hash, argument);
        }
        hash.finalize()
    }
}

fn hash_shape(hash: &mut blake3::Hasher, shape: &TypeShape) {
    use TypeShape::*;
    let tag = match shape {
        Primitive(_) => 0,
        Named(_, _) => 1,
        Func(_, _) => 2,
        Nullable(_) => 3,
        Slice(_) => 4,
        Array(_, _) => 5,
        ConstArray(_, _) => 6,
        Const(_) => 7,
        ConstParam(_) => 8,
        Ptr(_) => 9,
        Ref(_) => 10,
        RefMut(_) => 11,
        GenRef => 12,
        Tuple(_) => 13,
        Result(_, _) => 14,
        Option(_) => 15,
        Coroutine(_) => 16,
        Poll(_) => 17,
        Range(_) => 18,
        Err => 19,
        Void => 20,
        IntLiteral => 21,
        FloatLiteral => 22,
        Error => 23,
    };
    hash.update(&[tag]);
    match shape {
        Primitive(primitive) => {
            hash.update(&[*primitive as u8]);
        }
        Named(symbol, arguments) => {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
            hash.update(&(arguments.len() as u64).to_le_bytes());
            for argument in arguments {
                hash_shape(hash, argument);
            }
        }
        Func(arguments, result) => {
            hash.update(&(arguments.len() as u64).to_le_bytes());
            for argument in arguments {
                hash_shape(hash, argument);
            }
            hash_shape(hash, result);
        }
        Tuple(arguments) => {
            hash.update(&(arguments.len() as u64).to_le_bytes());
            for argument in arguments {
                hash_shape(hash, argument);
            }
        }
        Array(length, inner) => {
            hash.update(&length.to_le_bytes());
            hash_shape(hash, inner);
        }
        ConstArray(symbol, inner) => {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
            hash_shape(hash, inner);
        }
        Const(value) => {
            hash.update(&value.to_le_bytes());
        }
        ConstParam(symbol) => {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
        }
        Nullable(inner) | Slice(inner) | Ptr(inner) | Ref(inner) | RefMut(inner)
        | Option(inner) | Coroutine(inner) | Poll(inner) | Range(inner) => hash_shape(hash, inner),
        Result(ok, error) => {
            hash_shape(hash, ok);
            hash_shape(hash, error);
        }
        GenRef | Err | Void | IntLiteral | FloatLiteral | Error => {}
    }
}

impl StableHash for InstanceHir {
    fn stable_hash(&self) -> blake3::Hash {
        let mut hash = blake3::Hasher::new();
        hash.update(b"InstanceHir/v1");
        hash.update(self.artifacts.stable_hash().as_bytes());
        if let Some(function) = self.function {
            hash.update(&[1]);
            hash.update(&function.file_id.to_le_bytes());
            hash.update(&function.local_id.0.to_le_bytes());
        } else {
            hash.update(&[0]);
        }
        for (symbol, key) in &self.instances {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
            hash.update(key.stable_hash().as_bytes());
        }
        for symbol in &self.generated_symbols {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
        }
        hash.finalize()
    }
}

impl StableHash for RuntimeUnit {
    fn stable_hash(&self) -> blake3::Hash {
        let mut hash = blake3::Hasher::new();
        hash.update(b"RuntimeUnit/v1");
        hash.update(self.context.stable_hash().as_bytes());
        match &self.result {
            Ok(unit) => {
                hash.update(&[1]);
                hash.update(unit.function.stable_hash().as_bytes());
                for literal in &unit.literals.entries {
                    use arandu_middle::literal_pool::AmirLiteralEntry;
                    let (tag, value) = match literal {
                        AmirLiteralEntry::Int(v) => (0, v),
                        AmirLiteralEntry::Float(v) => (1, v),
                        AmirLiteralEntry::Str(v) => (2, v),
                        AmirLiteralEntry::Char(v) => (3, v),
                    };
                    hash.update(&[tag]);
                    hash.update(&(value.len() as u64).to_le_bytes());
                    hash.update(value.as_bytes());
                }
                hash.update(unit.diagnostics.stable_hash().as_bytes());
                hash.update(&[u8::from(unit.no_fallback)]);
                for block in &unit.debug_blocks {
                    hash.update(&(block.block.as_usize() as u64).to_le_bytes());
                    hash.update(&block.span.file_id.to_le_bytes());
                    hash.update(&block.span.start.to_le_bytes());
                    hash.update(&block.span.end.to_le_bytes());
                }
                for binding in &unit.debug_bindings {
                    hash.update(&(binding.temp.as_usize() as u64).to_le_bytes());
                    hash.update(&(binding.local.as_usize() as u64).to_le_bytes());
                }
            }
            Err(diagnostics) => {
                hash.update(&[0]);
                hash.update(diagnostics.stable_hash().as_bytes());
            }
        }
        for (symbol, key) in &self.instances {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
            hash.update(key.stable_hash().as_bytes());
        }
        for symbol in &self.generated_symbols {
            hash.update(&symbol.file_id.to_le_bytes());
            hash.update(&symbol.local_id.0.to_le_bytes());
        }
        if let Some(summary) = &self.borrow_summary {
            hash.update(&[1]);
            crate::stable_hash::hash_return_borrow_summary(&mut hash, summary);
        } else {
            hash.update(&[0]);
        }
        if let Some(function) = &self.analysis_function {
            hash.update(&[1]);
            hash.update(function.stable_hash().as_bytes());
        } else {
            hash.update(&[0]);
        }
        hash.finalize()
    }
}

#[salsa::tracked]
#[tracing::instrument(
    level = "trace",
    target = "arandu_query",
    skip(db),
    fields(query = "instance_hir")
)]
pub fn instance_hir<'db>(
    db: &'db dyn ArandCompilerDb,
    instance: Instance<'db>,
) -> HashEq<InstanceHir> {
    let key = instance.key(db);
    let template = function_hir(db, *instance.file(db), key.definition);
    let mut checked = template.type_check.clone();
    let mut hir = template.hir.as_ref().map(|source| {
        let mut hir = arandu_middle::hir::HirProgram {
            span: source.span,
            module: source.module.clone(),
            decls: Vec::new(),
            pool: arandu_middle::hir::HirPool::new(),
        };
        arandu_semantics::link_hir_module(&mut checked, &mut hir, &template.type_check, source);
        hir
    });
    let mut diagnostics = template.diagnostics.clone();
    let mut function = None;
    let mut instances = Vec::new();
    let mut generated_symbols = Vec::new();
    let mut fingerprint = blake3::Hasher::new();
    fingerprint.update(template.stable_hash().as_bytes());
    fingerprint.update(key.stable_hash().as_bytes());
    if let Some(hir) = &mut hir {
        db.unwind_if_revision_cancelled();
        // A concrete argument may come from the caller's module, not from the
        // generic definition's imports (e.g. Vec<String>). Bring only its
        // declaration context, never that module's sibling bodies.
        let mut files = std::collections::BTreeSet::new();
        for argument in &key.arguments {
            if argument
                .for_each_symbol(|symbol| {
                    if checked.symbols.try_get(symbol).is_none() {
                        files.insert(symbol.file_id);
                    }
                })
                .is_err()
            {
                diagnostics.push(Diagnostic::error(
                    arandu_middle::DiagCode::G002GenericInstantiationLimit,
                    "instance arguments exceed the structural type bounds",
                    hir.span,
                ));
            }
        }
        let mut pending = files.into_iter().collect::<Vec<_>>();
        let mut visited = rustc_hash::FxHashSet::default();
        while let Some(file_id) = pending.pop() {
            db.unwind_if_revision_cancelled();
            if !visited.insert(file_id) {
                continue;
            }
            let Some(context_file) = db.source_file_by_id(file_id) else {
                continue;
            };
            let declarations = declaration_hir(db, context_file);
            fingerprint.update(declarations.stable_hash().as_bytes());
            if let Some(context) = &declarations.artifacts.hir {
                arandu_semantics::link_hir_module(
                    &mut checked,
                    hir,
                    &declarations.artifacts.type_check,
                    context,
                );
            } else {
                diagnostics.extend(declarations.artifacts.diagnostics.iter().cloned());
            }
            for (_, imported) in crate::passes::module_import_edges(db, context_file)
                .iter()
                .rev()
            {
                if let Some(file_id) = imported {
                    pending.push(*file_id);
                }
            }
        }
        match arandu_semantics::passes::monomorphize::instantiate_function(&mut checked, hir, key) {
            Ok(concrete) => {
                function = Some(concrete.function);
                instances = concrete.instances;
                generated_symbols = concrete.generated_symbols;
            }
            Err(errors) => diagnostics.extend(errors),
        }
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        hir = None;
    }
    HashEq::new(InstanceHir {
        artifacts: PreparedHir {
            hir,
            type_check: checked,
            diagnostics,
            source_fingerprint: fingerprint.finalize(),
        },
        function,
        instances,
        generated_symbols,
    })
}

#[salsa::tracked]
#[tracing::instrument(
    level = "trace",
    target = "arandu_query",
    skip(db),
    fields(query = "runtime_raw_unit")
)]
pub fn runtime_raw_unit<'db>(
    db: &'db dyn ArandCompilerDb,
    instance: Instance<'db>,
) -> HashEq<RuntimeUnit> {
    let concrete = instance_hir(db, instance);
    let result = match (&concrete.artifacts.hir, concrete.function) {
        (Some(hir), Some(symbol)) => hir
            .decls
            .iter()
            .find_map(|&id| match hir.pool.decl(id) {
                arandu_middle::hir::HirDecl::Func(function)
                    if function.symbol == symbol && function.body.is_some() =>
                {
                    Some(function)
                }
                _ => None,
            })
            .ok_or_else(|| {
                vec![Diagnostic::ice(
                    arandu_middle::DiagCode::ICEL001,
                    "concrete unit has no selected function",
                    hir.span,
                )]
            })
            .and_then(|function| {
                arandu_mir::lower_function_unit(
                    &concrete.artifacts.type_check,
                    hir,
                    function,
                    db.target_config().data_layout(db).pointer_width(),
                )
            }),
        _ => {
            let mut diagnostics = concrete.artifacts.diagnostics.clone();
            if diagnostics.is_empty() {
                diagnostics.push(Diagnostic::ice(
                    arandu_middle::DiagCode::ICEL001,
                    "runtime instance has no typed function body",
                    arandu_middle::Span::new(instance.key(db).definition.file_id, 0, 0),
                ));
            }
            Err(diagnostics)
        }
    };
    HashEq::new(RuntimeUnit {
        result,
        context: std::sync::Arc::new(concrete.artifacts.type_check.clone()),
        instances: concrete.instances.clone(),
        generated_symbols: concrete.generated_symbols.clone(),
        borrow_summary: None,
        analysis_function: None,
    })
}

impl StableHash for DeclarationHir {
    fn stable_hash(&self) -> blake3::Hash {
        self.fingerprint
    }
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "declaration_hir", file = ?file.file_id(db),
))]
pub fn declaration_hir(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<DeclarationHir> {
    let declared = declaration_signatures(db, file);
    let parsed = parse(db, file);
    let mut checked = (**declared).clone();
    let mut fingerprint = blake3::Hasher::new();
    fingerprint.update(b"DeclarationHir/v1");
    let mut hir = None;
    let mut diagnostics = Vec::new();
    if let Ok(program) = &**parsed {
        let mut bodies = Vec::new();
        program.for_each_decl_recursive(|_, declaration| {
            if let arandu_parser::TopLevelDecl::Func(function) = declaration {
                bodies.push(function.body.span);
            }
        });
        // Resolution visits all bodies. Their errors belong to their own
        // function units, not to the declaration-only HIR context.
        checked.diagnostics.retain(|diagnostic| {
            !bodies.iter().any(|span| {
                diagnostic.span.file_id == span.file_id
                    && diagnostic.span.start >= span.start
                    && diagnostic.span.end <= span.end
            })
        });
        program.for_each_decl_recursive(|_, declaration| {
            if !matches!(declaration, arandu_parser::TopLevelDecl::Const(_)) {
                return;
            }
            if let Some(symbol) = arandu_semantics::primary_def_key(declaration)
                .and_then(|key| checked.resolved.definitions.get(&key))
                .copied()
            {
                let item = item_typing(db, file, symbol);
                checked.type_info_mut().merge_from(&item.type_info);
                checked.diagnostics.extend(item.diagnostics.iter().cloned());
                fingerprint.update(item_source_input(db, file, symbol).stable_hash().as_bytes());
            }
        });
        let text = file.text(db);
        for import in &program.imports {
            let span = import.span();
            if let Some(source) = text.get(span.start as usize..span.end as usize) {
                fingerprint.update(source.as_bytes());
            }
        }
        match arandu_semantics::lower_declarations_to_hir(&mut checked, program) {
            Ok(declarations) => {
                for &id in &declarations.decls {
                    let declaration = declarations.pool.decl(id);
                    let span = declaration.span();
                    fingerprint.update(&span.file_id.to_le_bytes());
                    fingerprint.update(&span.start.to_le_bytes());
                    fingerprint.update(&span.end.to_le_bytes());
                    if let arandu_middle::hir::HirDecl::Func(function) = declaration {
                        fingerprint
                            .update(&[u8::from(function.is_async), u8::from(function.no_fallback)]);
                        for parameter in declarations.pool.params_list(function.params) {
                            let receiver = match parameter.receiver_kind {
                                None => 0,
                                Some(arandu_middle::hir::ReceiverKind::Shared) => 1,
                                Some(arandu_middle::hir::ReceiverKind::Mut) => 2,
                                Some(arandu_middle::hir::ReceiverKind::Own) => 3,
                            };
                            fingerprint.update(&[u8::from(parameter.is_receiver), receiver]);
                        }
                    }
                }
                hir = Some(declarations);
            }
            Err(errors) => diagnostics = errors,
        }
    }
    fingerprint.update(crate::stable_hash::type_signature_hash(&checked).as_bytes());
    fingerprint.update(diagnostics.stable_hash().as_bytes());
    fingerprint.update(&[u8::from(hir.is_some())]);
    let fingerprint = fingerprint.finalize();
    HashEq::new(DeclarationHir {
        artifacts: PreparedHir {
            hir,
            type_check: checked,
            diagnostics,
            source_fingerprint: fingerprint,
        },
        fingerprint,
    })
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "function_hir", file = ?file.file_id(db), function = ?symbol,
))]
pub fn function_hir(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    symbol: SymbolId,
) -> HashEq<PreparedHir> {
    let source = item_source_input(db, file, symbol);
    let item = item_typing(db, file, symbol);
    let declared = declaration_signatures(db, file);
    let mut checked = (**item).clone();
    let mut span = None;
    source.program.for_each_decl_recursive(|_, declaration| {
        if arandu_semantics::primary_def_key(declaration)
            .and_then(|key| checked.resolved.definitions.get(&key))
            == Some(&symbol)
        {
            span = Some(arandu_semantics::item_source_span(declaration));
        }
    });
    checked.diagnostics.extend(
        declared
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                span.is_some_and(|span| {
                    diagnostic.span.file_id == span.file_id
                        && diagnostic.span.start >= span.start
                        && diagnostic.span.end <= span.end
                }) || source.program.imports.iter().any(|import| {
                    let span = import.span();
                    diagnostic.span.file_id == span.file_id
                        && diagnostic.span.start >= span.start
                        && diagnostic.span.end <= span.end
                })
            })
            .cloned(),
    );
    let mut fingerprint = blake3::Hasher::new();
    fingerprint.update(b"FunctionHir/v1");
    fingerprint.update(source.stable_hash().as_bytes());
    let mut diagnostics = Vec::new();
    let mut hir =
        match arandu_semantics::lower_function_to_hir(&mut checked, &source.program, symbol) {
            Ok(hir) => hir,
            Err(errors) => {
                diagnostics = errors;
                None
            }
        };
    if let Some(hir) = &mut hir {
        let mut visited = rustc_hash::FxHashSet::default();
        let mut pending = vec![file];
        while let Some(context_file) = pending.pop() {
            db.unwind_if_revision_cancelled();
            if !visited.insert(*context_file.file_id(db)) {
                continue;
            }
            let declarations = declaration_hir(db, context_file);
            fingerprint.update(declarations.stable_hash().as_bytes());
            if let Some(context) = &declarations.artifacts.hir {
                arandu_semantics::link_hir_module(
                    &mut checked,
                    hir,
                    &declarations.artifacts.type_check,
                    context,
                );
            } else {
                diagnostics.extend(declarations.artifacts.diagnostics.iter().cloned());
            }
            // Import topology is a narrow memo. Depending on parse here would
            // relower every sibling's HIR after an unrelated body edit even
            // when declaration_hir itself had already cut off that edit.
            for (_, imported) in crate::passes::module_import_edges(db, context_file)
                .iter()
                .rev()
            {
                if let Some(imported) = imported.and_then(|id| db.source_file_by_id(id)) {
                    pending.push(imported);
                }
            }
        }
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        hir = None;
    }
    HashEq::new(PreparedHir {
        hir,
        type_check: checked,
        diagnostics,
        source_fingerprint: fingerprint.finalize(),
    })
}
