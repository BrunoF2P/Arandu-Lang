#![allow(clippy::unwrap_used, clippy::expect_used)]
use arandu_query::db::DatabaseImpl;
use std::time::Instant;

#[test]
fn test_salsa_phases_performance() {
    let mut db = DatabaseImpl::default();

    // Generate a synthetic module graph with 50 modules
    let mut files = Vec::new();
    let num_modules = 50;

    for i in 0..num_modules {
        let path = format!("mod_{}.aru", i);
        let mut text = String::new();
        // Each module imports the previous one, forming a long chain.
        if i > 0 {
            text.push_str(&format!("import mod_{}\n", i - 1));
        }
        // Each module exports some functions.
        for j in 0..10 {
            text.push_str(&format!(
                "pub fn func_{}() -> i32 {{ return {}; }}\n",
                j,
                i * 10 + j
            ));
            if i > 0 {
                // Call previous module's function
                text.push_str(&format!(
                    "pub fn call_prev_{}() -> i32 {{ return mod_{}.func_{}(); }}\n",
                    j,
                    i - 1,
                    j
                ));
            }
        }
        files.push(db.new_file(path, text));
    }

    let last_file = *files.last().unwrap();

    // 1. Initial Compilation (Cold Cache)
    let start_cold = Instant::now();
    let _ = arandu_query::passes::type_check(&db, last_file);
    let cold_duration = start_cold.elapsed();
    println!(
        "Cold compilation of {} modules took: {:?}",
        num_modules, cold_duration
    );

    // Assert cold compilation is reasonably fast (under 1 second for 50 modules is extremely conservative)
    assert!(
        cold_duration.as_micros() < 1_000_000,
        "Cold compilation is too slow!"
    );

    // 2. Cached Compilation (Hot Cache - no changes)
    let start_hot = Instant::now();
    let _ = arandu_query::passes::type_check(&db, last_file);
    let hot_duration = start_hot.elapsed();
    println!("Hot compilation (no changes) took: {:?}", hot_duration);

    // Assert hot compilation is effectively instantaneous (under 5ms)
    assert!(
        hot_duration.as_micros() < 5000,
        "Hot compilation is too slow! ({}us)",
        hot_duration.as_micros()
    );

    // 3. Incremental Compilation (Change to the first module's body)
    // We change the body of `mod_0`, which does NOT change its public signature!
    // Thanks to Salsa's early cutoff, the rest of the chain should NOT be re-typechecked.

    let path0 = "mod_0.aru".to_string();
    let mut new_text0 = String::new();
    for j in 0..10 {
        new_text0.push_str(&format!(
            "pub fn func_{}() -> i32 {{ return {}; }}\n",
            j, 9999
        )); // Changed body!
    }
    db.new_file(path0, new_text0); // This overwrites the file and notifies Salsa

    let start_inc = Instant::now();
    let _ = arandu_query::passes::type_check(&db, last_file);
    let inc_duration = start_inc.elapsed();
    println!(
        "Incremental compilation (body change in mod_0) took: {:?}",
        inc_duration
    );

    // Assert incremental compilation is much faster than cold compilation due to early cutoff
    assert!(
        inc_duration.as_micros() < cold_duration.as_micros() / 2,
        "Incremental compilation is not benefiting from early cutoff!"
    );
}
