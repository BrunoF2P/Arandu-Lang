use arandu_lexer::Span;
use arandu_parser::Program;

use crate::{DocCommentMap, NodeKey, ResolutionResult, ResolvedNames, SymbolKind, SymbolTable};

use super::Resolver;

impl<'a> Resolver<'a> {
    pub(crate) fn new(pool: &'a arandu_parser::ast_pool::AstPool) -> Self {
        let mut resolver = Self {
            symbols: SymbolTable::new(),
            resolved: ResolvedNames::default(),
            docs: DocCommentMap::default(),
            diagnostics: Vec::new(),
            pool,
            import_aliases: rustc_hash::FxHashMap::default(),
        };
        resolver.define_prelude();
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

        ResolutionResult {
            symbols: self.symbols,
            resolved: self.resolved,
            docs: self.docs,
            diagnostics: self.diagnostics,
        }
    }

    pub(crate) fn define_prelude(&mut self) {
        let span = Span::new(0, 0, 0);
        for (module, members) in [
            ("io", ["println", "create", "remove"].as_slice()),
            ("err", ["new"].as_slice()),
        ] {
            for member in members {
                let _ = self.symbols.define_module_member(module, member, span);
            }
        }
    }
}
