use crate::amir::AmirProgram;
use crate::hir::HirProgram;
use crate::{Diagnostic, ResolutionResult, ResolvedNames, Severity, SymbolTable, TypeCheckResult};
use arandu_parser::{ParseError, Program};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

#[cfg(target_os = "linux")]
fn pin_thread_to_core(core_id: usize) {
    unsafe {
        let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(core_id, &mut cpuset);
        let thread = libc::pthread_self();
        let _ =
            libc::pthread_setaffinity_np(thread, std::mem::size_of::<libc::cpu_set_t>(), &cpuset);
    }
}

#[cfg(not(target_os = "linux"))]
fn pin_thread_to_core(_core_id: usize) {}

pub enum Task {
    Parse {
        file_idx: usize,
        path: PathBuf,
    },
    Resolve {
        file_idx: usize,
        program: Program,
        resolved_names: ResolvedNames,
        symbols: Arc<SymbolTable>,
        docs: Arc<crate::DocCommentMap>,
        diags: Arc<Vec<Diagnostic>>,
    },
    TypeCheck {
        file_idx: usize,
        program: Program,
        resolution: ResolutionResult,
    },
    LowerHir {
        file_idx: usize,
        program: Program,
        type_check_result: Arc<TypeCheckResult>,
    },
    LowerAmir {
        file_idx: usize,
        hir: HirProgram,
        type_check_result: Arc<TypeCheckResult>,
    },
    Shutdown,
}

pub enum WorkerMessage {
    ParseFinished {
        file_idx: usize,
        program: Option<Program>,
        parse_errors: Vec<ParseError>,
        symbol_table: Option<SymbolTable>,
        resolved_names: Option<ResolvedNames>,
        doc_map: Option<crate::DocCommentMap>,
        resolve_diags: Vec<Diagnostic>,
        io_error: Option<Diagnostic>,
    },
    ResolveFinished {
        file_idx: usize,
        program: Program,
        resolution: Option<ResolutionResult>,
    },
    TypeCheckFinished {
        file_idx: usize,
        program: Program,
        type_check_result: Option<Arc<TypeCheckResult>>,
    },
    LowerHirFinished {
        file_idx: usize,
        hir: Option<HirProgram>,
        diagnostics: Vec<Diagnostic>,
    },
    LowerAmirFinished {
        file_idx: usize,
        amir: Option<AmirProgram>,
        hir: HirProgram,
        diagnostics: Vec<Diagnostic>,
    },
}

#[derive(Debug)]
pub struct ParallelOutput {
    pub paths: Vec<PathBuf>,
    pub hirs: Vec<HirProgram>,
    pub amirs: Vec<AmirProgram>,
    pub symbols: Vec<SymbolTable>,
    pub type_interners: Vec<crate::passes::type_checker::types::TypeInterner>,
}

