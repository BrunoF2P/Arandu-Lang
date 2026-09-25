//! Deterministic, parallel-capable entry point for AranduSmith fuzz targets.

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::panic::{self, AssertUnwindSafe};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use arandu_fuzz_support::Target;

const MAX_ITERATIONS: u64 = 1_000_000;
const DEFAULT_BATCH_SIZE: u64 = 50;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Config {
    iterations: u64,
    duration: Option<Duration>,
    first_seed: u64,
    jobs: usize,
    batch_size: u64,
    target: Target,
    report_path: Option<String>,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    if let Some(ms_str) = s.strip_suffix("ms") {
        let ms: u64 = ms_str
            .parse()
            .map_err(|_| format!("invalid milliseconds in duration: {s}"))?;
        Ok(Duration::from_millis(ms))
    } else if let Some(secs_str) = s.strip_suffix('s') {
        let secs: u64 = secs_str
            .parse()
            .map_err(|_| format!("invalid seconds in duration: {s}"))?;
        Ok(Duration::from_secs(secs))
    } else if let Some(mins_str) = s.strip_suffix('m') {
        let mins: u64 = mins_str
            .parse()
            .map_err(|_| format!("invalid minutes in duration: {s}"))?;
        Ok(Duration::from_secs(mins * 60))
    } else if let Some(hours_str) = s.strip_suffix('h') {
        let hours: u64 = hours_str
            .parse()
            .map_err(|_| format!("invalid hours in duration: {s}"))?;
        Ok(Duration::from_secs(hours * 3600))
    } else {
        let secs: u64 = s
            .parse()
            .map_err(|_| format!("invalid duration '{s}': specify unit like 30s, 5m, 100ms"))?;
        Ok(Duration::from_secs(secs))
    }
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut config = Config {
        iterations: 1,
        duration: None,
        first_seed: 0,
        jobs: 1,
        batch_size: DEFAULT_BATCH_SIZE,
        target: Target::SynthesizedAll,
        report_path: None,
    };
    let mut explicit_iterations = false;

    while let Some(option) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{option} requires a value"))?;
        match option.as_str() {
            "--iterations" => {
                config.iterations = value
                    .parse()
                    .map_err(|_| "--iterations must be a positive integer".to_owned())?;
                explicit_iterations = true;
            }
            "--duration" => {
                config.duration = Some(parse_duration(&value)?);
            }
            "--report" => {
                config.report_path = Some(value);
            }
            "--seed" => {
                config.first_seed = value
                    .parse()
                    .map_err(|_| "--seed must be an unsigned 64-bit integer".to_owned())?;
            }
            "--target" => {
                config.target =
                    Target::parse(&value).ok_or_else(|| format!("unknown fuzz target: {value}"))?;
            }
            "--jobs" | "-j" => {
                config.jobs = value
                    .parse()
                    .map_err(|_| "--jobs must be a positive integer".to_owned())?;
                if config.jobs == 0 || config.jobs > 256 {
                    return Err("--jobs must be between 1 and 256".to_owned());
                }
            }
            "--batch-size" => {
                config.batch_size = value
                    .parse()
                    .map_err(|_| "--batch-size must be a positive integer".to_owned())?;
                if config.batch_size == 0 || config.batch_size > 10_000 {
                    return Err("--batch-size must be between 1 and 10000".to_owned());
                }
            }
            _ => return Err(format!("unknown option: {option}")),
        }
    }

    if config.duration.is_some() && !explicit_iterations {
        config.iterations = MAX_ITERATIONS;
    }

    if !(1..=MAX_ITERATIONS).contains(&config.iterations) {
        return Err(format!(
            "--iterations must be between 1 and {MAX_ITERATIONS}"
        ));
    }
    config
        .first_seed
        .checked_add(config.iterations - 1)
        .ok_or_else(|| "seed range overflows u64".to_owned())?;
    Ok(config)
}

struct ProgressReporter {
    total_seeds: Option<u64>,
    target_name: &'static str,
    start_time: Instant,
    last_update: Instant,
    is_tty: bool,
}

