# Plano gold — fechar Salsa, LSP e Generational IDs

**Status:** F0–F5 **implementados** (2026-07), incluindo F4 delta de diags / early cutoff por função.  
**Relacionado:** [`arandu-salsa-lsp-architecture-v0.1.md`](./arandu-salsa-lsp-architecture-v0.1.md), RFC Salsa, roadmap A1 / DX.6 / A10.  
**Princípio:** fechar na **origem** (modelo de concorrência + VFS + queries + política de handles). Reusar libs (`salsa`, `slotmap`, `lsp-server`/`lsp-types`). Sem polir o MVP async como se fosse o alvo. Sem afirmar superioridade a rust-analyzer.

**Fora de escopo:** ToStr/stdlib ownership, F2 `&` da linguagem, Display user, LLVM, CST Rowan (D7 = reparse de arquivo).

---

## 1. Estado real (auditoria 2026-07)

| Eixo | Já existe (honesto) | Ainda não é gold |
|------|---------------------|------------------|
| **Salsa (A1)** | `arandu_query` dono do grafo; `parse` → `resolve` → `module_signatures` → `type_check` → `lower_amir`; PERF.5 Arc; DX.5 `RebuildLog`; `CompileSession` **removido**; `symbol_span` real; guardrails `architecture_invariants`; `liveness_facts` via `arandu_mir::liveness`; `block_dataflow_facts` resumo por bloco (live-in/out counts) | I/O residual em `resolve_module_path` (disco no fallback — OK na DB, documentar); `block_dataflow` ainda **não** expõe init/move lattices; `func_amir` clona `AmirFunc`; typeck monólito por arquivo (esperado) |
| **LSP (DX.6)** | `arandu_lsp` + `tower-lsp`: diagnostics + `LineIndex`, goto-def, multi-open + `DocumentId`, `set_text` | Path quente **async**; `set_text` por keystroke; sem VFS/debounce; sem snapshot workers; sem cancel; stack errada para Salsa |
| **IDs (A10)** | A10.a densos (`SymbolId` file+local, TypeId, ExprId); A10.c `DocumentId` + `SlotMap` + testes stale/reopen | **Sem** `AnalysisRevision` / handles de IDE com revision; caches LSP não versionados; roadmap ainda confunde A10 “geracional” com middle |

### O que *não* fazer

| Tentação | Por quê é errado |
|----------|------------------|
| `generation` em todo `SymbolId` | Quebra estabilidade HashEq; stale-safety de análise é **revision de snapshot** |
| “Completar” tower-lsp com mais features | Arquitetura errada; reescreve duas vezes |
| Fake block-delta de typeck | Typeck ainda é por arquivo/função; delta honesto só em facts de bloco (e depois fatiar typeck) |
| Claim “superior a RA” | Critério = correto, cancelável, incremental; paridade honesta de features |

---

## 2. Arquitetura alvo

```
┌──────────────────────────────────────────────────────────────┐
│  Main thread (única escritora da verdade)                    │
│  • Vfs: pending edits + debounce (50–200 ms)                 │
│  • commit → SourceFile::set_text (1 revisão Salsa / batch)   │
│  • DocumentStore (DocumentId geracional)                     │
│  • DatabaseImpl (dono); writes só aqui                       │
└────────────────────────────┬─────────────────────────────────┘
                             │ snapshot = Clone barato (Storage Arc)
                             │ grava AnalysisRevision = current_revision
┌────────────────────────────▼─────────────────────────────────┐
│  Worker pool síncrono (sem tokio no path de query)           │
│  • type_check / goto / hover sobre clone congelado           │
│  • salsa::Cancelled / unwind_if_revision_cancelled           │
│  • resposta só se DocumentId ainda válido + revision match   │
└────────────────────────────┬─────────────────────────────────┘
                             │
┌────────────────────────────▼─────────────────────────────────┐
│  arandu_query (único dono #[salsa::tracked])                 │
│  parse → resolve → signatures → type_check → lower_amir      │
│  func_amir → liveness_facts → block_dataflow_facts           │
│  diagnostics Accumulator; (F4) delta por bloco dirty         │
└────────────────────────────┬─────────────────────────────────┘
                             │
   resolve / typeck / mir: lógica PURA; fronteira só recebe db
   lexer / parser / base / backends: NUNCA salsa-aware
```

