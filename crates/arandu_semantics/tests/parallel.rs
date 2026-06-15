use std::fs;
use std::path::PathBuf;
use arandu_semantics::{compile_parallel, resolve, type_check, lower_to_hir, lower_to_amir, optimize_amir};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate should be under workspace/crates")
        .to_path_buf()
}

#[test]
fn test_parallel_compilation_multi_file_project() {
    let root = workspace_root();
    let temp_dir = root.join("target").join("temp_test_parallel");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let file_a_path = temp_dir.join("file_a.aru");
    let file_b_path = temp_dir.join("file_b.aru");

    // file_a: a module with a helper function
    fs::write(
        &file_a_path,
        r#"
        module myModule

        func helper() int {
            return 100
        }
        "#,
    ).unwrap();

    // file_b: standalone module calling its own function
    fs::write(
        &file_b_path,
        r#"
        func run() int {
            x int = 42
            return x
        }
        "#,
    ).unwrap();

    let paths = vec![file_a_path, file_b_path];
    let parallel_result = compile_parallel(paths);
    assert!(
        parallel_result.is_ok(),
        "Parallel compilation of multi-file project failed: {:?}",
        parallel_result.err()
    );

    let output = parallel_result.unwrap();
    assert_eq!(output.paths.len(), 2);
    assert_eq!(output.hirs.len(), 2);
    assert_eq!(output.amirs.len(), 2);
    assert_eq!(output.symbols.len(), 2);

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_parallel_compilation_individual_files_parity() {
    let root = workspace_root();
    let ok_dir = root.join("tests").join("ui").join("type_checker").join("ok");
    assert!(ok_dir.exists(), "ok directory does not exist");

    let mut paths = Vec::new();
    for entry in fs::read_dir(ok_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("aru") {
            paths.push(path);
        }
    }
    paths.sort();

    // Compile each file individually using compile_parallel (which runs it via the scheduler)
    // and assert parity with sequential mode
    for path in paths {
        let source = fs::read_to_string(&path).unwrap();
        let program = arandu_parser::parse(&source).expect("parse");
        let resolution = resolve(&program);
        let tc = type_check(resolution, &program);
        
        let hir_seq = lower_to_hir(&tc, &program).expect("lower_to_hir seq");
        let mut amir_seq = lower_to_amir(&tc, &hir_seq).expect("lower_to_amir seq");
        optimize_amir(&mut amir_seq);

        // Run parallel compilation for this single file
        let parallel_result = compile_parallel(vec![path.clone()]);
        assert!(
            parallel_result.is_ok(),
            "Parallel compilation failed for file: {}: {:?}",
            path.display(),
            parallel_result.err()
        );

        let parallel_output = parallel_result.unwrap();
        assert_eq!(parallel_output.hirs.len(), 1);
        assert_eq!(parallel_output.amirs.len(), 1);

        let hir_par = &parallel_output.hirs[0];
        let amir_par = &parallel_output.amirs[0];

        // Compare structure/string representation
        let ctx_seq = arandu_semantics::hir::HirPrettyCtx {
            pool: &hir_seq.pool,
            symbols: &tc.symbols,
            show_spans: false,
        };
        let ctx_par = arandu_semantics::hir::HirPrettyCtx {
            pool: &hir_par.pool,
            symbols: &parallel_output.symbols[0],
            show_spans: false,
        };

        let hir_seq_str = hir_seq.pretty_print(&ctx_seq);
        let hir_par_str = hir_par.pretty_print(&ctx_par);
        assert_eq!(
            hir_seq_str, hir_par_str,
            "HIR mismatch for file: {}",
            path.display()
        );

        let amir_seq_str = amir_seq.pretty_print(&tc.symbols);
        let amir_par_str = amir_par.pretty_print(&parallel_output.symbols[0]);
        assert_eq!(
            amir_seq_str, amir_par_str,
            "AMIR mismatch for file: {}",
            path.display()
        );
    }
}
