//! DX.6 / P4 — synchronous LSP: main + VFS debounce + worker pool + full IDE caps.
//!
//! Protocol: `lsp-server` + `lsp-types` (no async on the analysis path).

mod conv;
mod ide;
mod pool;
mod state;
mod vfs;

use arandu_base::LineIndex;
use arandu_query::{AnalysisRevision, AnalysisSnapshot, ArandCompilerDb, DocumentId, SourceFile};
use conv::{position_to_offset, span_to_range};
use crossbeam_channel::{select, Receiver, Sender};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentSymbolRequest, Formatting, GotoDefinition, HoverRequest,
    References, Rename, Request as _, SemanticTokensFullRequest, SignatureHelpRequest,
    WorkspaceSymbolRequest,
};
use lsp_types::{
    CodeActionOptions, CodeActionProviderCapability, CompletionOptions, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DocumentSymbolResponse, GotoDefinitionResponse,
    HoverProviderCapability, InitializeResult, Location, NumberOrString, OneOf, Position,
    PublishDiagnosticsParams, RenameOptions, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SignatureHelpOptions,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url, WorkDoneProgressOptions,
    WorkspaceSymbolParams,
};
use pool::WorkerPool;
use rustc_hash::FxHashMap;
use state::{walk_register_aru, ServerState};
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

enum JobResult {
    Diagnostics {
        uri: Url,
        doc_id: DocumentId,
        revision: AnalysisRevision,
        fingerprint: [u8; 32],
        diags: Vec<Diagnostic>,
    },
    JsonResponse {
        id: RequestId,
        revision: AnalysisRevision,
        value: serde_json::Value,
    },
}

#[derive(Clone)]
struct DocInfo {
    source: SourceFile,
    path: Arc<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    let (connection, io_threads) = Connection::stdio();
    let (initialize_id, initialize_params) = connection.initialize_start()?;
    let init: lsp_types::InitializeParams = serde_json::from_value(initialize_params)?;

    let mut state = ServerState::new();
    if let Some(folders) = init.workspace_folders {
        for folder in folders {
            if let Ok(root) = folder.uri.to_file_path() {
                walk_register_aru(&mut state, &root);
            }
        }
    } else if let Some(root) = init.root_uri.as_ref().and_then(|u| u.to_file_path().ok()) {
        walk_register_aru(&mut state, &root);
    }

    let server_caps = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".into(), ":".into()]),
            ..CompletionOptions::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: None,
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(false),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: ide::semantic_tokens_legend(),
                range: Some(false),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            },
        )),
        document_formatting_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![lsp_types::CodeActionKind::QUICKFIX]),
            work_done_progress_options: WorkDoneProgressOptions::default(),
            resolve_provider: None,
        })),
        ..ServerCapabilities::default()
    };
    let init_result = InitializeResult {
        capabilities: server_caps,
        server_info: Some(ServerInfo {
            name: "arandu-lsp".into(),
            version: Some(env!("CARGO_PKG_VERSION").into()),
        }),
    };
    connection.initialize_finish(initialize_id, serde_json::to_value(init_result)?)?;

    let pool = WorkerPool::new(4);
    let (job_tx, job_rx) = crossbeam_channel::unbounded::<JobResult>();
    event_loop(&connection, &mut state, &pool, job_tx, job_rx)?;
    io_threads.join()?;
    Ok(())
}

fn event_loop(
    connection: &Connection,
    state: &mut ServerState,
    pool: &WorkerPool,
    job_tx: Sender<JobResult>,
    job_rx: Receiver<JobResult>,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    loop {
        let timeout = state
            .vfs
            .next_deadline()
            .unwrap_or(Duration::from_secs(3600));

        select! {
            recv(connection.receiver) -> msg => {
                let Ok(msg) = msg else { break };
                match msg {
                    Message::Request(req) => {
                        if connection.handle_shutdown(&req)? {
                            return Ok(());
                        }
                        on_request(connection, state, pool, &job_tx, req)?;
                    }
                    Message::Notification(not) => {
                        on_notification(connection, state, pool, &job_tx, not)?;
                    }
                    Message::Response(_) => {}
                }
            }
            recv(job_rx) -> job => {
                if let Ok(job) = job {
                    handle_job_result(connection, state, job)?;
                }
            }
            default(timeout) => {
                let committed = state.flush_due();
                for (uri, doc_id) in committed {
                    spawn_diagnostics(state, pool, &job_tx, uri, doc_id);
                }
            }
        }
    }
    Ok(())
}