### Três identidades (nunca misturar)

| ID | Camada | Geracional? | Função |
|----|--------|-------------|--------|
| `DocumentId` (`slotmap::SlotMap`) | Sessão LSP | **Sim** | Buffer aberto; close → stale; jobs validam `get(id)` |
| `FileId` + `SymbolId` / `TypeId` / `ExprId` | Compilação na revisão atual | **Não** (densos A10.a) | Identidade na análise corrente |
| `AnalysisRevision` (`u64` / `salsa::Revision`) | Entre edits | Sim (revisão) | Handles de IDE não atravessam edit sem revalidar |

**A10 gold** = A10.a denso + A10.c DocumentId + política de **AnalysisRevision** em handles expostos ao IDE.  
**Não** é `GenerationalId` genérico no middle para símbolos.

### Salsa 0.27 — padrão concreto (sem reinventar)

| Mecanismo | Uso no Arandu |
|-----------|----------------|
| `DatabaseImpl: Clone` (Storage Arc + contador de clones) | Snapshot O(1) para workers |
| Write (`set_text` / `zalsa_mut`) | Bloqueia até clones droparem → cancel implícito |
| `salsa::current_revision(db)` | Carimbar `AnalysisRevision` no snapshot |
| `db.unwind_if_revision_cancelled()` | Hot loops longos (typeck/mir) |
| `Cancelled` (panic sentinel) | Worker captura e descarta resposta |
| `HashEq` + blake3 | Early cutoff de valores grandes |
| `Durability` (futuro) | stdlib HIGH / user LOW se/quando inputs separarem |

---

## 3. Fases e status

### F0 — Inventário e invariantes — **DONE**

| Item | Status |
|------|--------|
| `docs/arandu-salsa-lsp-architecture-v0.1.md` | Existe |
| Testes `architecture_invariants` (sem `fs::read` em typeck/resolve; sem `CompileSession`) | Existe |
| Lista legado / D7 | Documentado |

**Manutenção:** atualizar tabela de status neste doc + architecture a cada fase.

---

### F1 — Fechar Salsa (compilador) — **quase DONE; remanescente fino**

#### Já fechado

- [x] `CompileSession` removido + guardrail
- [x] `symbol_span` a partir de `resolve` + `source_file_by_id`
- [x] Queries core reais + multi-file
- [x] `liveness_facts` → `analyze_local_liveness`
- [x] `block_dataflow_facts` → contagens live-in/out por bloco

#### Remanescente F1 (não bloquear F2/F3; encaixar quando tocar dataflow)

| ID | Trabalho | DoD |
|----|----------|-----|
| **F1.r1** | Extrair de `definite_init` / `move_checker` funções puras de **resumo** por bloco (counts/flags), sem second engine | `block_dataflow_facts` inclui campos estáveis (ex. `maybe_uninit_count`, `moved_live_count`) **ou** documentar conscientemente que o shape A1 gold = liveness-only até F4 |
| **F1.r2** | `func_amir`: preferir índice/`Arc` se clone de `AmirFunc` pesar (medir antes) | Sem regressão de HashEq; teste cutoff |
| **F1.r3** | `symbol_span`: lookup safe se `get` puder panic em id inválido | Span zero ou Option; nunca panic em path LSP |
| **F1.r4** | Documentar contrato: I/O de fonte **só** em `DatabaseImpl::resolve_module_path` + registro CLI/LSP | Architecture atualizado |

