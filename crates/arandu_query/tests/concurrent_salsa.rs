use arandu_query::analysis::AnalysisHost;
use std::thread;

#[test]
fn test_concurrent_salsa_queries() {
    let mut host = AnalysisHost::new();
    let f = host.new_file(
        "test_concurrent.aru".to_string(),
        "func main(): int { return 42 }".to_string(),
    );

    // Warm up query caches on main thread
    let _ = arandu_query::passes::type_check(host.db(), f);

    // Spawn concurrent readers using snapshots
    let mut readers = vec![];
    for _ in 0..4 {
        let snap = host.snapshot();
        readers.push(thread::spawn(move || {
            for _ in 0..50 {
                let _tc = arandu_query::passes::type_check(&snap.db, f);
            }
        }));
    }

    // Writer thread updates text, advancing database revision
    for i in 0..10 {
        let code = format!("func main(): int {{ return {} }}", i);
        host.set_text(f, code);
        thread::sleep(std::time::Duration::from_millis(2));
    }

    // Join readers and check errors without panicking the main thread
    for (i, r) in readers.into_iter().enumerate() {
        match r.join() {
            Ok(_) => println!("Thread {} succeeded", i),
            Err(e) => {
                let is_salsa_cancelled = e.is::<salsa::Cancelled>();
                println!(
                    "Thread {} failed: is_salsa_cancelled={}",
                    i, is_salsa_cancelled
                );
                // The test is considered successful if the thread either finished successfully
                // or was cancelled by Salsa when the writer mutated the inputs.
                assert!(
                    is_salsa_cancelled,
                    "Thread failed with non-cancellation panic!"
                );
            }
        }
    }
}