fn on_notification(
    connection: &Connection,
    state: &mut ServerState,
    pool: &WorkerPool,
    job_tx: &Sender<JobResult>,
    not: Notification,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    match not.method.as_str() {
        DidOpenTextDocument::METHOD => {
            let params: lsp_types::DidOpenTextDocumentParams =
                not.extract(DidOpenTextDocument::METHOD)?;
            let uri = params.text_document.uri;
            let id = state.open_or_commit(&uri, params.text_document.text);
            spawn_diagnostics(state, pool, job_tx, uri, id);
        }
        DidChangeTextDocument::METHOD => {
            let params: lsp_types::DidChangeTextDocumentParams =
                not.extract(DidChangeTextDocument::METHOD)?;
            let uri = params.text_document.uri;
            let text = params
                .content_changes
                .into_iter()
                .next()
                .map(|c| c.text)
                .unwrap_or_default();
            if !state.by_uri.contains_key(uri.as_str()) {
                let _ = state.open_or_commit(&uri, text.clone());
            }
            state.queue_change(&uri, text);
        }
        DidSaveTextDocument::METHOD => {
            let params: lsp_types::DidSaveTextDocumentParams =
                not.extract(DidSaveTextDocument::METHOD)?;
            if let Some(text) = params.text {
                state.queue_change(&params.text_document.uri, text);
            }
            let committed = state.flush_all();
            if committed.is_empty() {
                if let Some(&id) = state.by_uri.get(params.text_document.uri.as_str()) {
                    spawn_diagnostics(state, pool, job_tx, params.text_document.uri, id);
                }
            } else {
                for (uri, doc_id) in committed {
                    spawn_diagnostics(state, pool, job_tx, uri, doc_id);
                }
            }
        }
        DidCloseTextDocument::METHOD => {
            let params: lsp_types::DidCloseTextDocumentParams =
                not.extract(DidCloseTextDocument::METHOD)?;
            let uri = params.text_document.uri;
            state.close_uri(&uri);
            publish_diagnostics(connection, uri, Vec::new())?;
        }
        _ => {}
    }
    Ok(())
}

fn flush_for_request(state: &mut ServerState, pool: &WorkerPool, job_tx: &Sender<JobResult>) {
    let committed = state.flush_all();
    for (uri, doc_id) in committed {
        spawn_diagnostics(state, pool, job_tx, uri, doc_id);
    }
}

