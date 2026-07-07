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

pub fn cycle_recover(
    _db: &dyn ArandCompilerDb,
    _id: salsa::Id,
    file: SourceFile,
) -> HashEq<ResolutionResult> {
    println!("cycle_recover for resolve on file {:?}", file.file_id(_db));
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

pub fn symbol_span(
    _db: &dyn ArandCompilerDb,
    _symbol_id: arandu_middle::SymbolId,
) -> arandu_base::Span {
    // In a real implementation, we would just fetch the file by symbol_id.file_id
    // But since we can't construct SourceFile just from FileId directly without a query in this DB
    // we would use db to lookup the file.
    // For now, this is a placeholder returning a dummy span if it's not implemented yet.
    arandu_base::Span::new(0, 0, 0)
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "parse",
    file = ?file.file_id(db),
))]
pub fn parse(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
) -> HashEq<Result<Program, arandu_parser::ParseError>> {
    let text = file.text(db);
    match arandu_parser::parse_with_file_id(&text, file.file_id(db)) {
        Ok(program) => HashEq::new(Ok(program)),
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

    let resolved = match &*program_res {
        Ok(program) => arandu_resolve::resolve_imports_and_bodies(
            db.as_source_db(),
            program,
            (*locals_arc).clone(),
        ),
        Err(_) => (*locals_arc).clone(),
    };

    HashEq::new(resolved)
}

pub fn cycle_recover_module_signatures(
    _db: &dyn ArandCompilerDb,
    _id: salsa::Id,
    _file: SourceFile,
) -> HashEq<TypeCheckResult> {
    let mut res = TypeCheckResult {
        symbols: arandu_semantics::SymbolTable::default(),
        resolved: arandu_semantics::ResolvedNames::default(),
        type_info: arandu_semantics::TypeInfo::default(),
        diagnostics: vec![],
    };
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
            let mut checker = arandu_semantics::TypeChecker::new(
                (*resolved_arc).clone().symbols,
                (*resolved_arc).clone().resolved,
                (*resolved_arc).clone().diagnostics,
                &program.pool,
            );

            // Merge imported type info
            for import in &program.imports {
                let module_path = match import {
                    arandu_parser::ImportDecl::ModuleAlias { path, .. }
                    | arandu_parser::ImportDecl::Named { path, .. } => {
                        let path_str = path.join("/");
                        if let Some(stripped) = path_str.strip_prefix("std/core/") {
                            Some(format!("stdlib/core/{}.aru", stripped))
                        } else if let Some(stripped) = path_str.strip_prefix("std/alloc/") {
                            Some(format!("stdlib/alloc/{}.aru", stripped))
                        } else {
                            Some(format!("{path_str}.aru"))
                        }
                    }
                    arandu_parser::ImportDecl::ExternalAlias { source, .. }
                    | arandu_parser::ImportDecl::ExternalNamed { source, .. } => {
                        if let Some(stripped) = source.strip_prefix("std.core.") {
                            Some(format!("stdlib/core/{}.aru", stripped))
                        } else if let Some(stripped) = source.strip_prefix("std.alloc.") {
                            Some(format!("stdlib/alloc/{}.aru", stripped))
                        } else {
                            Some(source.to_string())
                        }
                    }
                };
                if let Some(path) = module_path {
                    if let Some(imported_file) = db.as_source_db().resolve_module_path(&path) {
                        let imported_sigs = module_signatures(db, imported_file);
                        println!(
                            "Imported sigs for {} into {:?}: {:?}",
                            path,
                            file.file_id(db),
                            imported_sigs.diagnostics
                        );
                        checker.type_info.merge_from(&imported_sigs.type_info);
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
            symbols: arandu_semantics::SymbolTable::default(),
            resolved: resolved_arc.resolved.clone(),
            type_info: arandu_semantics::TypeInfo::default(),
            diagnostics: vec![],
        },
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

    let program_res = parse(db, file);
    let signatures_arc = module_signatures(db, file);
    println!(
        "TypeCheckResult of module_signatures for {:?}: {:?}",
        file.file_id(db),
        signatures_arc.diagnostics
    );

    let res = match &*program_res {
        Ok(program) => arandu_semantics::check_bodies_only((*signatures_arc).clone(), program),
        Err(_) => (*signatures_arc).clone(),
    };
    println!(
        "TypeCheckResult at end of type_check for {:?}: {:?}",
        file.file_id(db),
        res.diagnostics
    );

    // Accumulate diagnostics without removing them from the return value!
    for diag in &res.diagnostics {
        arandu_middle::db::DiagnosticsAccumulator(diag.clone()).accumulate(db);
    }

    HashEq::new(res)
}

#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "lower_amir",
    file = ?file.file_id(db),
))]
pub fn lower_amir(db: &dyn ArandCompilerDb, file: SourceFile) -> HashEq<AmirProgram> {
    let program_res = parse(db, file);
    let type_check_result_arc = type_check(db, file);

    let mut type_check_result = (*type_check_result_arc).clone();

    let hir = match &*program_res {
        Ok(program) => match arandu_semantics::lower_to_hir(&mut type_check_result, program) {
            Ok(h) => h,
            Err(diags) => {
                for diag in diags {
                    arandu_middle::db::DiagnosticsAccumulator(diag).accumulate(db);
                }
                return HashEq::new(AmirProgram {
                    funcs: vec![],
                    literal_pool: arandu_middle::literal_pool::AmirLiteralPool::default(),
                    extern_funcs: Default::default(),
                });
            }
        },
        Err(_) => {
            return HashEq::new(AmirProgram {
                funcs: vec![],
                literal_pool: arandu_middle::literal_pool::AmirLiteralPool::default(),
                extern_funcs: Default::default(),
            })
        }
    };

    let amir = match arandu_semantics::lower_to_amir(&type_check_result, &hir) {
        Ok(a) => a,
        Err(diags) => {
            for diag in diags {
                arandu_middle::db::DiagnosticsAccumulator(diag).accumulate(db);
            }
            AmirProgram {
                funcs: vec![],
                literal_pool: arandu_middle::literal_pool::AmirLiteralPool::default(),
                extern_funcs: Default::default(),
            }
        }
    };
    HashEq::new(amir)
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
                let module_path = match import {
                    arandu_parser::ImportDecl::ModuleAlias { path, .. }
                    | arandu_parser::ImportDecl::Named { path, .. } => {
                        let path_str = path.join("/");
                        if let Some(stripped) = path_str.strip_prefix("std/core/") {
                            Some(format!("stdlib/core/{}.aru", stripped))
                        } else if let Some(stripped) = path_str.strip_prefix("std/alloc/") {
                            Some(format!("stdlib/alloc/{}.aru", stripped))
                        } else {
                            Some(format!("{path_str}.aru"))
                        }
                    }
                    arandu_parser::ImportDecl::ExternalAlias { source, .. }
                    | arandu_parser::ImportDecl::ExternalNamed { source, .. } => {
                        if let Some(stripped) = source.strip_prefix("std.core.") {
                            Some(format!("stdlib/core/{}.aru", stripped))
                        } else if let Some(stripped) = source.strip_prefix("std.alloc.") {
                            Some(format!("stdlib/alloc/{}.aru", stripped))
                        } else {
                            Some(source.to_string())
                        }
                    }
                };
                if let Some(path) = module_path {
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