pub fn compile_parallel(paths: Vec<PathBuf>) -> Result<ParallelOutput, Vec<Diagnostic>> {
    let num_files = paths.len();
    if num_files == 0 {
        return Ok(ParallelOutput {
            paths: Vec::new(),
            hirs: Vec::new(),
            amirs: Vec::new(),
            symbols: Vec::new(),
            type_interners: Vec::new(),
        });
    }

    let mut programs = vec![None; num_files];
    let mut resolveds = vec![None; num_files];
    let mut symbol_offsets = vec![0; num_files];
    let mut resolutions = vec![None; num_files];
    let mut type_checks = vec![None; num_files];
    let mut hirs = Vec::new();
    hirs.resize_with(num_files, || None);
    let mut amirs = Vec::new();
    amirs.resize_with(num_files, || None);
    let mut final_diagnostics = Vec::new();

    let mut symbol_tables = vec![None; num_files];
    let mut doc_maps = vec![None; num_files];
    let mut resolve_diags = vec![Vec::new(); num_files];

    let (tx_to_coordinator, rx_from_workers) = std::sync::mpsc::channel();
    let mut worker_senders = Vec::new();
    let mut worker_handles = Vec::new();

    let num_workers = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    for worker_id in 0..num_workers {
        let (tx, rx) = std::sync::mpsc::channel();
        worker_senders.push(tx);
        let tx_coordinator = tx_to_coordinator.clone();
        worker_handles.push(thread::spawn(move || {
            worker_thread_loop(worker_id, rx, tx_coordinator);
        }));
    }

    // Send initial Parse tasks
    for file_idx in 0..num_files {
        let worker_idx = file_idx % num_workers;
        let _ = worker_senders[worker_idx].send(Task::Parse {
            file_idx,
            path: paths[file_idx].clone(),
        });
    }

    let mut parsed_count = 0;
    let mut resolved_count = 0;
    let mut typechecked_count = 0;
    let mut hir_count = 0;
    let mut amir_count = 0;

    let mut merged_symbols = None;
    let mut merged_docs = None;
    let mut merged_diags = Vec::new();

    while let Ok(msg) = rx_from_workers.recv() {
        match msg {
            WorkerMessage::ParseFinished {
                file_idx,
                program,
                parse_errors: errs,
                symbol_table,
                resolved_names,
                doc_map,
                resolve_diags: r_diags,
                io_error,
            } => {
                if let Some(io_err) = io_error {
                    final_diagnostics.push(io_err);
                }
                for err in errs {
                    final_diagnostics.push(Diagnostic::from(err));
                }
                programs[file_idx] = program;
                symbol_tables[file_idx] = symbol_table;
                resolveds[file_idx] = resolved_names;
                doc_maps[file_idx] = doc_map;
                resolve_diags[file_idx] = r_diags;

                parsed_count += 1;
                if parsed_count == num_files {
                    // All files parsed. Merge symbols!
                    let mut combined_symbols =
                        crate::passes::name_resolution::create_symbol_table_with_prelude();
                    let mut combined_docs = crate::DocCommentMap::default();
                    let mut combined_diags = Vec::new();

                    for f_idx in 0..num_files {
                        let current_offset = combined_symbols.iter().count();
                        symbol_offsets[f_idx] = current_offset;

                        if let Some(syms) = symbol_tables[f_idx].take() {
                            combined_symbols.merge_from(syms);
                        }
                        if let Some(docs) = doc_maps[f_idx].take() {
                            for (k, v) in docs {
                                combined_docs.entry(k).or_default().extend(v);
                            }
                        }
                        let diags = std::mem::take(&mut resolve_diags[f_idx]);
                        combined_diags.extend(diags);
                    }

                    let arc_symbols = Arc::new(combined_symbols);
                    let arc_docs = Arc::new(combined_docs);
                    let arc_diags = Arc::new(combined_diags.clone());

                    merged_symbols = Some(arc_symbols.clone());
                    merged_docs = Some(arc_docs.clone());
                    merged_diags = combined_diags;

                    // Trigger Resolve tasks
                    for f_idx in 0..num_files {
                        if let Some(prog) = programs[f_idx].take()
                            && let Some(mut resolved) = resolveds[f_idx].take()
                        {
                            let offset = symbol_offsets[f_idx];
                            resolved.offset_symbols(offset as u32);

                            let worker_idx = f_idx % num_workers;
                            let _ = worker_senders[worker_idx].send(Task::Resolve {
                                file_idx: f_idx,
                                program: prog,
                                resolved_names: resolved,
                                symbols: arc_symbols.clone(),
                                docs: arc_docs.clone(),
                                diags: arc_diags.clone(),
                            });
                        } else {
                            resolved_count += 1;
                        }
                    }

                    if resolved_count == num_files {
                        break;
                    }
                }
            }
            WorkerMessage::ResolveFinished {
                file_idx,
                program,
                resolution,
            } => {
                resolutions[file_idx] = resolution;
                programs[file_idx] = Some(program);

                resolved_count += 1;
                if resolved_count == num_files {
                    // Start Typecheck
                    for f_idx in 0..num_files {
                        if let Some(prog) = programs[f_idx].take()
                            && let Some(res) = resolutions[f_idx].take()
                        {
                            let worker_idx = f_idx % num_workers;
                            let _ = worker_senders[worker_idx].send(Task::TypeCheck {
                                file_idx: f_idx,
                                program: prog,
                                resolution: res,
                            });
                        } else {
                            typechecked_count += 1;
                        }
                    }

                    if typechecked_count == num_files {
                        break;
                    }
                }
            }
            WorkerMessage::TypeCheckFinished {
                file_idx,
                program,
                type_check_result,
            } => {
                type_checks[file_idx] = type_check_result;
                programs[file_idx] = Some(program);

                typechecked_count += 1;
                if typechecked_count == num_files {
                    // Start Lower HIR
                    for f_idx in 0..num_files {
                        if let Some(prog) = programs[f_idx].take()
                            && let Some(tc) = type_checks[f_idx].as_ref()
                        {
                            let worker_idx = f_idx % num_workers;
                            let _ = worker_senders[worker_idx].send(Task::LowerHir {
                                file_idx: f_idx,
                                program: prog,
                                type_check_result: Arc::clone(tc),
                            });
                        } else {
                            hir_count += 1;
                        }
                    }

                    if hir_count == num_files {
                        break;
                    }
                }
            }
            WorkerMessage::LowerHirFinished {
                file_idx,
                hir,
                diagnostics,
            } => {
                final_diagnostics.extend(diagnostics);
                hirs[file_idx] = hir;

                hir_count += 1;
                if hir_count == num_files {
                    // Start Lower AMIR
                    for f_idx in 0..num_files {
                        if let Some(hir) = hirs[f_idx].take()
                            && let Some(tc) = type_checks[f_idx].as_ref()
                        {
                            let worker_idx = f_idx % num_workers;
                            let _ = worker_senders[worker_idx].send(Task::LowerAmir {
                                file_idx: f_idx,
                                hir,
                                type_check_result: Arc::clone(tc),
                            });
                        } else {
                            amir_count += 1;
                        }
                    }

                    if amir_count == num_files {
                        break;
                    }
                }
            }
            WorkerMessage::LowerAmirFinished {
                file_idx,
                amir,
                hir,
                diagnostics,
            } => {
                final_diagnostics.extend(diagnostics);
                amirs[file_idx] = amir;
                hirs[file_idx] = Some(hir);

                amir_count += 1;
                if amir_count == num_files {
                    break;
                }
            }
        }
    }

    // Shutdown workers
    for sender in &worker_senders {
        let _ = sender.send(Task::Shutdown);
    }
    for handle in worker_handles {
        let _ = handle.join();
    }

    // Collect diagnostics from type checks and name resolution
    final_diagnostics.extend(merged_diags);
    for file_idx in 0..num_files {
        if let Some(tc) = type_checks[file_idx].as_ref() {
            final_diagnostics.extend(tc.diagnostics.clone());
        }
    }

    let has_errors = final_diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    if has_errors {
        return Err(final_diagnostics);
    }

    let mut result_hirs = Vec::with_capacity(num_files);
    let mut result_amirs = Vec::with_capacity(num_files);
    let mut result_symbols = Vec::with_capacity(num_files);
    let mut result_interners = Vec::with_capacity(num_files);
    for file_idx in 0..num_files {
        if let Some(hir) = hirs[file_idx].take() {
            result_hirs.push(hir);
        }
        if let Some(amir) = amirs[file_idx].take() {
            result_amirs.push(amir);
        }
        if let Some(tc) = type_checks[file_idx].as_ref() {
            result_symbols.push(tc.symbols.clone());
            result_interners.push(tc.type_info.type_interner.clone());
        } else {
            result_symbols.push(
                merged_symbols
                    .as_ref()
                    .map(|s| (**s).clone())
                    .unwrap_or_default(),
            );
            result_interners.push(crate::passes::type_checker::types::TypeInterner::default());
        }
    }

    Ok(ParallelOutput {
        paths,
        hirs: result_hirs,
        amirs: result_amirs,
        symbols: result_symbols,
        type_interners: result_interners,
    })
}

