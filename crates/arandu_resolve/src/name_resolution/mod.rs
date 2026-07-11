use arandu_parser::{FuncName, Program, TopLevelDecl};
use smol_str::SmolStr;

use crate::{ResolutionResult, ResolvedNames, SymbolKind, SymbolTable};

mod collect;
mod decls;
mod expr;
mod program;
mod stmt;
mod symbols;
mod types;
mod util;

/// Builtin prelude modules injected by [`define_prelude`] / this helper.
/// Kept in one place so Salsa import resolution can short-circuit without
/// requiring on-disk `io.aru` / `err.aru` files.
pub const PRELUDE_MODULES: &[&str] = &["io", "err"];

/// Members registered for each prelude module (must stay in sync with
/// [`super::program::Resolver::define_prelude`]).
const PRELUDE_MODULE_MEMBERS: &[(&str, &[&str])] =
    &[("io", &["println", "create", "remove"]), ("err", &["new"])];

/// Returns the prelude module name if `path` is a single-segment prelude path.
#[must_use]
pub fn prelude_module_from_path(path: &[SmolStr]) -> Option<&'static str> {
    if path.len() != 1 {
        return None;
    }
    let name = path[0].as_str();
    PRELUDE_MODULES.iter().copied().find(|&m| m == name)
}

pub fn create_symbol_table_with_prelude(
    file_id: u32,
) -> Result<SymbolTable, Vec<crate::Diagnostic>> {
    let mut table = SymbolTable::new(file_id);
    let span = arandu_lexer::Span::new(0, 0, 0);
    tracing::debug!(target: "arandu_resolve", "Creating symbol table with prelude");
    for (module, members) in PRELUDE_MODULE_MEMBERS {
        for member in *members {
            let _ = table.define_module_member(module, member, span);
        }
    }
    let global_scope = table.global_scope();
    table.builtin_alloc = table
        .define(global_scope, "alloc", SymbolKind::Func, span)
        .ok();
    table.builtin_free = table
        .define(global_scope, "free", SymbolKind::Func, span)
        .ok();
    Ok(table)
}

#[must_use]
pub fn resolve_local(file_id: u32, program: &Program) -> ResolutionResult {
    Resolver::new(file_id, &program.pool, Some(program)).resolve_local(program)
}

/// Single-file / unit-test resolve that runs the **same** import pipeline as
/// production, with an empty module loader (no multi-file loads).
///
/// Prefer this over hand-rolled import collection so prelude short-circuit and
/// `canonicalize_import_path` stay shared with the CLI (RC-DUAL-RESOLVE).
#[must_use]
pub fn resolve_for_test(file_id: u32, program: &Program) -> ResolutionResult {
    let local = resolve_local(file_id, program);
    resolve_imports_and_bodies(&crate::EmptyModuleLoader, program, local)
}