**Decisão recomendada (gold pragmático):**  
F1.r1 **não** bloqueia F2/F3. Liveness real já desbloqueia delta de facts de bloco. Init/move por bloco entram quando F4 precisar de chaves mais ricas — extrair bitsets de checkers existentes, não reimplementar.

**DoD F1 “fechado para LSP gold”:** F0 + core queries + liveness + zero CompileSession + span real. **✓ hoje**

---

### F2 — AnalysisRevision / snapshot handles — **PRÓXIMA**

**Objetivo:** stale-safety de análise sem poluir `SymbolId`.

#### 2.1 Tipos na fronteira `arandu_query` (ou módulo `analysis_host`)

```rust
/// Carimbo de revisão Salsa no momento do snapshot (IDE-facing).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct AnalysisRevision(u64);

#[derive(Clone)]
pub struct AnalysisSnapshot {
    pub revision: AnalysisRevision,
    pub db: DatabaseImpl, // clone barato; só leitura no worker
}

/// Handle de símbolo válido só nesta revisão (goto/hover cache).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct LspSymbolId {
    pub symbol: arandu_middle::SymbolId,
    pub revision: AnalysisRevision,
}
```

- `AnalysisSnapshot::capture(db: &DatabaseImpl) -> Self`  
  - `revision = salsa::current_revision(db)` (mapear para `u64` estável)  
  - `db: db.clone()`
- Métodos de query no snapshot (`type_check`, `goto`, …) **não** mutam inputs.

#### 2.2 Política de uso (LSP e testes)

1. Main: após commit VFS, workers recebem **só** `AnalysisSnapshot` + `DocumentId`.  
2. Antes de publish: `docs.get(document_id).is_some()` **e** (se cache) `handle.revision == snap.revision`.  
3. Após edit: handles antigos → `None` / cancel; **nunca** reinterpretar `SymbolId` sozinho como entidade viva.  
4. Caches (`LineIndex`, last diags) indexados por `(DocumentId, text_revision | AnalysisRevision)`.

#### 2.3 Testes obrigatórios

| Teste | Assert |
|-------|--------|
| `stale_revision_after_set_text` | `LspSymbolId` com revision antiga não é aceito como “mesmo” handle pós-edit |
| `document_close_invalidates_jobs` | `DocumentId` fechado → `get` None (já existe parcial em `doc_store`) |
| `snapshot_clone_shares_memo` | clone + query não reexecuta sem mudança de input (counters / RebuildLog) |

#### 2.4 O que **não** entra em F2

- Reescrever middle com arenas geracionais  
- StableHandle (A10.b morto)  
- HopSlotMap (deprecated); manter `SlotMap`

**DoD F2:** tipos + testes; API estável para F3 consumir.  
**Estimativa:** 2–4 dias.

---

### F3 — LSP gold (substituir MVP) — **após F2 mínimo**

#### 3.1 Stack de protocolo

| Escolha | Motivo |
|---------|--------|
| `lsp-server` + `lsp-types` | Transporte síncrono; padrão RA |
| Remover `tower-lsp` / tokio do path de query | Cancel Salsa é síncrono; async esconde races |

`tokio` pode sumir do crate ou ficar só se algum I/O de workspace precisar; **não** no hot path de analysis.

#### 3.2 Layout de módulos (alvo)

```
crates/arandu_lsp/src/
  main.rs          // loop lsp-server
  state.rs         // ServerState: db, vfs, docs, pool
  vfs.rs           // pending edits + debounce + commit
  handlers.rs      // initialize, didOpen/Change/Close, goto, …
  jobs.rs          // spawn snapshot work; cancel/discard
  conv.rs          // spans (já existe — portar)
```

#### 3.3 Concorrência