fn on_request(
    connection: &Connection,
    state: &mut ServerState,
    pool: &WorkerPool,
    job_tx: &Sender<JobResult>,
    req: Request,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    // Ensure pending edits are visible for semantic requests.
    match req.method.as_str() {
        GotoDefinition::METHOD
        | HoverRequest::METHOD
        | Completion::METHOD
        | References::METHOD
        | Rename::METHOD
        | SignatureHelpRequest::METHOD
        | DocumentSymbolRequest::METHOD
        | WorkspaceSymbolRequest::METHOD
        | SemanticTokensFullRequest::METHOD
        | Formatting::METHOD
        | CodeActionRequest::METHOD => {
            flush_for_request(state, pool, job_tx);
        }
        _ => {}
    }

    match req.method.as_str() {
        GotoDefinition::METHOD => {
            let (id, params) =
                req.extract::<lsp_types::GotoDefinitionParams>(GotoDefinition::METHOD)?;
            let uri = params.text_document_position_params.text_document.uri;
            let pos = params.text_document_position_params.position;
            spawn_goto(state, pool, job_tx, id, uri, pos);
        }
        HoverRequest::METHOD => {
            let (id, params) = req.extract::<lsp_types::HoverParams>(HoverRequest::METHOD)?;
            let uri = params.text_document_position_params.text_document.uri;
            let pos = params.text_document_position_params.position;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                match ide::hover(snap, info.source, &text, pos) {
                    Some(h) => serde_json::to_value(h).unwrap_or(serde_json::Value::Null),
                    None => serde_json::Value::Null,
                }
            });
        }
        Completion::METHOD => {
            let (id, params) = req.extract::<lsp_types::CompletionParams>(Completion::METHOD)?;
            let uri = params.text_document_position.text_document.uri;
            let pos = params.text_document_position.position;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                let items = ide::completions(snap, info.source, &text, pos);
                serde_json::to_value(CompletionResponse::Array(items))
                    .unwrap_or(serde_json::Value::Null)
            });
        }
        SignatureHelpRequest::METHOD => {
            let (id, params) =
                req.extract::<lsp_types::SignatureHelpParams>(SignatureHelpRequest::METHOD)?;
            let uri = params.text_document_position_params.text_document.uri;
            let pos = params.text_document_position_params.position;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                match ide::signature_help(snap, info.source, &text, pos) {
                    Some(h) => serde_json::to_value(h).unwrap_or(serde_json::Value::Null),
                    None => serde_json::Value::Null,
                }
            });
        }
        References::METHOD => {
            let (id, params) = req.extract::<lsp_types::ReferenceParams>(References::METHOD)?;
            let uri = params.text_document_position.text_document.uri;
            let pos = params.text_document_position.position;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                let locs = ide::references(snap, info.source, &text, pos, &uri);
                serde_json::to_value(locs).unwrap_or(serde_json::Value::Null)
            });
        }
        Rename::METHOD => {
            let (id, params) = req.extract::<lsp_types::RenameParams>(Rename::METHOD)?;
            let uri = params.text_document_position.text_document.uri;
            let pos = params.text_document_position.position;
            let new_name = params.new_name;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                match ide::rename_edits(snap, info.source, &text, pos, &uri, &new_name) {
                    Some(edit) => serde_json::to_value(edit).unwrap_or(serde_json::Value::Null),
                    None => serde_json::Value::Null,
                }
            });
        }
        DocumentSymbolRequest::METHOD => {
            let (id, params) =
                req.extract::<lsp_types::DocumentSymbolParams>(DocumentSymbolRequest::METHOD)?;
            let uri = params.text_document.uri;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                let syms = ide::document_symbols(snap, info.source, &text, &uri);
                serde_json::to_value(DocumentSymbolResponse::Flat(syms))
                    .unwrap_or(serde_json::Value::Null)
            });
        }
        WorkspaceSymbolRequest::METHOD => {
            let (id, params) =
                req.extract::<WorkspaceSymbolParams>(WorkspaceSymbolRequest::METHOD)?;
            let query = params.query;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let list: Vec<ide::DocSnap> = docs
                    .iter()
                    .map(|(uri_s, info)| ide::DocSnap {
                        source: info.source,
                        path: Arc::clone(&info.path),
                        uri: Url::parse(uri_s).unwrap_or_else(|_| Url::parse("file:///").unwrap()),
                    })
                    .collect();
                let syms = ide::workspace_symbols(snap, &list, &query);
                serde_json::to_value(syms).unwrap_or(serde_json::Value::Null)
            });
        }
        SemanticTokensFullRequest::METHOD => {
            let (id, params) =
                req.extract::<lsp_types::SemanticTokensParams>(SemanticTokensFullRequest::METHOD)?;
            let uri = params.text_document.uri;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let tokens = ide::semantic_tokens(snap, info.source);
                serde_json::to_value(tokens).unwrap_or(serde_json::Value::Null)
            });
        }
        Formatting::METHOD => {
            let (id, params) =
                req.extract::<lsp_types::DocumentFormattingParams>(Formatting::METHOD)?;
            let uri = params.text_document.uri;
            spawn_json(state, pool, job_tx, id, move |snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let text = info.source.text(&snap.db);
                let edits = ide::format_document(&text);
                serde_json::to_value(edits).unwrap_or(serde_json::Value::Null)
            });
        }
        CodeActionRequest::METHOD => {
            let (id, params) =
                req.extract::<lsp_types::CodeActionParams>(CodeActionRequest::METHOD)?;
            let uri = params.text_document.uri;
            let context = params.context;
            spawn_json(state, pool, job_tx, id, move |_snap, docs| {
                let Some(info) = docs.get(uri.as_str()) else {
                    return serde_json::Value::Null;
                };
                let actions = ide::code_actions(&uri, &context);
                let _ = info;
                serde_json::to_value(actions).unwrap_or(serde_json::Value::Null)
            });
        }
        _ => {
            let resp = Response::new_err(
                req.id,
                lsp_server::ErrorCode::MethodNotFound as i32,
                format!("unknown request {}", req.method),
            );
            connection.sender.send(Message::Response(resp))?;
        }
    }
    Ok(())
}