#[must_use]
pub fn resolve_imports_and_bodies(
    db: &dyn crate::ModuleLoader,
    program: &Program,
    result: ResolutionResult,
) -> ResolutionResult {
    let mut resolver = Resolver {
        symbols: result.symbols,
        resolved: result.resolved,
        docs: result.docs,
        diagnostics: result.diagnostics,
        pool: &program.pool,
        import_aliases: rustc_hash::FxHashMap::default(),
        current_module: program.module.as_ref().map(|m| m.path.join(".")),
        imported_symbols: rustc_hash::FxHashMap::default(),
        used_symbols: rustc_hash::FxHashSet::default(),
    };

    let global = resolver.symbols.global_scope();

    for import in &program.imports {
        // Collect alias for import
        if let arandu_parser::ImportDecl::ExternalAlias { source, alias, .. } = import {
            resolver
                .import_aliases
                .insert(alias.clone(), source.clone());
        }

        resolver.collect_import(global, import);

        // Builtin prelude (`import io`, `import err`): members already live in
        // the symbol table from `define_prelude`. Do not require on-disk files.
        // Prefer a real file if one is registered; otherwise short-circuit.
        if let arandu_parser::ImportDecl::ModuleAlias { path, alias, .. } = import
            && let Some(prelude_name) = prelude_module_from_path(path)
        {
            let file_key = format!("{prelude_name}.aru");
            if db.resolve_module_path(&file_key).is_none() {
                // Alias points at the prelude module key used by module_members.
                if alias.as_str() != prelude_name {
                    resolver
                        .import_aliases
                        .insert(alias.clone(), SmolStr::new(prelude_name));
                    // Typeck looks up members by the in-scope name (`out.println`
                    // after `import io as out`), so mirror the prelude members
                    // under the alias key as well.
                    if let Some(members) =
                        resolver.symbols.module_members.get(prelude_name).cloned()
                    {
                        resolver
                            .symbols
                            .module_members
                            .insert(alias.clone(), members);
                    }
                }
                continue;
            }
        }

        // Merge exports from DB (single path helper — RC-PATH-TRIPLE).
        let module_path = crate::canonicalize_import_path(import);

        if let Some(path) = &module_path {
            if let Some(imported_file) = db.resolve_module_path(path) {
                let exports = db.exported_symbols(imported_file);
                match import {
                    arandu_parser::ImportDecl::ModuleAlias { alias, .. }
                    | arandu_parser::ImportDecl::ExternalAlias { alias, .. } => {
                        let module_name = alias.clone();
                        for (name, &(id, kind)) in &exports.symbols {
                            let sym = arandu_middle::Symbol {
                                id,
                                name: name.clone().into(),
                                kind,
                                span: import.span(),
                                scope: global,
                            };
                            resolver.symbols.register_imported_symbol(sym);
                            resolver
                                .symbols
                                .module_members
                                .entry(module_name.clone())
                                .or_default()
                                .insert(name.clone().into(), id);
                            // Root of T025 across modules: associated methods are
                            // exported as `"Type.method"` but interface satisfaction
                            // looks up `associated_members[Type][method]`. Rebuild
                            // that index on import so GlobalAllocator.alloc is found.
                            if matches!(kind, arandu_middle::SymbolKind::AssociatedFunc)
                                && let Some((ty, method)) = name.rsplit_once('.')
                            {
                                resolver
                                    .symbols
                                    .associated_members
                                    .entry(smol_str::SmolStr::new(ty))
                                    .or_default()
                                    .insert(smol_str::SmolStr::new(method), id);
                            }
                        }
                    }
                    arandu_parser::ImportDecl::Named { items, .. }
                    | arandu_parser::ImportDecl::ExternalNamed { items, .. } => {
                        for item in items {
                            if let Some(&(id, kind)) = exports.symbols.get(item.name.as_str()) {
                                let import_name = item.alias.as_ref().unwrap_or(&item.name).clone();
                                let sym = arandu_middle::Symbol {
                                    id,
                                    name: import_name.clone(),
                                    kind,
                                    span: item.span,
                                    scope: global,
                                };
                                if let Err(existing) = resolver.symbols.insert_imported(sym) {
                                    let existing_span = resolver.symbols.get(existing).span;
                                    resolver.diagnostics.push(
                                        arandu_middle::Diagnostic::error(
                                            arandu_middle::DiagCode::N006ImportConflict,
                                            format!(
                                                "import `{}` conflicts with an existing declaration",
                                                import_name
                                            ),
                                            item.span,
                                        )
                                        .with_label(existing_span, "already defined here"),
                                    );
                                }
                            }
                        }
                    }
                }
            } else if db.missing_import_is_error() {
                let import_name = match import {
                    arandu_parser::ImportDecl::ModuleAlias { path, .. }
                    | arandu_parser::ImportDecl::Named { path, .. } => path.join("."),
                    arandu_parser::ImportDecl::ExternalAlias { source, .. }
                    | arandu_parser::ImportDecl::ExternalNamed { source, .. } => source.to_string(),
                };
                resolver.diagnostics.push(arandu_middle::Diagnostic::error(
                    arandu_middle::DiagCode::M001UnresolvedImport,
                    format!("unresolved import: `{}`", import_name),
                    import.span(),
                ));
            }
        } else if db.missing_import_is_error() {
            let import_name = match import {
                arandu_parser::ImportDecl::ModuleAlias { path, .. }
                | arandu_parser::ImportDecl::Named { path, .. } => path.join("."),
                arandu_parser::ImportDecl::ExternalAlias { source, .. }
                | arandu_parser::ImportDecl::ExternalNamed { source, .. } => source.to_string(),
            };
            resolver.diagnostics.push(arandu_middle::Diagnostic::error(
                arandu_middle::DiagCode::M001UnresolvedImport,
                format!("unresolved import: `{}`", import_name),
                import.span(),
            ));
        }
    }

    for decl_id in &program.decls {
        let decl = resolver.pool.decl(*decl_id);
        resolver.resolve_top_level(global, decl);
    }

    resolver.check_unused_imports();

    ResolutionResult {
        is_cycle_fallback: false,
        symbols: resolver.symbols,
        resolved: resolver.resolved,
        docs: resolver.docs,
        diagnostics: resolver.diagnostics,
    }
}