```rust
struct ServerState {
    db: DatabaseImpl,
    vfs: Vfs,
    docs: DocumentStore,
    by_uri: FxHashMap<String, DocumentId>,
    by_file_id: FxHashMap<u32, DocumentId>,
    pool: ThreadPool, // rayon ou pool fixo std
}

// Request handler (main):
let snap = AnalysisSnapshot::capture(&state.db);
let doc_id = …;
pool.spawn(move || {
    let result = std::panic::catch_unwind(|| {
        // queries em snap.db
    });
    match result {
        Ok(Ok(payload)) => respond_if_still_valid(doc_id, snap.revision, payload),
        Ok(Err(_)) | Err(_) => { /* Cancelled or panic → discard */ }
    }
});
```

**Regra de ouro:** writes (`set_text`, register file) **somente** na main, após merge VFS. Workers nunca chamam `Setter`.

#### 3.4 VFS + debounce

```
didChange → Vfs.push(file, change)
         → timer 50–200 ms (main)
         → commit: apply → set_text uma vez por arquivo dirty
didSave / request explícito → flush imediato
didOpen → register SourceFile + DocumentStore::open
didClose → DocumentStore::close (id stale)
```

- Full-document change LSP = um replace (suficiente no v0).  
- `ropey` só se partial edits + apply local se tornarem hotspot.  
- **Não** commitar revisão Salsa a cada keystroke.

#### 3.5 Capabilities (paridade com MVP + endurecimento)

| Feature | Comportamento gold |
|---------|-------------------|
| Diagnostics | Após commit VFS; `LineIndex`; publish só se DocumentId vivo |
| Goto-def | Snapshot + `LspSymbolId` / revision; multi-file via `file_id` |
| Hover (mínimo) | Opcional se barato; senão pós-paridade diag/goto |
| Multi-file | Workspace folders → register paths; imports via `resolve_module_path` |
| DocumentId | Toda job assíncrona valida id |

#### 3.6 Cancelamento

- Edit na main avança revisão → workers em clones antigos cancelam.  
- Loops caros: `unwind_if_revision_cancelled` em typeck/mir se medido.  
- **Nunca** Cranelift/C emit no LSP sem query tracked + cancel.

#### 3.7 Testes F3

| Teste | Assert |
|-------|--------|
| `debounce_batches_set_text` | N didChanges em janela → ≤1 commit/revision bump por arquivo |
| `goto_def_span_roundtrip` | fixture → range correto (portar conv tests) |
| `cancel_discards_stale` | edit durante type_check → resposta velha não publicada como nova |
| `closed_doc_no_publish` | close mid-job → sem diagnostics publish |

**DoD F3:** bin `arandu-lsp` main+workers+VFS; sem tower-lsp no path de query; paridade diag+goto; testes acima.  
**Estimativa:** 5–10 dias.

**Ordem interna F3 (DAG mini):**  
`deps (lsp-server)` → `vfs + debounce` → `state + AnalysisSnapshot` → `handlers parity` → `drop tower-lsp` → testes.

---

### F4 — Diferencial: diagnostics on-type por bloco — **opcional / após F1.r1 + F3**

1. Chaves de cache: `(DocumentId, AnalysisRevision, FuncSym, BlockId)` ou hash de `block_dataflow_facts`.  
2. Após edit: republicar só blocos/funções cujo memo Salsa mudou (DX.5 / counters).  
3. Honestidade: typeck **inteiro** ainda reexecuta por arquivo se dependências mudarem; o ganho real começa em facts de bloco e, mais tarde, fatiar typeck.

**DoD:** fixture grande + `-Zexplain-rebuild` / contadores: edit isolado não reexecuta facts de blocos independentes.  
**Estimativa:** 5+ dias após F3 estável.

---

### F5 — Hardening e docs

- [ ] Atualizar roadmap: A1/DX.6/A10 com sub-itens “gold done” (sem marketing falso)  
- [ ] Architecture: status CompileSession/symbol_span/dataflow alinhado à realidade  
- [ ] D7 explícito: reparse arquivo, sem Rowan  
- [ ] CLI `-Zexplain-rebuild` permanece  
- [ ] Remover código morto tower-lsp  
- [ ] Comentário `doc_store`: SlotMap (não HopSlotMap)

