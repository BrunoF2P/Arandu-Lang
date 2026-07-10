#![allow(clippy::unwrap_used, clippy::expect_used)]
use arandu_query::DatabaseImpl;

#[test]
fn test_simple_import_graph() {
    let mut db = DatabaseImpl::default();
    let mod_b = db.new_file("mod_b.aru".to_string(), "func add() {}".to_string());
    let mod_a = db.new_file("mod_a.aru".to_string(), "import mod_b".to_string());

    let file_id_a = mod_a.file_id(&db);
    let file_id_b = mod_b.file_id(&db);

    let graph_hash = arandu_query::passes::module_dependency_graph(&db, mod_a);
    let graph = &*graph_hash;

    // Verify nodes
    let node_weights: Vec<u32> = graph.node_weights().copied().collect();
    assert!(node_weights.contains(&file_id_a));
    assert!(node_weights.contains(&file_id_b));
    assert_eq!(node_weights.len(), 2);

    // Find node indices
    let idx_a = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_a)
        .unwrap();
    let idx_b = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_b)
        .unwrap();

    // Verify edges
    assert!(graph.contains_edge(idx_a, idx_b));
    assert_eq!(graph.edge_count(), 1);
}

#[test]
fn test_transitive_import_graph() {
    let mut db = DatabaseImpl::default();
    let mod_c = db.new_file("mod_c.aru".to_string(), "".to_string());
    let mod_b = db.new_file("mod_b.aru".to_string(), "import mod_c".to_string());
    let mod_a = db.new_file("mod_a.aru".to_string(), "import mod_b".to_string());

    let file_id_a = mod_a.file_id(&db);
    let file_id_b = mod_b.file_id(&db);
    let file_id_c = mod_c.file_id(&db);

    let graph_hash = arandu_query::passes::module_dependency_graph(&db, mod_a);
    let graph = &*graph_hash;

    // Verify nodes
    let node_weights: Vec<u32> = graph.node_weights().copied().collect();
    assert!(node_weights.contains(&file_id_a));
    assert!(node_weights.contains(&file_id_b));
    assert!(node_weights.contains(&file_id_c));
    assert_eq!(node_weights.len(), 3);

    // Find node indices
    let idx_a = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_a)
        .unwrap();
    let idx_b = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_b)
        .unwrap();
    let idx_c = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_c)
        .unwrap();

    // Verify edges
    assert!(graph.contains_edge(idx_a, idx_b));
    assert!(graph.contains_edge(idx_b, idx_c));
    assert_eq!(graph.edge_count(), 2);
}

#[test]
fn test_circular_import_graph_does_not_overflow() {
    let mut db = DatabaseImpl::default();
    // A imports B, B imports A
    let mod_a = db.new_file("mod_a.aru".to_string(), "import mod_b".to_string());
    let mod_b = db.new_file("mod_b.aru".to_string(), "import mod_a".to_string());

    let file_id_a = mod_a.file_id(&db);
    let file_id_b = mod_b.file_id(&db);

    let graph_hash = arandu_query::passes::module_dependency_graph(&db, mod_a);
    let graph = &*graph_hash;

    // Verify nodes
    let node_weights: Vec<u32> = graph.node_weights().copied().collect();
    assert!(node_weights.contains(&file_id_a));
    assert!(node_weights.contains(&file_id_b));
    assert_eq!(node_weights.len(), 2);

    // Find node indices
    let idx_a = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_a)
        .unwrap();
    let idx_b = graph
        .node_indices()
        .find(|&i| graph[i] == file_id_b)
        .unwrap();

    // Verify edges (cyclic paths)
    assert!(graph.contains_edge(idx_a, idx_b));
    assert!(graph.contains_edge(idx_b, idx_a));
    assert_eq!(graph.edge_count(), 2);
}
