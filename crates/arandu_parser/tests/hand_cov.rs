#![allow(clippy::unwrap_used, clippy::expect_used)]
use arandu_parser::syntax::{hand, parse_syntax};
use std::path::PathBuf;

#[test]
fn fixture_hand_coverage() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/parser");
    let mut incomplete = Vec::new();
    let mut total = 0usize;
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("aru") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("invalid") {
            continue;
        }
        total += 1;
        let src = std::fs::read_to_string(&path).unwrap();
        let tree = parse_syntax(&src);
        let (ft, fh) = hand::count_hand_lowerable_funcs(&tree);
        let (dt, dh) = hand::count_hand_lowerable_decls(&tree);
        if ft != fh || dt != dh {
            incomplete.push(format!(
                "{}: funcs {}/{} decls {}/{}",
                path.file_stem().unwrap().to_string_lossy(),
                fh,
                ft,
                dh,
                dt
            ));
        }
    }
    incomplete.sort();
    for r in &incomplete {
        println!("{r}");
    }
    println!("--- incomplete: {}/{} ---", incomplete.len(), total);
    assert!(
        incomplete.is_empty(),
        "all valid fixtures should fully hand-lower funcs/decls"
    );
}
