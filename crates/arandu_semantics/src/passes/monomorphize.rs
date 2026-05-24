//! Monomorphization Pass
//!
//! This module implements the monomorphization infrastructure for Arandu generics.
//! Monomorphization creates specialized copies of generic functions and types for
//! each unique set of concrete type arguments used at call sites.
//!
//! ## Architecture
//!
//! 1. **`InstantiationKey`** — identifies a unique monomorphic instance:
//!    `(SymbolId, Vec<TypeId>)`.
//!
//! 2. **`InstantiationGraph`** — a directed graph where each node is a
//!    monomorphic instance. Edges represent callee relationships (e.g.
//!    `identity<int>` → `Box.new<int>`). Used for:
//!    - Recursion detection (cycle → error)
//!    - Recursion depth limiting (default: 64)
//!
//! 3. **Symbol Mangling** — generates collision-free linker names:
//!    `_A$module$name$I_int_E` (Arandu-prefixed, `$I` opens type args, `$E` closes).
//!
//! This module is designed to be invoked *after* type checking, operating on
//! the fully-typed HIR and using the `TypeInterner` for efficient type identity.

use crate::newtype_index;
use crate::passes::type_checker::types::{ArType, TypeId, TypeInterner};
use crate::SymbolId;
use crate::SymbolTable;
use std::collections::HashMap;

newtype_index!(InstantiationNodeId);

// ─── Instantiation Key ───────────────────────────────────────────────

/// A unique key identifying a monomorphic instantiation of a generic symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InstantiationKey {
    /// The original generic symbol (function, struct, etc.).
    pub symbol: SymbolId,
    /// The concrete type arguments, interned as `TypeId`s.
    pub type_args: Vec<TypeId>,
}

// ─── Instantiation Node ──────────────────────────────────────────────

/// A node in the instantiation graph representing a single monomorphic copy.
#[derive(Debug, Clone)]
pub struct InstantiationNode {
    pub id: InstantiationNodeId,
    /// The key that uniquely identifies this instantiation.
    pub key: InstantiationKey,
    /// The mangled symbol name for this instance.
    pub mangled_name: String,
    /// Edges to callees that this instance references.
    pub callees: Vec<InstantiationNodeId>,
}

// ─── Instantiation Graph ─────────────────────────────────────────────

/// The instantiation graph tracks all monomorphic instances of generic symbols
/// and their call relationships.
#[derive(Debug)]
pub struct InstantiationGraph {
    nodes: Vec<InstantiationNode>,
    /// Maps from key → node id for deduplication.
    index: HashMap<InstantiationKey, InstantiationNodeId>,
    /// Maximum recursion depth for generic instantiations.
    recursion_limit: usize,
}