impl ProgressReporter {
    fn new(total_seeds: Option<u64>, target_name: &'static str) -> Self {
        let is_tty = std::io::stderr().is_terminal();
        Self {
            total_seeds,
            target_name,
            start_time: Instant::now(),
            last_update: Instant::now(),
            is_tty,
        }
    }

    fn update(&mut self, completed: u64, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_update) < Duration::from_millis(200) {
            return;
        }
        self.last_update = now;
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.001 {
            completed as f64 / elapsed
        } else {
            0.0
        };

        if self.is_tty {
            if let Some(total) = self.total_seeds {
                let pct = (completed as f64 / total as f64) * 100.0;
                let eta = if speed > 0.0 && completed < total {
                    let rem_secs = (total - completed) as f64 / speed;
                    format!("{rem_secs:.1}s")
                } else {
                    "0.0s".to_string()
                };
                let bar_width = 20;
                let filled = ((pct / 100.0) * bar_width as f64).min(bar_width as f64) as usize;
                let bar: String = (0..bar_width)
                    .map(|i| {
                        if i < filled {
                            '='
                        } else if i == filled {
                            '>'
                        } else {
                            ' '
                        }
                    })
                    .collect();
                eprint!(
                    "\r\x1b[2Ksmith: [{bar}] {completed}/{total} ({pct:.1}%) | {speed:.1} seeds/s | elapsed: {elapsed:.1}s | ETA: {eta}"
                );
            } else {
                eprint!(
                    "\r\x1b[2Ksmith: {completed} seeds | {speed:.1} seeds/s | elapsed: {elapsed:.1}s (target={})",
                    self.target_name
                );
            }
            let _ = std::io::stderr().flush();
        } else if force || completed.is_multiple_of(100) {
            if let Some(total) = self.total_seeds {
                eprintln!(
                    "smith: {completed}/{total} seeds passed ({speed:.1} seeds/s, target={})",
                    self.target_name
                );
            } else {
                eprintln!(
                    "smith: {completed} seeds passed ({speed:.1} seeds/s, elapsed: {elapsed:.1}s, target={})",
                    self.target_name
                );
            }
        }
    }

    fn finish(&self, completed: u64, error: Option<&str>) {
        if self.is_tty {
            eprintln!();
        }
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.001 {
            completed as f64 / elapsed
        } else {
            0.0
        };

        eprintln!("\n======================== AranduSmith Fuzz Summary ========================");
        eprintln!("Target:           {}", self.target_name);
        eprintln!("Seeds evaluated:  {completed}");
        eprintln!("Time elapsed:     {elapsed:.2}s");
        eprintln!("Throughput:       {speed:.2} seeds/s");
        if let Some(err) = error {
            eprintln!("Status:           FAILED\n{err}");
        } else {
            eprintln!("Status:           PASSED (all backends in parity, 0 panics)");
        }
        eprintln!("==========================================================================\n");
    }
}

