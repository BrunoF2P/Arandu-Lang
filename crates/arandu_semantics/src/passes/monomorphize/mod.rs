mod collect;
mod demangle;
mod graph;

pub use collect::analyze_instantiations;
pub use demangle::{demangle_symbol, mangle_symbol};
pub use graph::{InstantiationGraph, InstantiationKey, InstantiationNode, InstantiationNodeId, MonoError};

#[cfg(test)]
mod tests {
    use super::*;
    use arandu_lexer::Span;
    use arandu_middle::symbol_table::{SymbolId, SymbolKind, SymbolTable};
    use arandu_middle::types::{ArType, Primitive, TypeInterner};

    fn setup() -> (SymbolTable, TypeInterner) {
        (SymbolTable::new(), TypeInterner::new())
    }

    fn define_symbol(st: &mut SymbolTable, name: &str) -> SymbolId {
        st.define(st.global_scope(), name, SymbolKind::Func, Span::new(0, 0, 0))
            .unwrap()
    }

    #[test]
    fn test_graph_deduplication() {
        let (mut st, mut interner) = setup();
        let sym = define_symbol(&mut st, "identity");
        let int_id = interner.intern(ArType::Primitive(Primitive::Int));

        let mut graph = InstantiationGraph::new();
        let key = InstantiationKey {
            symbol: sym,
            type_args: vec![int_id],
        };

        let id1 = graph.get_or_insert(key.clone(), &interner, &st).unwrap();
        let id2 = graph.get_or_insert(key, &interner, &st).unwrap();
        assert_eq!(id1, id2);
        assert!(graph.len() >= 1);
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
        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        for i in 0..3 {
            let tid = interner.intern(ArType::Array(i, int_id));
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

        let int_id = interner.intern(ArType::Primitive(Primitive::Int));
        let tid = interner.intern(ArType::Array(99, int_id));
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
        graph.add_edge(id_b, id_a);

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

    #[test]
    fn test_analyze_instantiations_collects_hir_generic_call() {
        let src = r#"
func identity<T>(value: T): T {
    return value
}

func main() {
    let x: int = identity<int>(42)
}
"#;
        let program = arandu_parser::parse(src).expect("parse failed");
        let resolution = crate::passes::name_resolution::resolve(&program);
        let mut tc = crate::passes::type_checker::type_check(resolution, &program);
        assert!(
            tc.diagnostics.is_empty(),
            "type check failed: {:?}",
            tc.diagnostics
        );
        let hir =
            crate::passes::lower_hir::lower_to_hir(&mut tc, &program).expect("HIR lowering failed");

        let graph = analyze_instantiations(&tc, &hir).expect("analysis failed");

        assert!(graph.len() >= 1);
        assert!(
            graph
                .iter()
                .any(|node| node.mangled_name.contains("identity"))
        );
    }
}