impl InstantiationGraph {
    /// Create a new empty graph with the default recursion limit of 64.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            index: HashMap::new(),
            recursion_limit: 64,
        }
    }

    /// Create a new graph with a custom recursion limit.
    #[must_use]
    pub fn with_recursion_limit(limit: usize) -> Self {
        Self {
            nodes: Vec::new(),
            index: HashMap::new(),
            recursion_limit: limit,
        }
    }

    /// Look up or create a monomorphic instance for the given key.
    ///
    /// Returns `Ok(node_id)` if the instance was found or created successfully.
    /// Returns `Err(MonoError::RecursionLimitExceeded)` if instantiation would
    /// exceed the recursion depth limit.
    pub fn get_or_insert(
        &mut self,
        key: InstantiationKey,
        interner: &TypeInterner,
        symbols: &SymbolTable,
    ) -> Result<InstantiationNodeId, MonoError> {
        if let Some(&id) = self.index.get(&key) {
            return Ok(id);
        }

        // Check recursion depth by counting how many nodes share the same symbol
        let same_symbol_count = self
            .nodes
            .iter()
            .filter(|n| n.key.symbol == key.symbol)
            .count();
        if same_symbol_count >= self.recursion_limit {
            return Err(MonoError::RecursionLimitExceeded {
                symbol: key.symbol,
                limit: self.recursion_limit,
            });
        }

        let mangled = mangle_symbol(&key, interner, symbols);
        let id = InstantiationNodeId::from_usize(self.nodes.len());
        self.nodes.push(InstantiationNode {
            id,
            key: key.clone(),
            mangled_name: mangled,
            callees: Vec::new(),
        });
        self.index.insert(key, id);
        Ok(id)
    }

    /// Record that `caller` calls `callee`.
    pub fn add_edge(&mut self, caller: InstantiationNodeId, callee: InstantiationNodeId) {
        self.nodes[caller.as_usize()].callees.push(callee);
    }

    /// Get a node by its id.
    #[must_use]
    pub fn get_node(&self, id: InstantiationNodeId) -> &InstantiationNode {
        &self.nodes[id.as_usize()]
    }

    /// Look up an existing instantiation by key.
    #[must_use]
    pub fn lookup(&self, key: &InstantiationKey) -> Option<InstantiationNodeId> {
        self.index.get(key).copied()
    }

    /// Number of nodes in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterate over all instantiation nodes.
    pub fn iter(&self) -> impl Iterator<Item = &InstantiationNode> {
        self.nodes.iter()
    }

    /// Detect if any cycles exist in the instantiation graph (recursive generics).
    /// Returns the first cycle found as a vector of node IDs, or None.
    #[must_use]
    pub fn find_cycle(&self) -> Option<Vec<InstantiationNodeId>> {
        let n = self.nodes.len();
        let mut visited = vec![false; n];
        let mut on_stack = vec![false; n];
        let mut path = Vec::new();

        for i in 0..n {
            if !visited[i] {
                if let Some(cycle) = self.dfs_cycle(
                    InstantiationNodeId::from_usize(i),
                    &mut visited,
                    &mut on_stack,
                    &mut path,
                ) {
                    return Some(cycle);
                }
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
                // Found a cycle: extract the cycle from the path
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

// ─── Symbol Mangling ─────────────────────────────────────────────────

/// Mangle a generic instantiation key into a unique linker-safe name.
///
/// Format: `_A$<symbol_name>$I_<type1>_<type2>_$E`
///
/// Examples:
/// - `identity<int>` → `_A$identity$I_int_$E`
/// - `swap<int, str>` → `_A$swap$I_int_str_$E`
/// - `Box<List<int>>` → `_A$Box$I_List_int__$E`
#[must_use]
pub fn mangle_symbol(
    key: &InstantiationKey,
    interner: &TypeInterner,
    symbols: &SymbolTable,
) -> String {
    let name = &symbols.get(key.symbol).name;
    let mut mangled = format!("_A${name}$I");
    for (i, &tid) in key.type_args.iter().enumerate() {
        if i > 0 {
            mangled.push('_');
        }
        mangled.push('_');
        mangle_type_into(&mut mangled, interner.resolve(tid), symbols);
    }
    mangled.push_str("_$E");
    mangled
}

/// Demangle an `_A$...$E` name back to a human-readable form (best-effort).
#[must_use]
pub fn demangle_symbol(mangled: &str) -> Option<String> {
    let inner = mangled.strip_prefix("_A$")?.strip_suffix("$E")?;
    let (name, rest) = inner.split_once("$I")?;
    let types_part = rest.trim_matches('_');
    if types_part.is_empty() {
        Some(name.to_string())
    } else {
        let types: Vec<&str> = types_part.split('_').filter(|s| !s.is_empty()).collect();
        Some(format!("{}<{}>", name, types.join(", ")))
    }
}

fn mangle_type_into(out: &mut String, ty: &ArType, symbols: &SymbolTable) {
    match ty {
        ArType::Primitive(p) => out.push_str(p.as_str()),
        ArType::Named(id, args) => {
            out.push_str(&symbols.get(*id).name);
            for arg in args {
                out.push('_');
                mangle_type_into(out, arg, symbols);
            }
        }
        ArType::Nullable(inner) => {
            out.push_str("opt_");
            mangle_type_into(out, inner, symbols);
        }
        ArType::Ptr(inner) => {
            out.push_str("ptr_");
            mangle_type_into(out, inner, symbols);
        }
        ArType::Slice(inner) => {
            out.push_str("slice_");
            mangle_type_into(out, inner, symbols);
        }
        ArType::Array(n, inner) => {
            out.push_str(&format!("arr{n}_"));
            mangle_type_into(out, inner, symbols);
        }
        ArType::Tuple(items) => {
            out.push_str("tup");
            for item in items {
                out.push('_');
                mangle_type_into(out, item, symbols);
            }
        }
        ArType::Func(params, ret) => {
            out.push_str("fn");
            for param in params {
                out.push('_');
                mangle_type_into(out, param, symbols);
            }
            out.push_str("_R_");
            mangle_type_into(out, ret, symbols);
        }
        ArType::Result(ok, err) => {
            out.push_str("res_");
            mangle_type_into(out, ok, symbols);
            out.push('_');
            mangle_type_into(out, err, symbols);
        }
        ArType::Option(inner) => {
            out.push_str("option_");
            mangle_type_into(out, inner, symbols);
        }
        ArType::Void => out.push_str("void"),
        ArType::Err => out.push_str("err"),
        ArType::IntLiteral => out.push_str("int"),
        ArType::FloatLiteral => out.push_str("float"),
        ArType::Error => out.push_str("error"),
    }
}

// ─── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonoError {
    RecursionLimitExceeded {
        symbol: SymbolId,
        limit: usize,
    },
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::passes::type_checker::types::Primitive;

    fn setup() -> (SymbolTable, TypeInterner) {
        let st = SymbolTable::new();
        let interner = TypeInterner::new();
        (st, interner)
    }

    fn define_symbol(st: &mut SymbolTable, name: &str) -> SymbolId {
        use arandu_lexer::Span;
        st.define(
            st.global_scope(),
            name,
            crate::SymbolKind::Func,
            Span::new(0, 0, 0, 0, 0, 0),
        )
        .unwrap()
    }

    #[test]
    fn test_graph_deduplication() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "identity");
        let tid = interner.intern(ArType::Primitive(Primitive::Int));

        let mut graph = InstantiationGraph::new();
        let key = InstantiationKey {
            symbol: sym,
            type_args: vec![tid],
        };

        let id1 = graph.get_or_insert(key.clone(), &interner, &st).unwrap();
        let id2 = graph.get_or_insert(key, &interner, &st).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(graph.len(), 1);
    }

    #[test]
    fn test_different_type_args_different_nodes() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "identity");
        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        let str_id = interner.intern(ArType::Primitive(Primitive::Str));

        let mut graph = InstantiationGraph::new();
        let id1 = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym,
                    type_args: vec![int_id],
                },
                &interner,
                &st,
            )
            .unwrap();
        let id2 = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym,
                    type_args: vec![str_id],
                },
                &interner,
                &st,
            )
            .unwrap();
        assert_ne!(id1, id2);
        assert_eq!(graph.len(), 2);
    }

    #[test]
    fn test_recursion_limit() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "recursive");

        let mut graph = InstantiationGraph::with_recursion_limit(3);
        for i in 0..3 {
            let tid = interner.intern(ArType::Array(i, Box::new(ArType::Primitive(Primitive::Int))));
            graph
                .get_or_insert(
                    InstantiationKey {
                        symbol: sym,
                        type_args: vec![tid],
                    },
                    &interner,
                    &st,
                )
                .unwrap();
        }

        // 4th unique instantiation of the same symbol should fail
        let tid = interner.intern(ArType::Array(99, Box::new(ArType::Primitive(Primitive::Int))));
        let result = graph.get_or_insert(
            InstantiationKey {
                symbol: sym,
                type_args: vec![tid],
            },
            &interner,
            &st,
        );
        assert_eq!(
            result,
            Err(MonoError::RecursionLimitExceeded {
                symbol: sym,
                limit: 3,
            })
        );
    }

    #[test]
    fn test_cycle_detection() {
        let (mut st, mut interner) = setup();
        let sym_a = define_symbol(&mut st, "funcA");
        let sym_b = define_symbol(&mut st, "funcB");
        let tid = interner.intern(ArType::Primitive(Primitive::Int));

        let mut graph = InstantiationGraph::new();
        let id_a = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym_a,
                    type_args: vec![tid],
                },
                &interner,
                &st,
            )
            .unwrap();
        let id_b = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym_b,
                    type_args: vec![tid],
                },
                &interner,
                &st,
            )
            .unwrap();

        graph.add_edge(id_a, id_b);
        graph.add_edge(id_b, id_a); // cycle!

        let cycle = graph.find_cycle();
        assert!(cycle.is_some(), "expected cycle to be detected");
    }

    #[test]
    fn test_no_cycle_in_dag() {
        let (mut st, mut interner) = setup();
        let sym_a = define_symbol(&mut st, "funcA");
        let sym_b = define_symbol(&mut st, "funcB");
        let sym_c = define_symbol(&mut st, "funcC");
        let tid = interner.intern(ArType::Primitive(Primitive::Int));

        let mut graph = InstantiationGraph::new();
        let id_a = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym_a,
                    type_args: vec![tid],
                },
                &interner,
                &st,
            )
            .unwrap();
        let id_b = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym_b,
                    type_args: vec![tid],
                },
                &interner,
                &st,
            )
            .unwrap();
        let id_c = graph
            .get_or_insert(
                InstantiationKey {
                    symbol: sym_c,
                    type_args: vec![tid],
                },
                &interner,
                &st,
            )
            .unwrap();

        // A -> B, A -> C, B -> C (DAG, no cycle)
        graph.add_edge(id_a, id_b);
        graph.add_edge(id_a, id_c);
        graph.add_edge(id_b, id_c);

        assert!(graph.find_cycle().is_none());
    }

    #[test]
    fn test_mangling_simple() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "identity");
        let tid = interner.intern(ArType::Primitive(Primitive::Int));

        let key = InstantiationKey {
            symbol: sym,
            type_args: vec![tid],
        };

        let mangled = mangle_symbol(&key, &interner, &st);
        assert!(mangled.starts_with("_A$identity$I"));
        assert!(mangled.ends_with("$E"));
        assert!(mangled.contains("int"));
    }

    #[test]
    fn test_mangling_multi_arg() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "swap");
        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        let str_id = interner.intern(ArType::Primitive(Primitive::Str));

        let key = InstantiationKey {
            symbol: sym,
            type_args: vec![int_id, str_id],
        };

        let mangled = mangle_symbol(&key, &interner, &st);
        assert!(mangled.contains("int"));
        assert!(mangled.contains("str"));
    }

    #[test]
    fn test_demangle_roundtrip() {
        let demangled = demangle_symbol("_A$identity$I__int_$E");
        assert!(demangled.is_some());
        let s = demangled.unwrap();
        assert!(s.contains("identity"), "got: {s}");
    }

    #[test]
    fn test_mangled_names_are_unique() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "identity");
        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        let bool_id = interner.intern(ArType::Primitive(Primitive::Bool));

        let key_int = InstantiationKey {
            symbol: sym,
            type_args: vec![int_id],
        };
        let key_bool = InstantiationKey {
            symbol: sym,
            type_args: vec![bool_id],
        };

        let mangled_int = mangle_symbol(&key_int, &interner, &st);
        let mangled_bool = mangle_symbol(&key_bool, &interner, &st);
        assert_ne!(mangled_int, mangled_bool);
    }
}
