//! DX.5 — causal-chain explain-rebuild from Salsa runtime events.
//!
//! Uses Salsa's own [`salsa::Event`] stream (no hand-rolled dependency graph).
//! Recording is opt-in via [`crate::db::DatabaseImpl::with_rebuild_log`] so the
//! hot path pays zero overhead when disabled.

use salsa::{Event, EventKind};
use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex};

/// Compact rebuild event (stringified key; allocated only while recording).
#[derive(Debug, Clone)]
pub enum RebuildEvent {
    /// Query body will run (cold or inputs dirty).
    Execute { key: String },
    /// Memo hit — inputs still valid.
    Validate { key: String },
}

impl fmt::Display for RebuildEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RebuildEvent::Execute { key } => write!(f, "execute {key}"),
            RebuildEvent::Validate { key } => write!(f, "validate {key}"),
        }
    }
}

/// Thread-safe ring of rebuild events shared with the Salsa event callback.
///
/// Prefer a single shared [`Arc`] across the database lifetime; clear between
/// measurement windows instead of reallocating the log.
#[derive(Default)]
pub struct RebuildLog {
    events: Mutex<Vec<RebuildEvent>>,
}

impl RebuildLog {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.events.lock() {
            g.clear();
        }
    }

    pub fn push(&self, event: RebuildEvent) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event);
        }
    }

    /// Snapshot for formatting / tests (clones only the recorded window).
    #[must_use]
    pub fn snapshot(&self) -> Vec<RebuildEvent> {
        self.events.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Human-readable causal chain of **executions** (validate hits omitted unless verbose).
    #[must_use]
    pub fn format_chain(&self, verbose: bool) -> String {
        let events = self.snapshot();
        let mut out = String::new();
        let mut n_exec = 0u32;
        let mut n_val = 0u32;
        for ev in &events {
            match ev {
                RebuildEvent::Execute { .. } => {
                    n_exec += 1;
                    let _ = writeln!(out, "  → {ev}");
                }
                RebuildEvent::Validate { .. } => {
                    n_val += 1;
                    if verbose {
                        let _ = writeln!(out, "  · {ev}");
                    }
                }
            }
        }
        let mut header = String::new();
        let _ = writeln!(header, "rebuild chain: {n_exec} execute, {n_val} validate");
        header.push_str(&out);
        header
    }

    /// Salsa event callback: maps runtime events into [`RebuildEvent`].
    pub fn salsa_callback(this: Arc<Self>) -> Box<dyn Fn(Event) + Send + Sync + 'static> {
        Box::new(move |event| match event.kind {
            EventKind::WillExecute { database_key } => {
                this.push(RebuildEvent::Execute {
                    key: format!("{database_key:?}"),
                });
            }
            EventKind::DidValidateMemoizedValue { database_key } => {
                this.push(RebuildEvent::Validate {
                    key: format!("{database_key:?}"),
                });
            }
            _ => {}
        })
    }
}

/// True if any `WillExecute` was recorded (something recomputed).
#[must_use]
pub fn any_execute(log: &RebuildLog) -> bool {
    log.snapshot()
        .iter()
        .any(|e| matches!(e, RebuildEvent::Execute { .. }))
}