fn collect_docs_map(state: &ServerState) -> FxHashMap<String, DocInfo> {
    let mut map = FxHashMap::default();
    for (uri, &id) in &state.by_uri {
        if let Some(doc) = state.docs.get(id) {
            map.insert(
                uri.clone(),
                DocInfo {
                    source: doc.source,
                    path: Arc::clone(&doc.path),
                },
            );
        }
    }
    map
}

fn spawn_diagnostics(
    state: &ServerState,
    pool: &WorkerPool,
    job_tx: &Sender<JobResult>,
    uri: Url,
    doc_id: DocumentId,
) {
    let Some(doc) = state.docs.get(doc_id) else {
        return;
    };
    let source = doc.source;
    let snap = state.snapshot();
    let revision = snap.revision;
    let tx = job_tx.clone();
    pool.spawn(move || {
        let (diags, fingerprint) = compute_diagnostics(&snap, source);
        let _ = tx.send(JobResult::Diagnostics {
            uri,
            doc_id,
            revision,
            fingerprint,
            diags,
        });
    });
}

fn spawn_goto(
    state: &ServerState,
    pool: &WorkerPool,
    job_tx: &Sender<JobResult>,
    req_id: RequestId,
    uri: Url,
    pos: Position,
) {
    let snap = state.snapshot();
    let revision = snap.revision;
    let by_uri = state.by_uri.clone();
    let by_file_id = state.by_file_id.clone();
    let docs = collect_doc_infos(state);
    let tx = job_tx.clone();
    pool.spawn(move || {
        let location = goto_on_snapshot(&snap, &by_uri, &by_file_id, &docs, &uri, pos);
        let value = match location {
            Some(loc) => serde_json::to_value(GotoDefinitionResponse::Scalar(loc))
                .unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        };
        let _ = tx.send(JobResult::JsonResponse {
            id: req_id,
            revision,
            value,
        });
    });
}

fn spawn_json<F>(
    state: &ServerState,
    pool: &WorkerPool,
    job_tx: &Sender<JobResult>,
    req_id: RequestId,
    f: F,
) where
    F: FnOnce(&AnalysisSnapshot, &FxHashMap<String, DocInfo>) -> serde_json::Value + Send + 'static,
{
    let snap = state.snapshot();
    let revision = snap.revision;
    let docs = collect_docs_map(state);
    let tx = job_tx.clone();
    pool.spawn(move || {
        let value = f(&snap, &docs);
        let _ = tx.send(JobResult::JsonResponse {
            id: req_id,
            revision,
            value,
        });
    });
}

fn compute_diagnostics(snap: &AnalysisSnapshot, source: SourceFile) -> (Vec<Diagnostic>, [u8; 32]) {
    let text = source.text(&snap.db);
    let index = LineIndex::new(&text);
    let ide = arandu_query::file_ide_diagnostics(&snap.db, source);
    let fp = arandu_query::ide_diags_fingerprint(&ide);
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(fp.as_bytes());
    let diags = ide
        .iter()
        .map(|d| {
            let span = arandu_base::Span::new(d.file_id, d.start, d.end);
            let range = span_to_range(&index, span);
            let severity = match d.severity {
                0 => DiagnosticSeverity::ERROR,
                1 => DiagnosticSeverity::WARNING,
                2 => DiagnosticSeverity::INFORMATION,
                _ => DiagnosticSeverity::HINT,
            };
            // Include block id in related info when present (honesty: real block tags).
            let message = if let Some(b) = d.block {
                format!("{} [bb{}]", d.message, b.as_usize())
            } else {
                d.message.clone()
            };
            Diagnostic {
                range,
                severity: Some(severity),
                code: Some(NumberOrString::String(d.code.clone())),
                message,
                source: Some("arandu".into()),
                ..Diagnostic::default()
            }
        })
        .collect();
    (diags, bytes)
}