#[must_use]
#[tracing::instrument(level = "trace", target = "arandu_resolve", skip(program))]
pub fn collect_symbols(
    program: &Program,
) -> (
    SymbolTable,
    ResolvedNames,
    crate::DocCommentMap,
    Vec<crate::Diagnostic>,
) {
    let mut resolver = Resolver {
        symbols: SymbolTable::new(0),
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
            .push(doc.text.to_string());
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
        if let arandu_parser::ImportDecl::ExternalAlias { source, alias, .. } = import {
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
        is_cycle_fallback: false,
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
    import_aliases: rustc_hash::FxHashMap<SmolStr, SmolStr>,
    current_module: Option<String>,
    imported_symbols: rustc_hash::FxHashMap<crate::SymbolId, (SmolStr, arandu_lexer::Span)>,
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
        name: SmolStr,
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

#[cfg(test)]
mod tests {
    use arandu_lexer::Span;
    use smallvec::SmallVec;
    use smol_str::SmolStr;

    use crate::{DiagCode, NodeKey, ResolvedNames, ScopeId, SymbolId, SymbolKind, SymbolTable};

    use super::Resolver;
    use super::util::is_type_case;

    fn dummy_span() -> Span {
        Span::new(0, 0, 0)
    }

    fn new_pool() -> arandu_parser::ast_pool::AstPool {
        arandu_parser::ast_pool::AstPool::new()
    }

    fn make_resolver(pool: &arandu_parser::ast_pool::AstPool) -> Resolver<'_> {
        Resolver {
            symbols: SymbolTable::new(0),
            resolved: ResolvedNames::default(),
            docs: crate::DocCommentMap::default(),
            diagnostics: Vec::new(),
            pool,
            import_aliases: rustc_hash::FxHashMap::default(),
            current_module: None,
            imported_symbols: rustc_hash::FxHashMap::default(),
            used_symbols: rustc_hash::FxHashSet::default(),
        }
    }

    fn resolver_no_pool() -> Resolver<'static> {
        // Only used for tests that don't touch the pool
        let pool = Box::new(arandu_parser::ast_pool::AstPool::new());
        make_resolver(Box::leak(pool))
    }

    fn dummy_expr() -> arandu_parser::Expr {
        arandu_parser::Expr::new(0)
    }

    fn dummy_block() -> arandu_parser::Block {
        arandu_parser::Block {
            span: dummy_span(),
            statements: Vec::new(),
        }
    }

    fn dummy_type_name(name: &str) -> arandu_parser::TypeName {
        arandu_parser::TypeName {
            span: dummy_span(),
            path: vec![SmolStr::new(name)].into(),
        }
    }

    // ── is_type_case ──

    #[test]
    fn is_type_case_upper() {
        assert!(is_type_case("Int"));
        assert!(is_type_case("String"));
        assert!(is_type_case("MyType"));
    }

    #[test]
    fn is_type_case_lower() {
        assert!(!is_type_case("int"));
        assert!(!is_type_case("x"));
        assert!(!is_type_case("my_var"));
    }

    // ── define ──

    #[test]
    fn define_new_symbol() {
        let mut r = resolver_no_pool();
        let sym = r.define(ScopeId(0), "x", SymbolKind::Local, dummy_span());
        assert!(sym.is_some());
        assert_eq!(r.symbols.get(sym.unwrap()).name, "x");
    }

    #[test]
    fn define_duplicate_in_same_scope_returns_none() {
        let mut r = resolver_no_pool();
        let _ = r.define(ScopeId(0), "x", SymbolKind::Local, dummy_span());
        let dup = r.define(ScopeId(0), "x", SymbolKind::Local, dummy_span());
        assert!(dup.is_none());
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].code, DiagCode::N003RedefinedName);
    }

    /// Methods restate receiver type params (`func Vec.push<T, A>(…)` after
    /// `import_receiver_type_params`). Must reuse SymbolIds, not emit N003.
    #[test]
    fn method_restated_type_params_reuse_existing_bindings() {
        let source = r#"
module t
struct BoxG<T> {
    v: T
}
func BoxG.get<T>(shared self): T {
    return self.v
}
"#;
        let program = arandu_parser::parse(source).expect("parse");
        let r = crate::resolve_for_test(0, &program);
        let n003: Vec<_> = r
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagCode::N003RedefinedName)
            .collect();
        assert!(
            n003.is_empty(),
            "restating receiver type params must not redefine: {n003:?}"
        );
    }

    #[test]
    fn define_duplicate_module_returns_previous() {
        let mut r = resolver_no_pool();
        let a = r.define(ScopeId(0), "mymod", SymbolKind::Module, dummy_span());
        let b = r.define(ScopeId(0), "mymod", SymbolKind::Module, dummy_span());
        assert_eq!(a, b);
        assert!(r.diagnostics.is_empty());
    }

    // ── is_namespace ──

    #[test]
    fn is_namespace_for_module() {
        let mut r = resolver_no_pool();
        r.define(ScopeId(0), "io", SymbolKind::Module, dummy_span());
        assert!(r.is_namespace(ScopeId(0), "io"));
        assert!(!r.is_namespace(ScopeId(0), "nonexistent"));
    }

    #[test]
    fn is_namespace_for_import_value() {
        let mut r = resolver_no_pool();
        r.define(ScopeId(0), "fmt", SymbolKind::ImportValue, dummy_span());
        assert!(r.is_namespace(ScopeId(0), "fmt"));
    }

    // ── expand_namespace_alias ──

    #[test]
    fn expand_namespace_no_alias() {
        let r = resolver_no_pool();
        assert_eq!(r.expand_namespace_alias("io.println"), "io.println");
    }

    #[test]
    fn expand_namespace_with_alias() {
        let mut r = resolver_no_pool();
        r.import_aliases
            .insert("fmt".into(), "std.core.format".into());
        assert_eq!(
            r.expand_namespace_alias("fmt.println"),
            "std.core.format.println"
        );
    }

    #[test]
    fn expand_namespace_alias_only() {
        let mut r = resolver_no_pool();
        r.import_aliases.insert("io".into(), "std.core.io".into());
        assert_eq!(r.expand_namespace_alias("io"), "std.core.io");
    }

    // ── suggest_from ──

    #[test]
    fn suggest_from_exact_match() {
        let r = resolver_no_pool();
        let syms = vec![crate::Symbol {
            id: SymbolId::new(0, 0),
            name: "println".into(),
            kind: SymbolKind::Func,
            span: dummy_span(),
            scope: ScopeId(0),
        }];
        assert_eq!(
            r.suggest_from("println", &syms),
            Some("println".to_string())
        );
    }

    #[test]
    fn suggest_from_levenshtein() {
        let r = resolver_no_pool();
        let syms = vec![crate::Symbol {
            id: SymbolId::new(0, 0),
            name: "println".into(),
            kind: SymbolKind::Func,
            span: dummy_span(),
            scope: ScopeId(0),
        }];
        assert_eq!(r.suggest_from("prntln", &syms), Some("println".to_string()));
    }

    #[test]
    fn suggest_from_no_match() {
        let r = resolver_no_pool();
        let syms = vec![crate::Symbol {
            id: SymbolId::new(0, 0),
            name: "println".into(),
            kind: SymbolKind::Func,
            span: dummy_span(),
            scope: ScopeId(0),
        }];
        assert_eq!(r.suggest_from("abcdef", &syms), None);
    }

    #[test]
    fn suggest_from_case_insensitive() {
        let r = resolver_no_pool();
        let syms = vec![crate::Symbol {
            id: SymbolId::new(0, 0),
            name: "Println".into(),
            kind: SymbolKind::Func,
            span: dummy_span(),
            scope: ScopeId(0),
        }];
        assert_eq!(
            r.suggest_from("println", &syms),
            Some("Println".to_string())
        );
    }

    // ── check_unused_imports ──

    #[test]
    fn unused_import_emits_warning() {
        let mut r = resolver_no_pool();
        let sym = r
            .define(ScopeId(0), "foo", SymbolKind::ImportValue, dummy_span())
            .unwrap();
        r.imported_symbols.insert(sym, ("foo".into(), dummy_span()));
        r.check_unused_imports();
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].code, DiagCode::W007UnusedImport);
    }

    #[test]
    fn used_import_no_warning() {
        let mut r = resolver_no_pool();
        let sym = r
            .define(ScopeId(0), "foo", SymbolKind::ImportValue, dummy_span())
            .unwrap();
        r.imported_symbols.insert(sym, ("foo".into(), dummy_span()));
        r.used_symbols.insert(sym);
        r.check_unused_imports();
        assert!(r.diagnostics.is_empty());
    }

    // ── collect_import ──

    #[test]
    fn collect_import_module_defines_symbol() {
        let mut r = resolver_no_pool();
        let import = arandu_parser::ImportDecl::ModuleAlias {
            span: dummy_span(),
            path: vec![SmolStr::new("std"), SmolStr::new("io")].into(),
            alias: SmolStr::new("std"),
        };
        r.collect_import(ScopeId(0), &import);
        let sym = r.symbols.lookup_module(ScopeId(0), "std");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_import_module_empty_path_emits_error() {
        let mut r = resolver_no_pool();
        let import = arandu_parser::ImportDecl::ModuleAlias {
            span: dummy_span(),
            path: SmallVec::new(),
            alias: SmolStr::new("empty"),
        };
        r.collect_import(ScopeId(0), &import);
    }

    #[test]
    fn collect_import_named_type_case() {
        let mut r = resolver_no_pool();
        let import = arandu_parser::ImportDecl::Named {
            span: dummy_span(),
            path: vec![SmolStr::new("std")].into(),
            items: vec![arandu_parser::ImportItem {
                span: dummy_span(),
                name: SmolStr::new("String"),
                alias: None,
            }],
        };
        r.collect_import(ScopeId(0), &import);
        let sym = r.symbols.lookup_type(ScopeId(0), "String");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_import_named_value_case() {
        let mut r = resolver_no_pool();
        let import = arandu_parser::ImportDecl::Named {
            span: dummy_span(),
            path: vec![SmolStr::new("std")].into(),
            items: vec![arandu_parser::ImportItem {
                span: dummy_span(),
                name: SmolStr::new("println"),
                alias: None,
            }],
        };
        r.collect_import(ScopeId(0), &import);
        let sym = r.symbols.lookup_value(ScopeId(0), "println");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_import_named_with_alias() {
        let mut r = resolver_no_pool();
        let import = arandu_parser::ImportDecl::Named {
            span: dummy_span(),
            path: vec![SmolStr::new("std")].into(),
            items: vec![arandu_parser::ImportItem {
                span: dummy_span(),
                name: SmolStr::new("println"),
                alias: Some(SmolStr::new("print")),
            }],
        };
        r.collect_import(ScopeId(0), &import);
        let sym = r.symbols.lookup_value(ScopeId(0), "print");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_import_external() {
        let mut r = resolver_no_pool();
        let import = arandu_parser::ImportDecl::ExternalAlias {
            span: dummy_span(),
            source: SmolStr::new("std.core.io"),
            alias: SmolStr::new("io"),
        };
        r.collect_import(ScopeId(0), &import);
        let sym = r.symbols.lookup_module(ScopeId(0), "io");
        assert!(sym.is_some());
        assert_eq!(
            r.import_aliases.get("io"),
            Some(&SmolStr::new("std.core.io"))
        );
    }

    // ── collect_top_level ──

    #[test]
    fn collect_top_level_const() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::Const(arandu_parser::ConstDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            name: "MAX".into(),
            ty: None,
            value: dummy_expr(),
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_value(ScopeId(0), "MAX");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_type_alias() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::TypeAlias(arandu_parser::TypeAliasDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            name: "MyInt".into(),
            generic_params: Vec::new().into(),
            ty: arandu_parser::TypeExprId::new(0),
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_type(ScopeId(0), "MyInt");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_func_free() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::Func(arandu_parser::FuncDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            is_async: false,
            name: arandu_parser::FuncName::Free {
                span: dummy_span(),
                name: "main".into(),
            },
            generic_params: Vec::new().into(),
            params: Vec::new(),
            result: None,
            where_clause: Vec::new().into(),
            body: dummy_block(),
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_value(ScopeId(0), "main");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_method() {
        let mut r = resolver_no_pool();
        let _ = r.define(ScopeId(0), "Foo", SymbolKind::Struct, dummy_span());
        let decl = arandu_parser::TopLevelDecl::Func(arandu_parser::FuncDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            is_async: false,
            name: arandu_parser::FuncName::Method {
                span: dummy_span(),
                receiver: dummy_type_name("Foo"),
                name: "bar".into(),
            },
            generic_params: Vec::new().into(),
            params: Vec::new(),
            result: None,
            where_clause: Vec::new().into(),
            body: dummy_block(),
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_associated_member("Foo", "bar");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_struct() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::Struct(arandu_parser::StructDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            name: "Point".into(),
            generic_params: Vec::new().into(),
            where_clause: Vec::new().into(),
            fields: Vec::new(),
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_type(ScopeId(0), "Point");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_enum_with_variants() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::Enum(arandu_parser::EnumDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            name: "Color".into(),
            generic_params: Vec::new().into(),
            where_clause: Vec::new().into(),
            variants: vec![
                arandu_parser::EnumVariant {
                    span: dummy_span(),
                    attrs: Vec::new().into(),
                    name: "Red".into(),
                    payload: None,
                },
                arandu_parser::EnumVariant {
                    span: dummy_span(),
                    attrs: Vec::new().into(),
                    name: "Blue".into(),
                    payload: None,
                },
            ],
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_type(ScopeId(0), "Color");
        assert!(sym.is_some());
        assert!(r.symbols.lookup_associated_member("Color", "Red").is_some());
        assert!(
            r.symbols
                .lookup_associated_member("Color", "Blue")
                .is_some()
        );
    }

    #[test]
    fn collect_top_level_interface() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::Interface(arandu_parser::InterfaceDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            visibility: arandu_parser::Visibility::Private,
            name: "Stringable".into(),
            generic_params: Vec::new().into(),
            where_clause: Vec::new().into(),
            members: Vec::new(),
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_type(ScopeId(0), "Stringable");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_extern() {
        let mut r = resolver_no_pool();
        let decl = arandu_parser::TopLevelDecl::Extern(arandu_parser::ExternDecl {
            span: dummy_span(),
            attrs: Vec::new().into(),
            abi: "C".into(),
            members: vec![arandu_parser::FuncSignature {
                span: dummy_span(),
                attrs: Vec::new().into(),
                name: "malloc".into(),
                generic_params: Vec::new().into(),
                params: Vec::new(),
                result: None,
                where_clause: Vec::new().into(),
            }],
        });
        r.collect_top_level(ScopeId(0), &decl);
        let sym = r.symbols.lookup_value(ScopeId(0), "malloc");
        assert!(sym.is_some());
    }

    #[test]
    fn collect_top_level_error_is_noop() {
        let mut r = resolver_no_pool();
        r.collect_top_level(
            ScopeId(0),
            &arandu_parser::TopLevelDecl::Error(dummy_span()),
        );
        assert_eq!(r.symbols.iter().count(), 0);
    }

    // ── resolve_value_name ──

    #[test]
    fn resolve_value_name_found_in_scope() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        let sym = r
            .define(ScopeId(0), "x", SymbolKind::Local, dummy_span())
            .unwrap();
        r.resolve_value_name(ScopeId(0), "x", expr, dummy_span());
        assert_eq!(r.resolved.expr_symbol(expr), Some(sym));
    }

    #[test]
    fn resolve_value_name_undefined() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        r.resolve_value_name(ScopeId(0), "nonexistent", expr, dummy_span());
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].code, DiagCode::N001UndefinedValue);
    }

    #[test]
    fn resolve_value_name_type_used_as_value() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        let _ = r.define(ScopeId(0), "MyType", SymbolKind::Struct, dummy_span());
        r.resolve_value_name(ScopeId(0), "MyType", expr, dummy_span());
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].code, DiagCode::N004TypeUsedAsValue);
    }

    #[test]
    fn resolve_value_name_namespace_used_as_value() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        let _ = r.define(ScopeId(0), "io", SymbolKind::Module, dummy_span());
        r.resolve_value_name(ScopeId(0), "io", expr, dummy_span());
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(r.diagnostics[0].code, DiagCode::M003NamespaceUsedAsValue);
    }

    #[test]
    fn resolve_value_name_with_current_module() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let pool2 = new_pool();
        let mut r = make_resolver(&pool2);
        r.current_module = Some("mymod".to_string());
        let _ = r
            .symbols
            .define_module_member("mymod", "foo", dummy_span())
            .unwrap();
        r.resolve_value_name(ScopeId(0), "foo", expr, dummy_span());
        let sym = r.symbols.lookup_module_member("mymod", "foo");
        assert_eq!(r.resolved.expr_symbol(expr), sym);
    }

    // ── resolve_assignment_target ──

    #[test]
    fn resolve_assignment_target_found() {
        let mut r = resolver_no_pool();
        let _ = r
            .define(ScopeId(0), "x", SymbolKind::Local, dummy_span())
            .unwrap();
        r.resolve_assignment_target(ScopeId(0), "x", dummy_span());
        assert!(
            r.resolved
                .value_refs
                .contains_key(&NodeKey::from(dummy_span()))
        );
    }

    #[test]
    fn resolve_assignment_target_undefined() {
        let mut r = resolver_no_pool();
        r.resolve_assignment_target(ScopeId(0), "nonexistent", dummy_span());
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(
            r.diagnostics[0].code,
            DiagCode::N007UndefinedAssignmentTarget
        );
    }

    // ── resolve_namespace_member ──

    #[test]
    fn resolve_namespace_member_found() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        let _ = r.define(ScopeId(0), "io", SymbolKind::Module, dummy_span());
        let sym = r
            .symbols
            .define_module_member("io", "println", dummy_span())
            .unwrap();
        let found = r.resolve_namespace_member(ScopeId(0), "io", "println", expr, dummy_span());
        assert!(found);
        assert_eq!(r.resolved.expr_symbol(expr), Some(sym));
    }

    #[test]
    fn resolve_namespace_member_not_namespace() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        let found =
            r.resolve_namespace_member(ScopeId(0), "nonexistent", "foo", expr, dummy_span());
        assert!(!found);
    }

    #[test]
    fn resolve_namespace_member_undefined_member() {
        let mut pool = new_pool();
        let expr = pool.alloc_expr(arandu_parser::ExprKind::Nil, dummy_span());
        let mut r = make_resolver(&pool);
        let _ = r.define(ScopeId(0), "io", SymbolKind::Module, dummy_span());
        let found = r.resolve_namespace_member(ScopeId(0), "io", "nonexistent", expr, dummy_span());
        assert!(found);
        assert_eq!(r.diagnostics.len(), 1);
        assert_eq!(
            r.diagnostics[0].code,
            DiagCode::M002UndefinedNamespaceMember
        );
    }
}
