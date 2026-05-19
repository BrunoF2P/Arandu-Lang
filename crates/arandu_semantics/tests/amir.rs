use arandu_semantics::{lower_to_amir, lower_to_hir, resolve, type_check};

#[test]
fn test_amir_golden_files() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = std::path::Path::new(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixtures_dir = root_dir.join("tests").join("amir");

    if !fixtures_dir.exists() {
        // No fixtures directory = nothing to test
        return;
    }

    let update_golden = std::env::var("UPDATE_GOLDEN").is_ok();

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&fixtures_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "aru") {
            entries.push(path);
        }
    }

    entries.sort();

    for path in entries {
        let name = path.file_stem().unwrap().to_str().unwrap();
        let src = std::fs::read_to_string(&path).unwrap();

        let program = arandu_parser::parse(&src).unwrap_or_else(|err| {
            panic!("failed to parse {}: {:?}", name, err);
        });
        let resolution = resolve(&program);
        let tc = type_check(resolution, &program);
        if !tc.diagnostics.is_empty() {
            panic!("type check failed for {}: {:?}", name, tc.diagnostics);
        }
        let hir = lower_to_hir(&tc, &program).expect("HIR lowering failed");
        hir.validate_invariants(&tc.symbols)
            .expect("HIR invariant validation failed");
        let amir = lower_to_amir(&tc, &hir).expect("AMIR lowering failed");
        let pretty = amir.pretty_print(&tc.symbols);

        let golden_path = fixtures_dir.join(format!("{}.amir", name));
        if update_golden {
            std::fs::write(&golden_path, &pretty).unwrap();
        } else {
            assert!(
                golden_path.exists(),
                "Golden file missing for {name}. Run with UPDATE_GOLDEN=1 to create it."
            );
            let expected = std::fs::read_to_string(&golden_path).unwrap();
            let expected_normalized = expected.replace("\r\n", "\n");
            let pretty_normalized = pretty.replace("\r\n", "\n");
            assert_eq!(
                pretty_normalized, expected_normalized,
                "AMIR mismatch for {}. Run with UPDATE_GOLDEN=1 to update.",
                name
            );
        }
    }
}
