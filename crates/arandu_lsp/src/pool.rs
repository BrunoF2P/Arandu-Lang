//! Fixed-size worker pool for IDE jobs (P4 honesty: no unbounded thread::spawn).

use crossbeam_channel::{unbounded, Sender};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
pub struct WorkerPool {
    tx: Sender<Job>,
}

impl WorkerPool {
    pub fn new(workers: usize) -> std::io::Result<Self> {
        let n = workers.clamp(1, 16);
        let (tx, rx) = unbounded::<Job>();
        for i in 0..n {
            let rx = rx.clone();
            thread::Builder::new()
                .name(format!("arandu-lsp-worker-{i}"))
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        // A worker is an isolation boundary. Never reuse values captured by a
                        // panicking job (notably AnalysisSnapshot), but keep the pool available
                        // for later independent requests.
                        let _ = catch_unwind(AssertUnwindSafe(job));
                    }
                })?;
        }
        Ok(Self { tx })
    }

    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let _ = self.tx.send(Box::new(f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn panic_in_one_job_does_not_kill_the_worker() {
        let pool = WorkerPool::new(1).expect("test worker must start");
        let (tx, rx) = std::sync::mpsc::channel();
        pool.spawn(|| panic!("synthetic worker failure"));
        pool.spawn(move || tx.send(42).expect("test receiver must remain alive"));

        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(42));
    }
}
