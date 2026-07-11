use crate::db::HashEq;
use crate::{ArandCompilerDb, SourceFile};
use arandu_parser::Program;
use arandu_resolve::ResolutionResult;
use arandu_semantics::{amir::AmirProgram, TypeCheckResult};
use salsa::Accumulator;
#[cfg(any(test, debug_assertions))]
use std::sync::atomic::{AtomicUsize, Ordering};

use std::sync::Arc;

#[cfg(any(test, debug_assertions))]
pub static RESOLVE_EXEC_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(any(test, debug_assertions))]
pub static TYPE_CHECK_EXEC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Counts `item_body_typeck` body executions (P1 fine-grained).
#[cfg(any(test, debug_assertions))]
pub static ITEM_BODY_TYPECK_EXEC_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn cycle_recover(
    _db: &dyn ArandCompilerDb,
    _id: salsa::Id,
    file: SourceFile,
) -> HashEq<ResolutionResult> {
    tracing::debug!(
        target: "arandu_query",
        file = ?file.file_id(_db),
        "cycle_recover for resolve"
    );
    HashEq::new(ResolutionResult::cycle_fallback())
}

#[salsa::tracked]
pub fn local_symbols(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<ResolutionResult> {
    let program_res = parse(db, file);
    let resolved = match &*program_res {
        Ok(program) => arandu_resolve::resolve_local(file.file_id(db), program),
        Err(_) => ResolutionResult {
            is_cycle_fallback: false,
            symbols: arandu_semantics::SymbolTable::default(),
            resolved: arandu_semantics::ResolvedNames::default(),
            docs: arandu_semantics::DocCommentMap::default(),
            diagnostics: vec![],
        },
    };

    HashEq::new(resolved)
}

#[salsa::tracked]
pub fn exported_symbols(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
) -> Arc<arandu_middle::ExportedSymbolTable> {
    let locals = local_symbols(db, file);
    let mut map = std::collections::BTreeMap::new();

    // For now, just expose everything in the global scope as exported.
    let global_scope = locals.symbols.global_scope();
    for symbol in locals.symbols.iter() {
        if symbol.scope == global_scope {
            map.insert(symbol.name.to_string(), (symbol.id, symbol.kind));
        }
    }

    Arc::new(arandu_middle::ExportedSymbolTable { symbols: map })
}

/// Real definition span for `symbol_id` (from the owning file's resolve result).
///
/// Never panics on unknown / cross-file ids: returns a zero-width span.
pub fn symbol_span(
    db: &dyn ArandCompilerDb,
    symbol_id: arandu_middle::SymbolId,
) -> arandu_base::Span {
    let empty = arandu_base::Span::new(symbol_id.file_id, 0, 0);
    let Some(file) = db.source_file_by_id(symbol_id.file_id) else {
        return empty;
    };
    let resolved = resolve(db, file);
    let Some(symbol) = resolved.symbols.try_get(symbol_id) else {
        return empty;
    };
    let span = symbol.span;
    arandu_base::Span::new(span.file_id, span.start, span.end)
}

/// P5: authoritative CST (rowan). Built from source alone — no AST dependency.
///
/// Uses the DB CST cache + [`arandu_parser::reparse_subtree`] when a single
/// contiguous edit is detected against the previous tree for this file.
#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "syntax_tree",
    file = ?file.file_id(db),
))]
pub fn syntax_tree(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
) -> HashEq<arandu_parser::SyntaxTree> {
    let text = file.text(db);
    let file_id = file.file_id(db);
    // Prefer DatabaseImpl incremental path; share Arc text with SourceFile.
    let tree = if let Some(impl_db) = db.as_db_impl() {
        impl_db.syntax_tree_for_arc(file_id, Arc::clone(&text))
    } else {
        arandu_parser::parse_syntax_arc(Arc::clone(&text))
    };
    HashEq::new(tree)
}

/// AST for typeck/resolve: **lowered from CST tokens** (no re-lex, no dual parse).
///
/// Memo stores `Arc<Program>` so per-item queries share the same program without deep-clone.
#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "parse",
    file = ?file.file_id(db),
))]
pub fn parse(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
) -> HashEq<Result<Arc<Program>, arandu_parser::ParseError>> {
    let tree = syntax_tree(db, file);
    match arandu_parser::lower_syntax_to_program(&tree, file.file_id(db)) {
        Ok(program) => HashEq::new(Ok(Arc::new(program))),
        Err(err) => HashEq::new(Err(err)),
    }
}

