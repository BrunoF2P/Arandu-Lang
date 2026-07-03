use arandu_lexer::Span;
use arandu_middle::parse_cache::ParseCache;
use arandu_middle::StdlibPathCache;
use arandu_parser::Program;

use crate::{DocCommentMap, NodeKey, ResolutionResult, ResolvedNames, SymbolKind, SymbolTable};

use super::Resolver;

impl<'a> Resolver<'a> {
    pub(crate) fn new(
        pool: &'a arandu_parser::ast_pool::AstPool,
        program: Option<&Program>,
        cache: &mut ParseCache,
        stdlib_cache: &mut StdlibPathCache,
    ) -> Self {
        let current_module = program.and_then(|p| p.module.as_ref().map(|m| m.path.join(".")));
        let mut resolver = Self {
            symbols: SymbolTable::new(),
            resolved: ResolvedNames::default(),
            docs: DocCommentMap::default(),
            diagnostics: Vec::new(),
            pool,
            import_aliases: rustc_hash::FxHashMap::default(),
            current_module,
            imported_symbols: rustc_hash::FxHashMap::default(),
            used_symbols: rustc_hash::FxHashSet::default(),
        };
        resolver.define_prelude(program, cache, stdlib_cache);
        resolver.symbols.setup_prelude_scope();
        resolver
    }

    pub(crate) fn resolve_program(mut self, program: &Program) -> ResolutionResult {
        for doc in &program.docs {
            self.docs
                .entry(NodeKey::from(doc.target_span))
                .or_default()
                .push(doc.text.clone());
        }

        let global = self.symbols.global_scope();
        if let Some(module) = &program.module
            && let Some(root) = module.path.first()
        {
            self.define(global, root, SymbolKind::Module, module.span);
        }

        for import in &program.imports {
            self.collect_import(global, import);
        }

        for decl_id in &program.decls {
            let decl = self.pool.decl(*decl_id);
            self.collect_top_level(global, decl);
        }

        for decl_id in &program.decls {
            let decl = self.pool.decl(*decl_id);
            self.resolve_top_level(global, decl);
        }

        self.check_unused_imports();

        ResolutionResult {
            symbols: self.symbols,
            resolved: self.resolved,
            docs: self.docs,
            diagnostics: self.diagnostics,
        }
    }

    pub(crate) fn define_prelude(
        &mut self,
        program: Option<&Program>,
        cache: &mut ParseCache,
        stdlib_cache: &mut StdlibPathCache,
    ) {
        let span = Span::new(0, 0, 0);
        for (module, members) in [
            ("io", ["println", "create", "remove"].as_slice()),
            ("err", ["new"].as_slice()),
        ] {
            for member in members {
                let _ = self.symbols.define_module_member(module, member, span);
            }
        }
        let current_module = program.and_then(|p| p.module.as_ref().map(|m| m.path.join(".")));
        super::load_stdlib_transitively(
            &mut self.symbols,
            "stdlib/core/prelude.aru",
            current_module.as_deref(),
            program,
            cache,
            stdlib_cache,
        );
        let global = self.symbols.global_scope();
        let has_result = self.symbols.lookup_type(global, "Result").is_some();
        let has_option = self.symbols.lookup_type(global, "Option").is_some();
        tracing::debug!(target: "arandu_resolve", has_result, has_option, "Prelude types in scope");
        tracing::debug!(target: "arandu_resolve", total = self.symbols.iter().count(), "Symbol table after prelude load");
    }
}
