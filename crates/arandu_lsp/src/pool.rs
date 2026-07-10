//! Fixed-size worker pool for IDE jobs (P4 honesty: no unbounded thread::spawn).

use crossbeam_channel::{unbounded, Sender};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
pub struct WorkerPool {
    tx: Sender<Job>,
}

impl WorkerPool {
    #[must_use]
    pub fn new(workers: usize) -> Self {
        let n = workers.clamp(1, 16);
        let (tx, rx) = unbounded::<Job>();
        for i in 0..n {
            let rx = rx.clone();
            thread::Builder::new()
                .name(format!("arandu-lsp-worker-{i}"))
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        job();
                    }
                })
                .expect("spawn lsp worker");
        }
        Self { tx }
    }

    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let _ = self.tx.send(Box::new(f));
    }
}