#[salsa::tracked(cycle_result = cycle_recover)]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "resolve",
    file = ?file.file_id(db),
))]
pub fn resolve(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<ResolutionResult> {
    #[cfg(any(test, debug_assertions))]
    RESOLVE_EXEC_COUNT.fetch_add(1, Ordering::SeqCst);

    let program_res = parse(db, file);
    let locals_arc = local_symbols(db, file);

    // Prefer Arc unshare over deep-cloning ResolutionResult when we are the sole owner.
    let locals_owned = std::sync::Arc::unwrap_or_clone(std::sync::Arc::clone(&locals_arc.value));
    let resolved = match &*program_res {
        Ok(program) => arandu_resolve::resolve_imports_and_bodies(
            &arandu_resolve::SourceDbLoader(db.as_source_db()),
            program,
            locals_owned,
        ),
        Err(_) => locals_owned,
    };

    HashEq::new(resolved)
}

pub fn cycle_recover_module_signatures(
    _db: &dyn ArandCompilerDb,
    _id: salsa::Id,
    _file: SourceFile,
) -> HashEq<TypeCheckResult> {
    let mut res = TypeCheckResult::empty();
    res.diagnostics.push(arandu_middle::Diagnostic::error(
        arandu_middle::DiagCode::N006ImportConflict,
        "cyclic module signature dependency detected".to_string(),
        arandu_middle::Span::new(0, 0, 0),
    ));
    HashEq::new(res)
}

#[salsa::tracked(cycle_result = cycle_recover_module_signatures)]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "module_signatures",
    file = ?file.file_id(db),
))]
pub fn module_signatures(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<TypeCheckResult> {
    let program_res = parse(db, file);
    let resolved_arc = resolve(db, file);

    let res = match &*program_res {
        Ok(program) => {
            // Destructure once — avoids 3 separate full clones of ResolutionResult.
            let ResolutionResult {
                symbols,
                resolved,
                diagnostics,
                ..
            } = (*resolved_arc).clone();
            let mut checker =
                arandu_semantics::TypeChecker::new(symbols, resolved, diagnostics, &program.pool);

            // Merge imported type info (path rewrite shared with resolve).
            for import in &program.imports {
                if let Some(path) = arandu_resolve::canonicalize_import_path(import) {
                    if let Some(imported_file) = db.as_source_db().resolve_module_path(&path) {
                        let imported_sigs = module_signatures(db, imported_file);
                        tracing::debug!(
                            target: "arandu_query",
                            %path,
                            file = ?file.file_id(db),
                            diags = ?imported_sigs.diagnostics,
                            "merged imported module signatures"
                        );
                        checker
                            .type_info
                            .merge_from(imported_sigs.type_info.as_ref());
                        for diag in &imported_sigs.diagnostics {
                            if diag.message.contains("cyclic") {
                                checker.diagnostics.push(diag.clone());
                            }
                        }
                    }
                }
            }

            arandu_semantics::check_signatures(&mut checker, program);
            checker.finish()
        }
        Err(_) => TypeCheckResult {
            symbols: std::sync::Arc::new(arandu_semantics::SymbolTable::default()),
            resolved: std::sync::Arc::new(resolved_arc.resolved.clone()),
            type_info: std::sync::Arc::new(arandu_semantics::TypeInfo::default()),
            diagnostics: vec![],
        },
    };

    HashEq::new(res)
}

/// Per-item input for body typeck: holds current [`Program`] but **HashEq**
/// only fingerprints that item's source span (sibling edits early-cutoff).
#[derive(Clone)]
pub struct ItemSourceInput {
    pub program: Arc<Program>,
    pub item_sym: arandu_middle::SymbolId,
    /// blake3 of the item's source slice for StableHash / early cutoff.
    pub(crate) body_fp: blake3::Hash,
}

/// Backward-compatible alias used by older call sites / StableHash.
pub type FuncBodyInput = ItemSourceInput;

/// Extract one item's AST dependency from `parse`+`resolve` with content-addressed HashEq.
#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "item_source_input",
    file = ?file.file_id(db),
    item = ?item_sym,
))]
pub fn item_source_input(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    item_sym: arandu_middle::SymbolId,
) -> HashEq<ItemSourceInput> {
    use arandu_parser::TopLevelDecl;

    let program_res = parse(db, file);
    let resolved = resolve(db, file);
    let text = file.text(db);

    let Ok(program) = &*program_res else {
        return HashEq::new(ItemSourceInput {
            program: Arc::new(empty_program()),
            item_sym,
            body_fp: blake3::hash(b"parse-error"),
        });
    };

    // Depend on CST for incremental invalidation; fingerprint from text slices (no String alloc).
    let tree = syntax_tree(db, file);
    let ranges = tree.item_ranges();

    let mut body_fp = blake3::hash(b"item-missing");
    for decl_id in &program.decls {
        let decl = program.pool.decl(*decl_id);
        let matches = match arandu_semantics::primary_def_key(decl) {
            Some(key) => resolved.resolved.definitions.get(&key) == Some(&item_sym),
            None => false,
        } || matches!(
            decl,
            TopLevelDecl::Extern(ext)
                if ext.members.iter().any(|m| {
                    resolved.resolved.definitions.get(&arandu_middle::NodeKey::from(m.span))
                        == Some(&item_sym)
                })
        );
        if !matches {
            continue;
        }
        let span = arandu_semantics::item_source_span(decl);
        let start = (span.start as usize).min(text.len());
        let end = (span.end as usize).min(text.len()).max(start);
        let mut h = blake3::Hasher::new();
        h.update(b"item_body_v4");
        // Prefer covering CST ITEM range (zero-copy slice of shared text).
        let mut used_cst = false;
        for &(s, e) in &ranges {
            if s <= span.start && span.end <= e {
                let s = (s as usize).min(text.len());
                let e = (e as usize).min(text.len()).max(s);
                h.update(text[s..e].as_bytes());
                used_cst = true;
                break;
            }
        }
        if !used_cst {
            h.update(text[start..end].as_bytes());
        }
        body_fp = h.finalize();
        break;
    }

    HashEq::new(ItemSourceInput {
        program: Arc::clone(program),
        item_sym,
        body_fp,
    })
}