pub fn run(args: impl Iterator<Item = String>) -> i32 {
    let config = match parse_args(args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("smith: {error}\nusage: cargo run --locked -p xtask -- smith [--iterations N] [--duration TIME] [--seed N] [--target TARGET] [--jobs N] [--batch-size N] [--report PATH]");
            return 2;
        }
    };

    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            eprintln!("smith: cannot locate the xtask executable: {error}");
            return 1;
        }
    };

    let mut batches = Vec::new();
    let mut remaining = config.iterations;
    let mut current_seed = config.first_seed;
    while remaining > 0 {
        let count = remaining.min(config.batch_size);
        batches.push((current_seed, count));
        current_seed += count;
        remaining -= count;
    }

    let deadline = config.duration.map(|d| Instant::now() + d);
    let total_seeds = if config.duration.is_some() {
        None
    } else {
        Some(config.iterations)
    };
    let mut reporter = ProgressReporter::new(total_seeds, config.target.name());

    if config.jobs <= 1 || batches.len() <= 1 {
        let mut completed = 0u64;
        let mut failed = None;
        for (start_seed, count) in batches {
            if let Some(dl) = deadline {
                if Instant::now() >= dl {
                    break;
                }
            }
            if let Err(error) = run_batch_isolated(&executable, config.target, start_seed, count) {
                let detail = isolate_failure_detail(&executable, config.target, start_seed, count);
                failed = Some(format!("{error}\n{detail}"));
                break;
            }
            completed += count;
            reporter.update(completed, false);
        }
        reporter.update(completed, true);
        reporter.finish(completed, failed.as_deref());
        save_report(&config, completed, failed.as_deref(), reporter.start_time);
        return if failed.is_some() { 1 } else { 0 };
    }

    let batch_queue = Arc::new(Mutex::new(batches.into_iter().collect::<VecDeque<_>>()));
    let stop_signal = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    let actual_workers = config.jobs.min(batch_queue.lock().unwrap().len());
    let mut handles = Vec::with_capacity(actual_workers);

    for _ in 0..actual_workers {
        let queue = Arc::clone(&batch_queue);
        let stop = Arc::clone(&stop_signal);
        let thread_tx = tx.clone();
        let exe = executable.clone();
        let target = config.target;

        handles.push(thread::spawn(move || loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            if let Some(dl) = deadline {
                if Instant::now() >= dl {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            }
            let next_batch = {
                let mut lock = queue.lock().unwrap();
                lock.pop_front()
            };
            let Some((start_seed, count)) = next_batch else {
                break;
            };

            let result = run_batch_isolated(&exe, target, start_seed, count);
            if result.is_err() {
                stop.store(true, Ordering::Relaxed);
            }
            let _ = thread_tx.send((start_seed, count, result));
        }));
    }
    drop(tx);

    let mut completed = 0u64;
    let mut failed = None;

    for (start_seed, count, result) in rx {
        match result {
            Ok(()) => {
                completed += count;
                reporter.update(completed, false);
            }
            Err(error) => {
                if failed.is_none() {
                    let detail =
                        isolate_failure_detail(&executable, config.target, start_seed, count);
                    failed = Some(format!("{error}\n{detail}"));
                }
            }
        }
    }

    for handle in handles {
        let _ = handle.join();
    }

    reporter.update(completed, true);
    reporter.finish(completed, failed.as_deref());
    save_report(&config, completed, failed.as_deref(), reporter.start_time);

    if failed.is_some() {
        1
    } else {
        0
    }
}

fn save_report(config: &Config, completed: u64, error: Option<&str>, start_time: Instant) {
    if let Some(ref report_path) = config.report_path {
        let elapsed = start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.001 {
            completed as f64 / elapsed
        } else {
            0.0
        };
        let report = serde_json::json!({
            "target": config.target.name(),
            "seeds_evaluated": completed,
            "elapsed_seconds": elapsed,
            "seeds_per_second": speed,
            "workers": config.jobs,
            "success": error.is_none(),
            "error": error,
        });
        if let Err(e) = std::fs::write(report_path, report.to_string()) {
            eprintln!("smith: failed to write report to {report_path}: {e}");
        }
    }
}

/// Execute a batch of seeds in a worker so a stuck JIT cannot pin the campaign process.
fn run_batch_isolated(
    executable: &std::path::Path,
    target: Target,
    start_seed: u64,
    count: u64,
) -> Result<(), String> {
    let per_seed_timeout = crate::fuzz_regressions::seed_timeout(target);
    let batch_multiplier = u32::try_from(count).unwrap_or(u32::MAX);
    let timeout = per_seed_timeout
        .saturating_mul(batch_multiplier)
        .saturating_add(Duration::from_secs(30));

    let mut command = Command::new(executable);
    command
        .arg("smith-worker")
        .arg(target.name())
        .arg(start_seed.to_string())
        .arg(count.to_string())
        .stdin(Stdio::null());
    let status = run_command_with_timeout(&mut command, timeout).map_err(|error| {
        format!(
            "target {} {error} for batch starting at seed {start_seed}",
            target.name()
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "target {} failed for batch starting at seed {start_seed} (count={count}, {status})",
            target.name()
        ))
    }
}