fn worker_thread_loop(
    worker_id: usize,
    rx: std::sync::mpsc::Receiver<Task>,
    tx: std::sync::mpsc::Sender<WorkerMessage>,
) {
    pin_thread_to_core(worker_id);

    while let Ok(task) = rx.recv() {
        match task {
            Task::Parse { file_idx, path } => {
                let mut program = None;
                let mut parse_errors = Vec::new();
                let mut symbol_table = None;
                let mut resolved_names = None;
                let mut doc_map = None;
                let mut resolve_diags = Vec::new();
                let mut io_error = None;

                match std::fs::read_to_string(&path) {
                    Ok(source) => {
                        let output =
                            arandu_parser::parse_recovering_with_file_id(&source, file_idx as u32);
                        parse_errors = output.diagnostics;
                        let prog = output.program;

                        let (syms, resolved, docs, diags) =
                            crate::passes::name_resolution::collect_symbols(&prog);
                        symbol_table = Some(syms);
                        resolved_names = Some(resolved);
                        doc_map = Some(docs);
                        resolve_diags = diags;
                        program = Some(prog);
                    }
                    Err(_) => {
                        io_error = Some(Diagnostic::error(
                            crate::DiagCode::M001UnresolvedImport,
                            format!("failed to read file {}", path.display()),
                            arandu_lexer::Span::new(file_idx as u32, 0, 0),
                        ));
                    }
                }

                let _ = tx.send(WorkerMessage::ParseFinished {
                    file_idx,
                    program,
                    parse_errors,
                    symbol_table,
                    resolved_names,
                    doc_map,
                    resolve_diags,
                    io_error,
                });
            }
            Task::Resolve {
                file_idx,
                program,
                resolved_names,
                symbols,
                docs,
                diags,
            } => {
                let res = crate::passes::name_resolution::resolve_with_symbols(
                    (*symbols).clone(),
                    resolved_names,
                    (*docs).clone(),
                    (*diags).clone(),
                    &program,
                );
                let _ = tx.send(WorkerMessage::ResolveFinished {
                    file_idx,
                    program,
                    resolution: Some(res),
                });
            }
            Task::TypeCheck {
                file_idx,
                program,
                resolution,
            } => {
                let tc = crate::passes::type_checker::type_check(resolution, &program);
                let _ = tx.send(WorkerMessage::TypeCheckFinished {
                    file_idx,
                    program,
                    type_check_result: Some(Arc::new(tc)),
                });
            }
            Task::LowerHir {
                file_idx,
                program,
                type_check_result,
            } => {
                let mut hir = None;
                let mut diagnostics = Vec::new();
                match crate::lower_to_hir(&type_check_result, &program) {
                    Ok(h) => hir = Some(h),
                    Err(diags) => diagnostics = diags,
                }
                let _ = tx.send(WorkerMessage::LowerHirFinished {
                    file_idx,
                    hir,
                    diagnostics,
                });
            }
            Task::LowerAmir {
                file_idx,
                hir,
                type_check_result,
            } => {
                let mut amir = None;
                let mut diagnostics = Vec::new();
                match crate::lower_to_amir(&type_check_result, &hir) {
                    Ok(mut a) => {
                        crate::optimize_amir(&mut a);
                        amir = Some(a);
                    }
                    Err(diags) => diagnostics = diags,
                }
                let _ = tx.send(WorkerMessage::LowerAmirFinished {
                    file_idx,
                    amir,
                    hir,
                    diagnostics,
                });
            }
            Task::Shutdown => break,
        }
    }
}