**Estimativa:** 1–2 dias.

---

## 4. DAG de PRs

```
F0 ✅
F1 core ✅ ── F1.r* (fino, paralelo / quando F4 pedir)
       │
       ▼
      F2 AnalysisRevision + snapshot API + testes
       │
       ▼
      F3.1 lsp-server + Vfs + debounce
       │
       ▼
      F3.2 workers + cancel + handlers (diag, goto)
       │
       ▼
      F3.3 drop tower-lsp + testes de paridade
       │
       ├──────────────► F4 block diag delta (opcional)
       ▼
      F5 docs / roadmap / cleanup
```

**Não** fazer F3 antes de F2 mínimo (senão handles sem revision).  
**Não** fazer F4 antes de F3 estável + facts de bloco (senão fake).

---

## 5. Libs

| Necessidade | Lib | Status |
|-------------|-----|--------|
| Incremental | `salsa` 0.27 | já |
| Doc handles | `slotmap` | já |
| Protocolo LSP | `lsp-server` + `lsp-types` | migrar |
| Edits | full text + debounce; `ropey` opcional | — |
| Hash maps | `rustc-hash` | já |
| Thread pool | `rayon` ou pool `std` fixo | a adicionar |

---

## 6. Riscos e mitigações

| Risco | Mitigação |
|-------|-----------|
| Modelo de clone Salsa 0.27 ≠ docs antigos “ParallelDatabase” | Spike: provar clone + write bloqueia + Cancelled; documentar no architecture |
| Migração tower-lsp | Checklist de paridade (diag, goto, multi-file) antes de dropar |
| `resolve_module_path` com `fs::read` na main | OK na DB; workers não devem registrar arquivos novos — só main |
| Scope creep Display / stdlib | Fora deste plano |
| NTFS / locks no monorepo | Evitar mass-edit em `lib.rs` sob hang; testes focados por crate |
| `func_amir` `.expect` | Path LSP só com SymbolId de resolve válido; ou Option |

---

## 7. Definition of Done global

1. **Salsa:** um dono (`arandu_query`); crates puras sem Salsa no miolo; sem CompileSession; I/O fonte centralizado; liveness/dataflow de bloco não-placeholder (liveness ✓).  
2. **LSP:** main síncrona + snapshot workers + VFS debounce + `lsp-server`; DocumentId em todo job; spans corretos; goto-def; multi-file; cancel descarta stale.  
3. **IDs:** A10.a denso + A10.c DocumentId + `AnalysisRevision` em handles de IDE; sem SymbolId geracional desnecessário.  
4. **Testes:** cancel, debounce, stale document, stale revision, multi-file import, architecture_invariants.  
5. **D7:** reparse de arquivo nomeado e aceito; sem promessa de CST.

---

## 8. Estimativa residual

| Fase | Esforço | Status |
|------|---------|--------|
| F0 | — | ✅ |
| F1 core | — | ✅ |
| F1.r* | 1–3 dias | opcional / paralelo |
| F2 | 2–4 dias | **próxima** |
| F3 | 5–10 dias | bloqueada por F2 mínimo |
| F4 | 5+ dias | opcional |
| F5 | 1–2 dias | final |
| **Gold sem F4** | **~1.5–3 semanas** foco | |
| **Com F4** | **~3–5 semanas** | |

---

## 9. Resumo executivo

| Camada | Alvo gold |
|--------|-----------|
| Compilador | Salsa fechado: queries reais, legado morto, liveness de bloco memoizada |
| IDs | DocumentId geracional + densos no middle + **AnalysisRevision** no IDE |
| LSP | Substituir tower-lsp por **main + VFS debounce + snapshot workers + lsp-server** |

O MVP atual é **ponto de partida a substituir** (F2→F3), não a “completar com features em cima do async”.

**Próximo passo de implementação:** F2 (`AnalysisSnapshot` / `AnalysisRevision` / `LspSymbolId` + testes), depois F3.1 VFS.