/// When a batch fails, re-run candidate seeds individually with captured output to pinpoint the exact reproducer.
fn isolate_failure_detail(
    executable: &std::path::Path,
    target: Target,
    start_seed: u64,
    count: u64,
) -> String {
    for offset in 0..count {
        let seed = start_seed + offset;
        let mut command = Command::new(executable);
        command
            .arg("smith-worker")
            .arg(target.name())
            .arg(seed.to_string())
            .arg("1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Ok(output) = command.output() {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let detail = if !stderr.is_empty() {
                    stderr.to_string()
                } else {
                    stdout.to_string()
                };
                return format!("--- Exact failing seed: {seed} (0x{seed:016x}) ---\n{detail}");
            }
        }
    }
    format!(
        "batch {start_seed}..{} failed but individual seeds did not reproduce cleanly",
        start_seed + count
    )
}

fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<ExitStatus, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start worker: {error}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                terminate_worker(&mut child);
                return Err(format!("timed out after {timeout:?}"));
            }
            Err(error) => {
                terminate_worker(&mut child);
                return Err(format!("cannot wait for worker: {error}"));
            }
        }
    }
}

fn terminate_worker(child: &mut Child) {
    #[cfg(unix)]
    arandu_fuzz_support::process_job::terminate_process_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

/// Internal worker entry point used by `run_batch_isolated`.
pub fn run_worker(mut args: impl Iterator<Item = String>) -> i32 {
    let Some(target) = args.next().and_then(|name| Target::parse(&name)) else {
        eprintln!("smith-worker: invalid target");
        return 2;
    };
    let Some(start_seed) = args.next().and_then(|seed| seed.parse::<u64>().ok()) else {
        eprintln!("smith-worker: invalid seed");
        return 2;
    };
    let count = match args.next() {
        Some(count_str) => match count_str.parse::<u64>() {
            Ok(c) if c > 0 => c,
            _ => {
                eprintln!("smith-worker: invalid count");
                return 2;
            }
        },
        None => 1,
    };
    if args.next().is_some() {
        eprintln!("smith-worker: unexpected extra arguments");
        return 2;
    }

    for offset in 0..count {
        let seed = start_seed + offset;
        let bytes = seed.to_le_bytes();
        match panic::catch_unwind(AssertUnwindSafe(|| {
            arandu_fuzz_support::run(target, &bytes);
        })) {
            Ok(()) => {}
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic in fuzz target");
                eprintln!(
                    "smith-worker: target {} panicked at seed {seed}: {message}",
                    target.name()
                );
                return 1;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> std::vec::IntoIter<String> {
        values
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn defaults_to_one_differential_seed() {
        assert_eq!(
            parse_args(args(&[])),
            Ok(Config {
                iterations: 1,
                duration: None,
                first_seed: 0,
                jobs: 1,
                batch_size: 50,
                target: Target::SynthesizedAll,
                report_path: None,
            })
        );
    }

    #[test]
    fn parses_seed_range_and_target() {
        assert_eq!(
            parse_args(args(&[
                "--iterations",
                "25",
                "--seed",
                "90",
                "--target",
                "lsp-session",
                "--jobs",
                "4",
                "--batch-size",
                "10",
                "--duration",
                "30s",
                "--report",
                "fuzz-report.json",
            ])),
            Ok(Config {
                iterations: 25,
                duration: Some(Duration::from_secs(30)),
                first_seed: 90,
                jobs: 4,
                batch_size: 10,
                target: Target::LspSession,
                report_path: Some("fuzz-report.json".to_owned()),
            })
        );
    }

    #[test]
    fn parses_various_duration_formats() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("10s"), Ok(Duration::from_secs(10)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("1h"), Ok(Duration::from_secs(3600)));
        assert!(parse_duration("invalid").is_err());
    }

    #[test]
    fn rejects_invalid_bounds_and_seed_overflow() {
        assert!(parse_args(args(&["--iterations", "0"])).is_err());
        assert!(parse_args(args(&["--iterations", "1000001"])).is_err());
        assert!(parse_args(args(&["--jobs", "0"])).is_err());
        assert!(parse_args(args(&["--batch-size", "0"])).is_err());
        assert!(parse_args(args(&[
            "--seed",
            "18446744073709551615",
            "--iterations",
            "2",
        ]))
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn seed_worker_timeout_kills_the_process_group() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 5");
        let error = run_command_with_timeout(&mut command, Duration::from_millis(30))
            .expect_err("a sleeping worker should exceed its deadline");
        assert!(error.contains("timed out after"), "{error}");
    }
}