/// Alias for P1 name (thin wrapper; same memo as [`item_source_input`]).
#[inline]
pub fn func_body_input(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    func_sym: arandu_middle::SymbolId,
) -> HashEq<ItemSourceInput> {
    item_source_input(db, file, func_sym)
}

fn empty_program() -> Program {
    Program {
        span: arandu_base::Span::new(0, 0, 0),
        module: None,
        imports: vec![],
        decls: vec![],
        docs: vec![],
        pool: arandu_parser::ast_pool::AstPool::default(),
    }
}

/// Per-item body typeck (P1 funcs + P2 all top-level body items).
///
/// Depends on [`item_source_input`] (HashEq by item source span) + [`module_signatures`].
#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "item_body_typeck",
    file = ?file.file_id(db),
    item = ?item_sym,
))]
pub fn item_body_typeck(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    item_sym: arandu_middle::SymbolId,
) -> HashEq<TypeCheckResult> {
    #[cfg(any(test, debug_assertions))]
    ITEM_BODY_TYPECK_EXEC_COUNT.fetch_add(1, Ordering::SeqCst);

    let body_in = item_source_input(db, file, item_sym);
    let signatures = module_signatures(db, file);
    let res =
        arandu_semantics::check_item_body_only(&signatures, body_in.program.as_ref(), item_sym);
    HashEq::new(res)
}

