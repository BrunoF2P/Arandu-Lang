use arandu_middle::newtype_index;
use arandu_middle::symbol_table::{SymbolId, SymbolTable};
use arandu_middle::types::{TypeId, TypeInterner};
use rustc_hash::FxHashMap;

newtype_index!(InstantiationNodeId);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstantiationKey {
    pub symbol: SymbolId,
    pub type_args: Vec<TypeId>,
}

#[derive(Debug, Clone)]
pub struct InstantiationNode {
    pub id: InstantiationNodeId,
    pub key: InstantiationKey,
    pub mangled_name: String,
    pub callees: Vec<InstantiationNodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonoError {
    RecursionLimitExceeded { symbol: SymbolId, limit: usize },
}

#[derive(Debug)]
pub struct InstantiationGraph {
    nodes: Vec<InstantiationNode>,
    index: FxHashMap<InstantiationKey, InstantiationNodeId>,
    instantiation_counts: FxHashMap<SymbolId, usize>,
    recursion_limit: usize,
}

impl InstantiationGraph {
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            index: FxHashMap::default(),
            instantiation_counts: FxHashMap::default(),
            recursion_limit: 64,
        }
    }

    #[must_use]
    pub fn with_recursion_limit(limit: usize) -> Self {
        Self {
            nodes: Vec::new(),
            index: FxHashMap::default(),
            instantiation_counts: FxHashMap::default(),
            recursion_limit: limit,
        }
    }

    pub fn get_or_insert(
        &mut self,
        key: &InstantiationKey,
        interner: &TypeInterner,
        symbols: &SymbolTable,
    ) -> Result<InstantiationNodeId, MonoError> {
        if let Some(&id) = self.index.get(key) {
            return Ok(id);
        }

        let same_symbol_count = self
            .instantiation_counts
            .get(&key.symbol)
            .copied()
            .unwrap_or(0);
        if same_symbol_count >= self.recursion_limit {
            return Err(MonoError::RecursionLimitExceeded {
                symbol: key.symbol,
                limit: self.recursion_limit,
            });
        }

        let mangled = super::demangle::mangle_symbol(key, interner, symbols);
        let id = InstantiationNodeId::from_usize(self.nodes.len());
        self.nodes.push(InstantiationNode {
            id,
            key: key.clone(),
            mangled_name: mangled,
            callees: Vec::new(),
        });
        self.index.insert(key.clone(), id);
        *self.instantiation_counts.entry(key.symbol).or_insert(0) += 1;
        Ok(id)
    }

    pub fn add_edge(&mut self, caller: InstantiationNodeId, callee: InstantiationNodeId) {
        self.nodes[caller.as_usize()].callees.push(callee);
    }

    #[must_use]
    pub fn get_node(&self, id: InstantiationNodeId) -> &InstantiationNode {
        &self.nodes[id.as_usize()]
    }

    #[must_use]
    pub fn lookup(&self, key: &InstantiationKey) -> Option<InstantiationNodeId> {
        self.index.get(key).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &InstantiationNode> {
        self.nodes.iter()
    }

    #[must_use]
    pub fn find_cycle(&self) -> Option<Vec<InstantiationNodeId>> {
        let n = self.nodes.len();
        let mut visited = vec![false; n];
        let mut on_stack = vec![false; n];
        let mut path = Vec::new();

        for i in 0..n {
            if !visited[i]
                && let Some(cycle) = self.dfs_cycle(
                    InstantiationNodeId::from_usize(i),
                    &mut visited,
                    &mut on_stack,
                    &mut path,
                )
            {
                return Some(cycle);
            }
        }
        None
    }

    fn dfs_cycle(
        &self,
        node: InstantiationNodeId,
        visited: &mut Vec<bool>,
        on_stack: &mut Vec<bool>,
        path: &mut Vec<InstantiationNodeId>,
    ) -> Option<Vec<InstantiationNodeId>> {
        let idx = node.as_usize();
        visited[idx] = true;
        on_stack[idx] = true;
        path.push(node);

        for &callee in &self.nodes[idx].callees {
            let ci = callee.as_usize();
            if !visited[ci] {
                if let Some(cycle) = self.dfs_cycle(callee, visited, on_stack, path) {
                    return Some(cycle);
                }
            } else if on_stack[ci] {
                let start = path.iter().position(|&n| n == callee).unwrap_or(0);
                return Some(path[start..].to_vec());
            }
        }

        path.pop();
        on_stack[idx] = false;
        None
    }
}

impl Default for InstantiationGraph {
    fn default() -> Self {
        Self::new()
    }
}