fn collect_doc_infos(state: &ServerState) -> FxHashMap<DocumentId, DocInfo> {
    let mut by_id = FxHashMap::default();
    for &id in state.by_uri.values() {
        if let Some(doc) = state.docs.get(id) {
            by_id.insert(
                id,
                DocInfo {
                    source: doc.source,
                    path: Arc::clone(&doc.path),
                },
            );
        }
    }
    by_id
}

fn goto_on_snapshot(
    snap: &AnalysisSnapshot,
    by_uri: &FxHashMap<String, DocumentId>,
    by_file_id: &FxHashMap<u32, DocumentId>,
    docs: &FxHashMap<DocumentId, DocInfo>,
    uri: &Url,
    position: Position,
) -> Option<Location> {
    use arandu_base::{LineIndex, Span};
    use arandu_query::LspSymbolId;

    let id = *by_uri.get(uri.as_str())?;
    let info = docs.get(&id)?;
    let text = info.source.text(&snap.db);
    let index = LineIndex::new(&text);
    let offset = position_to_offset(&index, position, &text);
    let tc = arandu_query::passes::type_check(&snap.db, info.source);
    let sym_id = state::ServerState::symbol_at(&tc, offset)?;
    let lsp_sym = LspSymbolId::new(sym_id, snap.revision);
    let sym_id = lsp_sym.resolve(snap)?;
    let symbol = tc.symbols.try_get(sym_id)?;
    let def_span: Span = symbol.span;
    let def_uri = uri_for_file_id(by_file_id, docs, &snap.db, def_span.file_id)?;
    let def_text = if def_span.file_id == info.source.file_id(&snap.db) {
        text
    } else if let Some(&def_id) = by_file_id.get(&def_span.file_id) {
        let d = docs.get(&def_id)?;
        d.source.text(&snap.db)
    } else {
        let p = snap.db.file_path(def_span.file_id);
        Arc::from(std::fs::read_to_string(p.as_ref()).ok()?.as_str())
    };
    let def_index = LineIndex::new(&def_text);
    Some(Location {
        uri: def_uri,
        range: span_to_range(&def_index, def_span),
    })
}

fn uri_for_file_id(
    by_file_id: &FxHashMap<u32, DocumentId>,
    docs: &FxHashMap<DocumentId, DocInfo>,
    db: &arandu_query::DatabaseImpl,
    file_id: u32,
) -> Option<Url> {
    if let Some(&id) = by_file_id.get(&file_id) {
        if let Some(doc) = docs.get(&id) {
            return Url::from_file_path(doc.path.as_ref()).ok();
        }
    }
    let path = db.file_path(file_id);
    if path.as_os_str().is_empty() {
        return None;
    }
    Url::from_file_path(path.as_ref()).ok()
}

fn handle_job_result(
    connection: &Connection,
    state: &mut ServerState,
    job: JobResult,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    match job {
        JobResult::Diagnostics {
            uri,
            doc_id,
            revision,
            fingerprint,
            diags,
        } => {
            if state.docs.get(doc_id).is_none() {
                return Ok(());
            }
            if revision != state.revision() {
                return Ok(());
            }
            if state.last_diag_fp.get(&doc_id) == Some(&fingerprint) {
                return Ok(());
            }
            publish_diagnostics(connection, uri, diags)?;
            state.last_diag_fp.insert(doc_id, fingerprint);
        }
        JobResult::JsonResponse {
            id,
            revision,
            value,
        } => {
            if revision != state.revision() {
                connection.sender.send(Message::Response(Response::new_ok(
                    id,
                    serde_json::Value::Null,
                )))?;
                return Ok(());
            }
            connection
                .sender
                .send(Message::Response(Response::new_ok(id, value)))?;
        }
    }
    Ok(())
}

fn publish_diagnostics(
    connection: &Connection,
    uri: Url,
    diagnostics: Vec<Diagnostic>,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    };
    let not = Notification::new(
        PublishDiagnostics::METHOD.to_string(),
        serde_json::to_value(params)?,
    );
    connection.sender.send(Message::Notification(not))?;
    Ok(())
}