/// Composed file typeck: signatures + per-item body memos (P2).
///
/// This is the incremental-friendly view; [`type_check`] delegates here.
#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "file_typeck_view",
    file = ?file.file_id(db),
))]
pub fn file_typeck_view(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<TypeCheckResult> {
    let program_res = parse(db, file);
    let signatures = module_signatures(db, file);

    let Ok(program) = &*program_res else {
        return HashEq::new((*signatures).clone());
    };

    let item_syms = arandu_semantics::body_item_symbols(program, signatures.resolved.as_ref());

    let mut merged_info = (*signatures.type_info).clone();
    let mut diagnostics = signatures.diagnostics.clone();

    for &item_sym in &item_syms {
        let item = item_body_typeck(db, file, item_sym);
        merged_info.merge_from(item.type_info.as_ref());
        diagnostics.extend(item.diagnostics.iter().cloned());
    }

    // Residual for decls without primary keys (normally empty).
    let residual = arandu_semantics::check_non_func_bodies_only(&signatures, program);
    merged_info.merge_from(residual.type_info.as_ref());
    diagnostics.extend(residual.diagnostics);

    let res = TypeCheckResult {
        symbols: Arc::clone(&signatures.symbols),
        resolved: Arc::clone(&signatures.resolved),
        type_info: Arc::new(merged_info),
        diagnostics,
    };
    HashEq::new(res)
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "type_check",
    file = ?file.file_id(db),
))]
pub fn type_check(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<TypeCheckResult> {
    #[cfg(any(test, debug_assertions))]
    TYPE_CHECK_EXEC_COUNT.fetch_add(1, Ordering::SeqCst);

    // P1: compose per-function body checks (early cutoff across funcs).
    let res = file_typeck_view(db, file);

    tracing::debug!(
        target: "arandu_query",
        file = ?file.file_id(db),
        diags = ?res.diagnostics,
        "type_check complete (file_typeck_view)"
    );

    for diag in &res.diagnostics {
        arandu_middle::db::DiagnosticsAccumulator(diag.clone()).accumulate(db);
    }

    // Share the same Arc as `file_typeck_view` — no deep clone of TypeCheckResult.
    HashEq::share(&res)
}

/// AMIR plus the **post-monomorphize** [`TypeCheckResult`] used to build it.
///
/// Codegen must use `type_check` from this bundle — monomorphization allocates
/// new symbols that do not exist on the pre-mono `type_check` query alone.
/// Returning both from one Salsa query avoids the old CLI double path
/// (local HIR+mono + `lower_amir` HIR+mono) while keeping symbol tables aligned.
#[derive(Debug, Clone)]
pub struct LowerAmirArtifacts {
    pub amir: AmirProgram,
    pub type_check: TypeCheckResult,
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "lower_amir",
    file = ?file.file_id(db),
))]
pub fn lower_amir(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<LowerAmirArtifacts> {
    let program_res = parse(db, file);
    let type_check_result_arc = type_check(db, file);

    // Clone for mutation: lower_to_hir / monomorphize update symbols + type_info.
    // Arc fields are O(1); mono may Arc::make_mut type_info once.
    let mut type_check_result = (*type_check_result_arc).clone();

    let empty_amir = || AmirProgram {
        funcs: vec![],
        literal_pool: arandu_middle::literal_pool::AmirLiteralPool::default(),
        extern_funcs: Default::default(),
    };

    let mut hir = {
        arandu_base::time_pass!("lower-hir");
        match &*program_res {
            Ok(program) => match arandu_semantics::lower_to_hir(&mut type_check_result, program) {
                Ok(h) => h,
                Err(diags) => {
                    for diag in diags {
                        arandu_middle::db::DiagnosticsAccumulator(diag).accumulate(db);
                    }
                    return HashEq::new(LowerAmirArtifacts {
                        amir: empty_amir(),
                        type_check: type_check_result,
                    });
                }
            },
            Err(_) => {
                return HashEq::new(LowerAmirArtifacts {
                    amir: empty_amir(),
                    type_check: type_check_result,
                });
            }
        }
    };

    {
        arandu_base::time_pass!("monomorphize");
        if let Err(diags) = arandu_semantics::passes::monomorphize::monomorphize_program(
            &mut type_check_result,
            &mut hir,
        ) {
            for diag in diags {
                arandu_middle::db::DiagnosticsAccumulator(diag).accumulate(db);
            }
            return HashEq::new(LowerAmirArtifacts {
                amir: empty_amir(),
                type_check: type_check_result,
            });
        }
    }

    let amir = {
        arandu_base::time_pass!("lower-amir-body");
        match arandu_semantics::lower_to_amir(&type_check_result, &hir) {
            Ok(a) => a,
            Err(diags) => {
                for diag in diags {
                    arandu_middle::db::DiagnosticsAccumulator(diag).accumulate(db);
                }
                empty_amir()
            }
        }
    };
    HashEq::new(LowerAmirArtifacts {
        amir,
        type_check: type_check_result,
    })
}

#[salsa::tracked]
pub fn module_dependency_graph(
    db: &dyn ArandCompilerDb,
    root: SourceFile,
) -> HashEq<petgraph::Graph<u32, ()>> {
    use petgraph::Graph;
    let mut graph = Graph::new();
    let mut visited = std::collections::HashMap::new();

    fn walk(
        db: &dyn ArandCompilerDb,
        file: SourceFile,
        graph: &mut Graph<u32, ()>,
        visited: &mut std::collections::HashMap<u32, petgraph::graph::NodeIndex>,
    ) -> petgraph::graph::NodeIndex {
        let file_id = file.file_id(db.as_source_db());
        if let Some(&node) = visited.get(&file_id) {
            return node;
        }

        let node = graph.add_node(file_id);
        visited.insert(file_id, node);

        let program_res = crate::passes::parse(db, file);
        if let Ok(program) = &*program_res {
            for import in &program.imports {
                if let Some(path) = arandu_resolve::canonicalize_import_path(import) {
                    if let Some(imported_file) = db.as_source_db().resolve_module_path(&path) {
                        let imported_node = walk(db, imported_file, graph, visited);
                        graph.add_edge(node, imported_node, ());
                    }
                }
            }
        }

        node
    }

    walk(db, root, &mut graph, &mut visited);
    HashEq::new(graph)
}
