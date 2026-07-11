use arandu_lexer::Span;
use arandu_parser::Program;

use crate::{DocCommentMap, NodeKey, ResolutionResult, ResolvedNames, SymbolKind, SymbolTable};

use super::Resolver;

impl<'a> Resolver<'a> {
    pub(crate) fn new(
        file_id: u32,
        pool: &'a arandu_parser::ast_pool::AstPool,
        program: Option<&Program>,
    ) -> Self {
        let current_module = program.and_then(|p| p.module.as_ref().map(|m| m.path.join(".")));
        let mut resolver = Self {
            symbols: SymbolTable::new(file_id),
            resolved: ResolvedNames::default(),
            docs: DocCommentMap::default(),
            diagnostics: Vec::new(),
            pool,
            import_aliases: rustc_hash::FxHashMap::default(),
            current_module,
            imported_symbols: rustc_hash::FxHashMap::default(),
            used_symbols: rustc_hash::FxHashSet::default(),
        };
        resolver.define_prelude(program);
        resolver.symbols.setup_prelude_scope();
        resolver
    }

    pub(crate) fn resolve_local(mut self, program: &Program) -> ResolutionResult {
        for doc in &program.docs {
            self.docs
                .entry(NodeKey::from(doc.target_span))
                .or_default()
                .push(doc.text.to_string());
        }

        let global = self.symbols.global_scope();
        if let Some(module) = &program.module
            && let Some(root) = module.path.first()
        {
            self.define(global, root, SymbolKind::Module, module.span);
        }

        for decl_id in &program.decls {
            let decl = self.pool.decl(*decl_id);
            self.collect_top_level(global, decl);
        }

        ResolutionResult {
            is_cycle_fallback: false,
            symbols: self.symbols,
            resolved: self.resolved,
            docs: self.docs,
            diagnostics: self.diagnostics,
        }
    }

    pub(crate) fn define_prelude(&mut self, _program: Option<&Program>) {
        let span = Span::new(0, 0, 0);
        for (module, members) in super::PRELUDE_MODULE_MEMBERS {
            for member in *members {
                let _ = self.symbols.define_module_member(module, member, span);
            }
        }
        let global_scope = self.symbols.global_scope();
        self.symbols.builtin_alloc = self
            .symbols
            .define(global_scope, "alloc", SymbolKind::Func, span)
            .ok();
        self.symbols.builtin_free = self
            .symbols
            .define(global_scope, "free", SymbolKind::Func, span)
            .ok();

        let _ = self
            .symbols
            .define(global_scope, "Result", SymbolKind::Enum, span);
        let _ = self
            .symbols
            .define(global_scope, "Option", SymbolKind::Enum, span);
        let _ = self
            .symbols
            .define(global_scope, "Coroutine", SymbolKind::Enum, span);
        let _ = self
            .symbols
            .define(global_scope, "Poll", SymbolKind::Enum, span);

        let global = self.symbols.global_scope();
        let has_result = self.symbols.lookup_type(global, "Result").is_some();
        let has_option = self.symbols.lookup_type(global, "Option").is_some();
        let has_poll = self.symbols.lookup_type(global, "Poll").is_some();
        tracing::debug!(target: "arandu_resolve", has_result, has_option, has_poll, "Prelude types in scope");
        tracing::debug!(target: "arandu_resolve", total = self.symbols.iter().count(), "Symbol table after prelude load");
    }
}
