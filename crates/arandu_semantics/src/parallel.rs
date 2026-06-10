use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use arandu_parser::{Program, ParseError};
use crate::{Diagnostic, ResolutionResult, TypeCheckResult, SymbolTable, ResolvedNames, Severity};
use crate::hir::HirProgram;
use crate::amir::AmirProgram;


#[cfg(target_os = "linux")]
fn pin_thread_to_core(core_id: usize) {
    unsafe {
        let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(core_id, &mut cpuset);
        let thread = libc::pthread_self();
        let _ = libc::pthread_setaffinity_np(
            thread,
            std::mem::size_of::<libc::cpu_set_t>(),
            &cpuset,
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn pin_thread_to_core(_core_id: usize) {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    Parse { file_idx: usize },
    MergeSymbols,
    Resolve { file_idx: usize },
    TypeCheck { file_idx: usize },
    LowerHir { file_idx: usize },
    LowerAmir { file_idx: usize },
}

struct TaskQueue {
    tasks: Mutex<VecDeque<Task>>,
}

struct SchedulerState {
    merge_deps: AtomicUsize,
    resolve_deps: Vec<AtomicUsize>,
    typecheck_deps: Vec<AtomicUsize>,
    hir_deps: Vec<AtomicUsize>,
    amir_deps: Vec<AtomicUsize>,
}

pub struct CompilationContext {
    pub paths: Vec<PathBuf>,
    pub num_files: usize,
    
    // Outputs & Intermediate Results
    pub programs: Vec<Mutex<Option<Program>>>,
    pub parse_errors: Vec<Mutex<Vec<ParseError>>>,
    pub symbol_tables: Vec<Mutex<Option<SymbolTable>>>,
    pub resolveds: Vec<Mutex<Option<ResolvedNames>>>,
    pub doc_maps: Vec<Mutex<Option<crate::DocCommentMap>>>,
    pub resolve_diags: Vec<Mutex<Vec<Diagnostic>>>,
    pub symbol_offsets: Vec<Mutex<usize>>,
    
    pub merged_symbol_table: Mutex<Option<SymbolTable>>,
    pub merged_docs: Mutex<Option<crate::DocCommentMap>>,
    pub merged_diags: Mutex<Vec<Diagnostic>>,
    
    pub resolutions: Vec<Mutex<Option<ResolutionResult>>>,
    pub type_checks: Vec<Mutex<Option<TypeCheckResult>>>,
    pub hirs: Vec<Mutex<Option<HirProgram>>>,
    pub amirs: Vec<Mutex<Option<AmirProgram>>>,
    
    pub diagnostics: Mutex<Vec<Diagnostic>>,
}

#[derive(Debug)]
pub struct ParallelOutput {
    pub paths: Vec<PathBuf>,
    pub hirs: Vec<HirProgram>,
    pub amirs: Vec<AmirProgram>,
    pub symbols: Vec<SymbolTable>,
}

pub fn compile_parallel(paths: Vec<PathBuf>) -> Result<ParallelOutput, Vec<Diagnostic>> {
    let num_files = paths.len();
    if num_files == 0 {
        return Ok(ParallelOutput {
            paths: Vec::new(),
            hirs: Vec::new(),
            amirs: Vec::new(),
            symbols: Vec::new(),
        });
    }


    let mut programs = Vec::with_capacity(num_files);
    let mut parse_errors = Vec::with_capacity(num_files);
    let mut symbol_tables = Vec::with_capacity(num_files);
    let mut resolveds = Vec::with_capacity(num_files);
    let mut doc_maps = Vec::with_capacity(num_files);
    let mut resolve_diags = Vec::with_capacity(num_files);
    let mut symbol_offsets = Vec::with_capacity(num_files);
    let mut resolutions = Vec::with_capacity(num_files);
    let mut type_checks = Vec::with_capacity(num_files);
    let mut hirs = Vec::with_capacity(num_files);
    let mut amirs = Vec::with_capacity(num_files);
    let mut resolve_deps = Vec::with_capacity(num_files);
    let mut typecheck_deps = Vec::with_capacity(num_files);
    let mut hir_deps = Vec::with_capacity(num_files);
    let mut amir_deps = Vec::with_capacity(num_files);

    for _ in 0..num_files {
        programs.push(Mutex::new(None));
        parse_errors.push(Mutex::new(Vec::new()));
        symbol_tables.push(Mutex::new(None));
        resolveds.push(Mutex::new(None));
        doc_maps.push(Mutex::new(None));
        resolve_diags.push(Mutex::new(Vec::new()));
        symbol_offsets.push(Mutex::new(0));
        resolutions.push(Mutex::new(None));
        type_checks.push(Mutex::new(None));
        hirs.push(Mutex::new(None));
        amirs.push(Mutex::new(None));
        resolve_deps.push(AtomicUsize::new(1));
        typecheck_deps.push(AtomicUsize::new(1));
        hir_deps.push(AtomicUsize::new(1));
        amir_deps.push(AtomicUsize::new(1));
    }

    let ctx = Arc::new(CompilationContext {
        paths,
        num_files,
        programs,
        parse_errors,
        symbol_tables,
        resolveds,
        doc_maps,
        resolve_diags,
        symbol_offsets,
        merged_symbol_table: Mutex::new(None),
        merged_docs: Mutex::new(None),
        merged_diags: Mutex::new(Vec::new()),
        resolutions,
        type_checks,
        hirs,
        amirs,
        diagnostics: Mutex::new(Vec::new()),
    });

    let state = Arc::new(SchedulerState {
        merge_deps: AtomicUsize::new(num_files),
        resolve_deps,
        typecheck_deps,
        hir_deps,
        amir_deps,
    });

    // 5 tasks per file (Parse, Resolve, Typecheck, Hir, Amir) + 1 Merge task
    let todo_count = Arc::new(AtomicUsize::new(5 * num_files + 1));
    let condvar = Arc::new(Condvar::new());
    let condvar_mutex = Arc::new(Mutex::new(()));

    let num_workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let mut queues = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        queues.push(TaskQueue {
            tasks: Mutex::new(VecDeque::new()),
        });
    }
    let queues = Arc::new(queues);

    // Initial load: parse tasks for all files
    for file_idx in 0..num_files {
        let worker = file_idx % num_workers;
        queues[worker].tasks.lock().unwrap().push_back(Task::Parse { file_idx });
    }

    let mut handles = Vec::with_capacity(num_workers);
    for worker_id in 0..num_workers {
        let queues = Arc::clone(&queues);
        let state = Arc::clone(&state);
        let ctx = Arc::clone(&ctx);
        let todo_count = Arc::clone(&todo_count);
        let condvar = Arc::clone(&condvar);
        let condvar_mutex = Arc::clone(&condvar_mutex);

        handles.push(thread::spawn(move || {
            worker_thread_loop(
                worker_id,
                num_workers,
                queues,
                state,
                ctx,
                todo_count,
                condvar,
                condvar_mutex,
            );
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }

    // Collect all diagnostics
    let mut final_diagnostics = std::mem::take(&mut *ctx.diagnostics.lock().unwrap());
    
    // Add parse errors
    for file_idx in 0..num_files {
        let errors = ctx.parse_errors[file_idx].lock().unwrap();
        let filepath = ctx.paths[file_idx].to_string_lossy();
        for err in &*errors {
            final_diagnostics.push(Diagnostic::error(
                crate::DiagCode::N006UnresolvedImport,
                err.format_for_cli(&filepath),
                err.span,
            ));
        }
    }

    // Add resolution and type check diagnostics
    {
        let res_diags = ctx.merged_diags.lock().unwrap();
        final_diagnostics.extend(res_diags.clone());
    }

    for file_idx in 0..num_files {
        if let Some(tc) = ctx.type_checks[file_idx].lock().unwrap().as_ref() {
            final_diagnostics.extend(tc.diagnostics.clone());
        }
    }

    let has_errors = final_diagnostics.iter().any(|d| d.severity == Severity::Error);
    if has_errors {
        return Err(final_diagnostics);
    }

    let mut result_hirs = Vec::with_capacity(num_files);
    let mut result_amirs = Vec::with_capacity(num_files);
    let mut result_symbols = Vec::with_capacity(num_files);
    for file_idx in 0..num_files {
        if let Some(hir) = ctx.hirs[file_idx].lock().unwrap().take() {
            result_hirs.push(hir);
        }
        if let Some(amir) = ctx.amirs[file_idx].lock().unwrap().take() {
            result_amirs.push(amir);
        }
        // Use per-file type-check symbols (includes locals, params, fields from resolve+typecheck)
        if let Some(tc) = ctx.type_checks[file_idx].lock().unwrap().as_ref() {
            result_symbols.push(tc.symbols.clone());
        } else {
            // Fallback to merged table if type-check didn't run (e.g. parse error)
            result_symbols.push(
                ctx.merged_symbol_table.lock().unwrap().clone().unwrap_or_else(SymbolTable::new)
            );
        }
    }

    Ok(ParallelOutput {
        paths: ctx.paths.clone(),
        hirs: result_hirs,
        amirs: result_amirs,
        symbols: result_symbols,
    })
}

fn worker_thread_loop(
    worker_id: usize,
    num_workers: usize,
    queues: Arc<Vec<TaskQueue>>,
    state: Arc<SchedulerState>,
    ctx: Arc<CompilationContext>,
    todo_count: Arc<AtomicUsize>,
    condvar: Arc<Condvar>,
    condvar_mutex: Arc<Mutex<()>>,
) {
    pin_thread_to_core(worker_id);

    loop {
        let mut task = None;

        // Try local queue
        if let Ok(mut q) = queues[worker_id].tasks.lock() {
            task = q.pop_back();
        }

        // Steal work if local queue is empty
        if task.is_none() {
            for offset in 1..num_workers {
                let target_id = (worker_id + offset) % num_workers;
                if let Ok(mut q) = queues[target_id].tasks.lock() {
                    if let Some(t) = q.pop_front() {
                        task = Some(t);
                        break;
                    }
                }
            }
        }

        if let Some(t) = task {
            run_task(t, worker_id, &queues, &state, &ctx, &todo_count, &condvar);
            continue;
        }

        if todo_count.load(Ordering::Acquire) == 0 {
            break;
        }

        let lock = condvar_mutex.lock().unwrap();
        if todo_count.load(Ordering::Acquire) == 0 {
            break;
        }
        let _unused = condvar.wait(lock).unwrap();
    }
}

fn enqueue_task(
    task: Task,
    target_worker: usize,
    queues: &Vec<TaskQueue>,
    condvar: &Condvar,
) {
    if let Ok(mut q) = queues[target_worker].tasks.lock() {
        q.push_back(task);
    }
    condvar.notify_all();
}

fn run_task(
    task: Task,
    worker_id: usize,
    queues: &Vec<TaskQueue>,
    state: &SchedulerState,
    ctx: &CompilationContext,
    todo_count: &AtomicUsize,
    condvar: &Condvar,
) {
    match task {
        Task::Parse { file_idx } => {
            let path = &ctx.paths[file_idx];
            if let Ok(source) = std::fs::read_to_string(path) {
                let output = arandu_parser::parse_recovering(&source);
                *ctx.programs[file_idx].lock().unwrap() = Some(output.program);
                *ctx.parse_errors[file_idx].lock().unwrap() = output.diagnostics;
            } else {
                ctx.diagnostics.lock().unwrap().push(Diagnostic::error(
                    crate::DiagCode::N006UnresolvedImport,
                    format!("failed to read file {}", path.display()),
                    arandu_lexer::Span::new(0, 0, 0, 0, 0, 0),
                ));
            }

            if let Some(program) = ctx.programs[file_idx].lock().unwrap().as_ref() {
                let (syms, resolved, docs, diags) = crate::passes::name_resolution::collect_symbols(program);
                *ctx.symbol_tables[file_idx].lock().unwrap() = Some(syms);
                *ctx.resolveds[file_idx].lock().unwrap() = Some(resolved);
                *ctx.doc_maps[file_idx].lock().unwrap() = Some(docs);
                *ctx.resolve_diags[file_idx].lock().unwrap() = diags;
            }

            let prev = state.merge_deps.fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                enqueue_task(Task::MergeSymbols, worker_id, queues, condvar);
            }
        }

        Task::MergeSymbols => {
            let mut combined_symbols = crate::passes::name_resolution::create_symbol_table_with_prelude();
            let mut combined_docs = crate::DocCommentMap::default();
            let mut combined_diags = Vec::new();

            for file_idx in 0..ctx.num_files {
                let current_offset = combined_symbols.iter().count();
                *ctx.symbol_offsets[file_idx].lock().unwrap() = current_offset;

                if let Some(syms) = ctx.symbol_tables[file_idx].lock().unwrap().take() {
                    combined_symbols.merge_from(syms);
                }
                if let Some(docs) = ctx.doc_maps[file_idx].lock().unwrap().take() {
                    for (k, v) in docs {
                        combined_docs.entry(k).or_default().extend(v);
                    }
                }
                let diags = std::mem::take(&mut *ctx.resolve_diags[file_idx].lock().unwrap());
                combined_diags.extend(diags);
            }

            *ctx.merged_symbol_table.lock().unwrap() = Some(combined_symbols);
            *ctx.merged_docs.lock().unwrap() = Some(combined_docs);
            *ctx.merged_diags.lock().unwrap() = combined_diags;

            for file_idx in 0..ctx.num_files {
                let prev = state.resolve_deps[file_idx].fetch_sub(1, Ordering::SeqCst);
                if prev == 1 {
                    enqueue_task(Task::Resolve { file_idx }, worker_id, queues, condvar);
                }
            }
        }

        Task::Resolve { file_idx } => {
            let program_lock = ctx.programs[file_idx].lock().unwrap();
            let mut resolved_lock = ctx.resolveds[file_idx].lock().unwrap();
            if let Some(program) = program_lock.as_ref()
                && let Some(mut resolved) = resolved_lock.take()
            {
                let offset = *ctx.symbol_offsets[file_idx].lock().unwrap();
                resolved.offset_symbols(offset as u32);

                let syms = ctx.merged_symbol_table.lock().unwrap().clone().unwrap();
                let docs = ctx.merged_docs.lock().unwrap().clone().unwrap();
                let diags = ctx.merged_diags.lock().unwrap().clone();

                let res = crate::passes::name_resolution::resolve_with_symbols(syms, resolved, docs, diags, program);
                *ctx.resolutions[file_idx].lock().unwrap() = Some(res);
            }

            let prev = state.typecheck_deps[file_idx].fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                enqueue_task(Task::TypeCheck { file_idx }, worker_id, queues, condvar);
            }
        }

        Task::TypeCheck { file_idx } => {
            let program_lock = ctx.programs[file_idx].lock().unwrap();
            let mut res_lock = ctx.resolutions[file_idx].lock().unwrap();
            if let Some(program) = program_lock.as_ref()
                && let Some(resolution) = res_lock.take()
            {
                let tc = crate::passes::type_checker::type_check(resolution, program);
                *ctx.type_checks[file_idx].lock().unwrap() = Some(tc);
            }

            let prev = state.hir_deps[file_idx].fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                enqueue_task(Task::LowerHir { file_idx }, worker_id, queues, condvar);
            }
        }

        Task::LowerHir { file_idx } => {
            let program_lock = ctx.programs[file_idx].lock().unwrap();
            let tc_lock = ctx.type_checks[file_idx].lock().unwrap();
            if let Some(program) = program_lock.as_ref()
                && let Some(tc) = tc_lock.as_ref()
            {
                match crate::lower_to_hir(tc, program) {
                    Ok(hir) => {
                        *ctx.hirs[file_idx].lock().unwrap() = Some(hir);
                    }
                    Err(diags) => {
                        ctx.diagnostics.lock().unwrap().extend(diags);
                    }
                }
            }

            let prev = state.amir_deps[file_idx].fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                enqueue_task(Task::LowerAmir { file_idx }, worker_id, queues, condvar);
            }
        }

        Task::LowerAmir { file_idx } => {
            let tc_lock = ctx.type_checks[file_idx].lock().unwrap();
            let hir_lock = ctx.hirs[file_idx].lock().unwrap();
            if let Some(tc) = tc_lock.as_ref()
                && let Some(hir) = hir_lock.as_ref()
            {
                match crate::lower_to_amir(tc, hir) {
                    Ok(mut amir) => {
                        crate::optimize_amir(&mut amir);
                        *ctx.amirs[file_idx].lock().unwrap() = Some(amir);
                    }
                    Err(diags) => {
                        ctx.diagnostics.lock().unwrap().extend(diags);
                    }
                }
            }
        }
    }

    let prev = todo_count.fetch_sub(1, Ordering::SeqCst);
    if prev == 1 {
        condvar.notify_all();
    }
}
