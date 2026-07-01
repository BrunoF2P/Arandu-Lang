use arandu_parser::{FuncName, Program, TopLevelDecl};

use crate::{ResolutionResult, ResolvedNames, SymbolKind, SymbolTable};

mod collect;
mod decls;
mod expr;
mod program;
mod stmt;
mod symbols;
mod types;
mod util;

fn find_stdlib_path(relative: &str) -> Option<std::path::PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

pub(crate) fn load_stdlib_transitively(
    table: &mut SymbolTable,
    start_relative_path: &str,
    current_module: Option<&str>,
    program: Option<&Program>,
) {
    let mut visited = rustc_hash::FxHashSet::default();
    let mut queue = Vec::new();
    if let Some(path) = find_stdlib_path(start_relative_path) {
        queue.push((path, start_relative_path.to_string()));
    }
    if let Some(p) = program {
        for import in &p.imports {
            if let arandu_parser::ImportDecl::External { source, .. } = import {
                if let Some(relative) = source.strip_prefix("std.core.") {
                    let stdlib_rel = format!("stdlib/core/{relative}.aru");
                    if let Some(path) = find_stdlib_path(&stdlib_rel) {
                        queue.push((path, stdlib_rel.clone()));
                    }
                } else if let Some(relative) = source.strip_prefix("std.alloc.") {
                    let stdlib_rel = format!("stdlib/alloc/{relative}.aru");
                    if let Some(path) = find_stdlib_path(&stdlib_rel) {
                        queue.push((path, stdlib_rel.clone()));
                    }
                }
            }
        }
    }

    while let Some((path, _rel_path)) = queue.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(&path)
            && let Ok(program) = arandu_parser::parse(&source)
        {
            let is_current = if let Some(module) = &program.module {
                let module_name = module.path.join(".");
                Some(module_name.as_str()) == current_module
            } else {
                false
            };

            if !is_current {
                let mod_label = program
                    .module
                    .as_ref()
                    .map(|m| m.path.join("."))
                    .unwrap_or_default();
                eprintln!("[load_stdlib] Processing: {}", mod_label);
                // Collect symbols and merge
                let (syms, _, _, _) = crate::name_resolution::collect_symbols(&program);
                let before = table.iter().count();
                table.merge_from(syms);
                let after = table.iter().count();
                eprintln!(
                    "[load_stdlib] Merged {} new symbols into table",
                    after - before
                );

                // Define module members
                if let Some(module) = &program.module {
                    let module_name = module.path.join(".");
                    for decl_id in &program.decls {
                        let decl = program.pool.decl(*decl_id);
                        match decl {
                            arandu_parser::TopLevelDecl::Enum(d) => {
                                let _ = table.define_module_member(&module_name, &d.name, d.span);
                            }
                            arandu_parser::TopLevelDecl::Struct(d) => {
                                let _ = table.define_module_member(&module_name, &d.name, d.span);
                            }
                            arandu_parser::TopLevelDecl::Extern(d) => {
                                for member in &d.members {
                                    let _ = table.define_module_member(
                                        &module_name,
                                        &member.name,
                                        member.span,
                                    );
                                }
                            }
                            arandu_parser::TopLevelDecl::Func(d) => {
                                if let arandu_parser::FuncName::Free { span, name } = &d.name {
                                    let _ = table.define_module_member(&module_name, name, *span);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Enqueue imports
            for import in &program.imports {
                if let arandu_parser::ImportDecl::External { source, .. } = import {
                    eprintln!(
                        "[load_stdlib] Import: source='{}' in {:?}",
                        source,
                        program.module.as_ref().map(|m| m.path.join("."))
                    );
                    if let Some(relative) = source.strip_prefix("std.core.") {
                        let stdlib_rel = format!("stdlib/core/{relative}.aru");
                        if let Some(p) = find_stdlib_path(&stdlib_rel) {
                            eprintln!("[load_stdlib] Enqueuing: {:?}", p);
                            queue.push((p, stdlib_rel));
                        } else {
                            eprintln!("[load_stdlib] NOT FOUND: {}", stdlib_rel);
                        }
                    } else if let Some(relative) = source.strip_prefix("std.alloc.") {
                        let stdlib_rel = format!("stdlib/alloc/{relative}.aru");
                        if let Some(p) = find_stdlib_path(&stdlib_rel) {
                            queue.push((p, stdlib_rel));
                        }
                    }
                }
            }
        }
    }
}

#[must_use]
pub fn create_symbol_table_with_prelude() -> SymbolTable {
    let mut table = SymbolTable::new();
    let span = arandu_lexer::Span::new(0, 0, 0);
    eprintln!("[prelude] Creating symbol table with prelude");
    for (module, members) in [
        ("io", ["println", "create", "remove"].as_slice()),
        ("err", ["new"].as_slice()),
    ] {
        for member in members {
            let _ = table.define_module_member(module, member, span);
        }
    }
    load_stdlib_transitively(&mut table, "stdlib/core/prelude.aru", None, None);
    table
}

#[must_use]
pub fn resolve(program: &Program) -> ResolutionResult {
    Resolver::new(&program.pool, Some(program)).resolve_program(program)
}

#[must_use]
pub fn collect_symbols(
    program: &Program,
) -> (
    SymbolTable,
    ResolvedNames,
    crate::DocCommentMap,
    Vec<crate::Diagnostic>,
) {
    let mut resolver = Resolver {
        symbols: SymbolTable::new(),
        resolved: ResolvedNames::default(),
        docs: crate::DocCommentMap::default(),
        diagnostics: Vec::new(),
        pool: &program.pool,
        import_aliases: rustc_hash::FxHashMap::default(),
        current_module: program.module.as_ref().map(|m| m.path.join(".")),
        imported_symbols: rustc_hash::FxHashMap::default(),
        used_symbols: rustc_hash::FxHashSet::default(),
    };

    for doc in &program.docs {
        resolver
            .docs
            .entry(crate::NodeKey::from(doc.target_span))
            .or_default()
            .push(doc.text.clone());
    }

    let global = resolver.symbols.global_scope();
    if let Some(module) = &program.module
        && let Some(root) = module.path.first()
    {
        resolver.define(global, root, SymbolKind::Module, module.span);
    }

    for import in &program.imports {
        resolver.collect_import(global, import);
    }

    for decl_id in &program.decls {
        let decl = program.pool.decl(*decl_id);
        resolver.collect_top_level(global, decl);
    }

    if let Some(module) = &program.module {
        let module_name = module.path.join(".");
        for decl_id in &program.decls {
            let decl = program.pool.decl(*decl_id);
            match decl {
                TopLevelDecl::Const(d) => {
                    let _ = resolver
                        .symbols
                        .define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::TypeAlias(d) => {
                    let _ = resolver
                        .symbols
                        .define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Func(d) => {
                    if let FuncName::Free { span, name } = &d.name {
                        let _ = resolver
                            .symbols
                            .define_module_member(&module_name, name, *span);
                    }
                }
                TopLevelDecl::Struct(d) => {
                    let _ = resolver
                        .symbols
                        .define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Enum(d) => {
                    let _ = resolver
                        .symbols
                        .define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Interface(d) => {
                    let _ = resolver
                        .symbols
                        .define_module_member(&module_name, &d.name, d.span);
                }
                TopLevelDecl::Extern(d) => {
                    for member in &d.members {
                        let _ = resolver.symbols.define_module_member(
                            &module_name,
                            &member.name,
                            member.span,
                        );
                    }
                }
                TopLevelDecl::Error(_) => {}
            }
        }
    }

    (
        resolver.symbols,
        resolver.resolved,
        resolver.docs,
        resolver.diagnostics,
    )
}

#[must_use]
pub fn resolve_with_symbols(
    global_symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: crate::DocCommentMap,
    diagnostics: Vec<crate::Diagnostic>,
    program: &Program,
) -> ResolutionResult {
    let mut resolver = Resolver {
        symbols: global_symbols,
        resolved,
        docs,
        diagnostics,
        pool: &program.pool,
        import_aliases: rustc_hash::FxHashMap::default(),
        current_module: program.module.as_ref().map(|m| m.path.join(".")),
        imported_symbols: rustc_hash::FxHashMap::default(),
        used_symbols: rustc_hash::FxHashSet::default(),
    };

    for import in &program.imports {
        if let arandu_parser::ImportDecl::External { source, alias, .. } = import {
            resolver
                .import_aliases
                .insert(alias.clone(), source.clone());
        }
    }

    let global = resolver.symbols.global_scope();
    for decl_id in &program.decls {
        let decl = program.pool.decl(*decl_id);
        resolver.resolve_top_level(global, decl);
    }

    resolver.check_unused_imports();

    ResolutionResult {
        symbols: resolver.symbols,
        resolved: resolver.resolved,
        docs: resolver.docs,
        diagnostics: resolver.diagnostics,
    }
}

struct Resolver<'a> {
    symbols: SymbolTable,
    resolved: ResolvedNames,
    docs: crate::DocCommentMap,
    diagnostics: Vec<crate::Diagnostic>,
    pool: &'a arandu_parser::ast_pool::AstPool,
    import_aliases: rustc_hash::FxHashMap<String, String>,
    current_module: Option<String>,
    imported_symbols: rustc_hash::FxHashMap<crate::SymbolId, (String, arandu_lexer::Span)>,
    used_symbols: rustc_hash::FxHashSet<crate::SymbolId>,
}

impl<'a> Resolver<'a> {
    pub(crate) fn mark_used(&mut self, symbol: crate::SymbolId) {
        self.used_symbols.insert(symbol);
    }

    pub(crate) fn record_expr_ref(
        &mut self,
        expr: arandu_parser::ast_pool::ExprId,
        symbol: crate::SymbolId,
    ) {
        self.resolved.expr_ref(expr, symbol);
        self.mark_used(symbol);
    }

    pub(crate) fn record_value_ref(&mut self, span: arandu_lexer::Span, symbol: crate::SymbolId) {
        self.resolved.value_ref(span, symbol);
        self.mark_used(symbol);
    }

    pub(crate) fn record_type_ref(&mut self, span: arandu_lexer::Span, symbol: crate::SymbolId) {
        self.resolved.type_ref(span, symbol);
        self.mark_used(symbol);
    }

    pub(crate) fn record_import_symbol(
        &mut self,
        symbol: crate::SymbolId,
        name: String,
        span: arandu_lexer::Span,
    ) {
        self.imported_symbols.insert(symbol, (name, span));
    }

    pub(crate) fn lookup_and_record_module(
        &mut self,
        scope: crate::ScopeId,
        name: &str,
    ) -> Option<crate::SymbolId> {
        let sym = self.symbols.lookup_module(scope, name)?;
        self.mark_used(sym);
        Some(sym)
    }

    pub(crate) fn check_unused_imports(&mut self) {
        for (sym_id, (name, span)) in &self.imported_symbols {
            if !self.used_symbols.contains(sym_id) {
                self.diagnostics.push(crate::Diagnostic::warning(
                    crate::DiagCode::W007UnusedImport,
                    format!("unused import `{name}`"),
                    *span,
                ));
            }
        }
    }
}
