use std::collections::HashMap;

use arandu_lexer::Span;

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
    pub value_refs: HashMap<NodeKey, SymbolId>,
    pub type_refs: HashMap<NodeKey, SymbolId>,
}

impl ResolvedNames {
    pub fn define(&mut self, span: Span, symbol: SymbolId) {
        self.definitions.insert(span.into(), symbol);
    }

    pub fn value_ref(&mut self, span: Span, symbol: SymbolId) {
        self.value_refs.insert(span.into(), symbol);
    }

    pub fn type_ref(&mut self, span: Span, symbol: SymbolId) {
        self.type_refs.insert(span.into(), symbol);
    }
}
