use std::collections::HashMap;

use arandu_lexer::Span;
use arandu_parser::ast_pool::ExprId;

use crate::SymbolId;

pub type DocCommentMap = HashMap<NodeKey, Vec<String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeKey {
    pub start: usize,
    pub end: usize,
}

impl From<Span> for NodeKey {
    fn from(span: Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedNames {
    pub definitions: HashMap<NodeKey, SymbolId>,
    pub expr_symbols: Vec<Option<SymbolId>>,
    pub value_refs: HashMap<NodeKey, SymbolId>,
    pub type_refs: HashMap<NodeKey, SymbolId>,
}

impl ResolvedNames {
    pub fn define(&mut self, span: Span, symbol: SymbolId) {
        self.definitions.insert(span.into(), symbol);
    }

    pub fn expr_ref(&mut self, expr: ExprId, symbol: SymbolId) {
        let idx = expr.as_usize();
        if self.expr_symbols.len() <= idx {
            self.expr_symbols.resize(idx + 1, None);
        }
        self.expr_symbols[idx] = Some(symbol);
    }

    #[must_use]
    pub fn expr_symbol(&self, expr: ExprId) -> Option<SymbolId> {
        self.expr_symbols
            .get(expr.as_usize())
            .and_then(|symbol| *symbol)
    }

    pub fn value_ref(&mut self, span: Span, symbol: SymbolId) {
        self.value_refs.insert(span.into(), symbol);
    }

    pub fn type_ref(&mut self, span: Span, symbol: SymbolId) {
        self.type_refs.insert(span.into(), symbol);
    }
}
