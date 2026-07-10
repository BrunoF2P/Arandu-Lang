# Arandu Compiler Architecture — Master Roadmap (v0.1 → v0.4)

**Fonte única de verdade (checklist executivo).**
Este documento consolida as decisões arquiteturais sobre Data-Oriented Design (Interning), Polimorfismo Híbrido, OSSA (Ownership SSA), Effects, Async Colorless, Arquitetura de Memória e Binários de Pegada Zero em uma especificação técnica unificada e acionável.

| Documento | Status |
|-----------|--------|
| `arandu-strategic-plan-v0.1.md` | **Síntese** — decisões, bugs, papers, fases |
| `arandu-ir-architecture-v0.1.md` | Referência técnica de IR |
| `arandu-amir-v0.1.md` | Contrato AMIR + invariantes formais |
| `README.md` Next Steps | **Alinhado** — aponta para memória/backend como próximos marcos |

---

## 📊 Painel de Progresso

Legenda: `[x]` feito · `[/]` em andamento · `[ ]` não iniciado

### Pipeline de Compilação

| Passo | Estado | Notas |
|-------|--------|-------|
| Lexer | `[x]` | Recovery, spans, semicolon insertion |
| Parser → AST | `[x]` | AST estruturada, `self`, `Result<T,E>` canônico |
| Name resolver | `[x]` | N001–N006 |
| Type checker | `[x]` | HM Bidirecional, `TypeId` interner |
| AHIR | `[x]` | Golden `tests/hir/` |
| AMIR CFG | `[x]` | Dominadores, SSA registers vs stack slots |
| Definite init | `[x]` | lattices, InitBits flow, O008 diagnostic |
| Move checker | `[x]` | OSSA intraprocedural, O001/O005/O007, spans reais |
| Middle-end opt | `[x]` | Constant folding intra-bloco + DCE denso |
| Backend Cranelift | `[x]` | v0.2 Dev/Debug |
| Backend C | `[x]` | Portabilidade fallback |
| Backend LLVM | `[ ]` | v0.4+ Release Optimizer |

### Marcos de Entrega

```
Fase 1 — Estabilização Semântica (v0.1) · [CONCLUÍDA]
[x] B      Result<T,E> + Option<T> no type checker
[x] C      self receiver (own | mut | shared)
[x] T      Generics instantiations + Constraints (Go-style)
[x] G      Definite initialization (O008)
[x] F1     Instruções OSSA no AMIR (StorageLive/StorageDead/Destroy/Borrow/Move)
[x] M1     Move checker básico (O001, O005, O007) com spans reais (BUG-01)
[x] O1     Constant folding + DCE com bitset denso O(1) (SCALE-02)

Fase 2 — A Construção da Infraestrutura & Execução (v0.2) · [EM ANDAMENTO]
[x] INF2.1 Refatoração para InternPool (AstPool & TypeInterner centralizado)
[x] HIR Pool-first migration — structural HIR nodes (blocks, stmts, expr-blocks) stored in `HirPool` and referenced via `HirBlockId` (lowering, monomorphize, pretty-print, AMIR lowering and tests updated)
[x] A5     Layout de Dados Orientado a Objetos/Registros (SoA, dense ID-based graphs no AMIR/CFG)
[x] A6     CPU-Oriented Execution Model (table-driven parsing, branchless categorization)
[x] A7     Portable SIMD Infrastructure (AVX2/NEON UTF-8 validation e keyword matching)
[x] A8     Parallel Task Scheduler (work-stealing DAG, thread-local arenas, affinity)
[x] A9     Dense Bitset Infrastructure (dataflow, liveness, OSSA/DCE bits)
[x] A10    Stable ID Infrastructure
   ├─ [x] A10.a  IDs inteiros estáveis (ExprId, TypeId, SymbolId — NonZeroU32 + IndexVec) — em uso em todo o compilador
   ├─ [x] A10.b  StableHandle por hash estrutural — removido (código morto, nunca integrado)
   ├─ [x] A10.c  Generational IDs — `DocumentStore` + `slotmap::SlotMap` (`DocumentId`);
   │              close/reopen invalida handles antigos (testes em `doc_store`)
   └─ [x] A10.d  AnalysisRevision / LspSymbolId — stale-safety de análise por revisão de snapshot
                  (não generation em SymbolId); ver `arandu_query::analysis`
[x] A11    Token & String Storage Engine (packed tokens, SSO via smol_str, string interning)
[x] BC     Backend Cranelift (Dev/Debug com compilador em memória)
   ├─ [x] BC.1   Fat Pointer String JIT (tratar String como ptr + len na convenção de chamadas do Cranelift)
   ├─ [x] BC.2   Implementar EnumPayload & Discriminant no Cranelift JIT (Garantia estática contra double-free depende de M2; atualmente mitigado via poison-check em debug)
   ├─ [x] BC.3   Implementar IndexAccess & Array/Tuple no Cranelift JIT (Garantia estática contra double-free depende de M2; atualmente mitigado via poison-check em debug)
   ├─ [ ] BC.4a  Borrow/BorrowMut no Cranelift JIT
   │              · heap/`ptr`: implementável sem F2 (ponteiro já materializado)
   │              · stack local `&`/`&mut`: depende de F2.0–F2.3
   ├─ [ ] BC.4b  Await no Cranelift JIT (depende inteiramente de A3 — independente de F2)
   └─ [x] FUZZ   Fuzzing Lexer/Parser SIMD (arandu_fuzz e cron jobs semanais de robustez)
[x] C_FB   Backend C de portabilidade e bootstrapping
[x] DX     Diagnostics & Tooling Infrastructure (DX1-DX3, DX4 CFG visualization; DX2 recovery anchors completed)
[x] PERF   Compiler Instrumentation & Observabilidade (pass timers, allocations, query logs, -Z flags, tracing-based self-profile)
    ├─ [x] PERF.1   Tracing subscriber + -Zdebug-* flags via EnvFilter (replaces time_pass!/debug_point!)
    ├─ [x] PERF.2   SelfProfile Layer — Trace Event JSON buffer em memória, finalize_self_profile()
    ├─ [x] PERF.3   #[instrument] em 22 funções críticas (parser, unify, typeck, resolve, etc.)
    └─ [x] PERF.4   ParseCache (legado CompileSession) — absorvido pela query Salsa `parse`
[x] SL_C   Stdlib Fundamental: arandu_core e arandu_alloc (primitivas heapless e arena/smallvec/bitset)
[x] DOC1   docs/ossa-virtual-anchoring.md — RFC retroativo documentando a técnica de âncoras virtuais + poda

Fase 3 — OSSA Avançado, Semântica e OS Runtime (v0.3) · [NÃO INICIADA]
[x] A1     Query System (Incremental Semantic Database, Salsa-like O(1) invalidation)
   ├─ [x] A1.1   Salsa Integration / CompilerDatabase migration (`CompileSession` removido)
   ├─ [x] A1.2   O(1) FileId lookup em DatabaseImpl (índice reverso FileId→SourceFile via FxHashMap)
   ├─ [x] A1.3   Dataflow por bloco — `liveness_facts` / `block_dataflow_facts` (live+init+move)
   ├─ [x] A1.4   F4 IDE diags — `file_ide_diagnostics` / `func_analysis_diags` + early cutoff
   ├─ [x] DX.5   Causal-Chain explain-rebuild — `RebuildLog` + Salsa `Event` callback;
   │              CLI `-Zexplain-rebuild`; testes `explain_rebuild`
   └─ [x] DX.6   LSP gold — `arandu_lsp` com `lsp-server` + main síncrona + VFS debounce +
                 snapshot workers (`AnalysisHost`/`AnalysisSnapshot`); diagnostics + goto-def +
                 multi-file; `DocumentId` geracional; stale revision descarta jobs
[x] PERF.5  Arc nos campos pesados de TypeCheckResult (pré-requisito para DX.6)
   │  Feito: symbols / resolved / type_info atrás de `Arc`; diagnostics por valor.
   │  Clone de `TypeCheckResult` é O(1) atomic refcount; `type_info_mut()` /
   │  `Arc::make_mut` no lower HIR quando o interner precisa mutar.
   │  `check_bodies_only` usa `Arc::unwrap_or_clone` ao reentrar no checker.
[ ] A2     Effect System (pure, readonly, noalloc, nothrow, nosuspend)
[ ] A3     Modelo Async Semântico Colorless (coroutine splitting, zero heap stack-first, OSSA checks)
[ ] A4     Memory Layout Optimization Engine (field reordering, niche tags, SOO)
[ ] F2     OSSA borrow completo (borrow_shared, borrow_mut, end_borrow)
   ├─ [ ] F2.0   Sintaxe de referências à pilha (& / &mut) no parser + type-checker
   ├─ [ ] F2.1   Local Borrow Checking Incremental (Salsa query-level borrow check por bloco de controle de fluxo)
   ├─ [ ] F2.2   Janelas de Liveness de Empréstimos (reutilizar liveness SSA de referências como regiões NLL do CFG)
   └─ [ ] F2.3   Análise de Escape e Fallback Geracional (Vale-style generational refs para stack-locals que escapam)
[ ] M2     Move checker avançado (O002, O003, O006)
           └─ Dependência: fecha a garantia estática de double-free que hoje é mitigada apenas por poison-check (0xDE) em debug (ver BC.2/BC.3)
[ ] G2     Generational fallback opcional + O004 (escape analysis)
[ ] T2     DX Enhancements: Default Generic Parameters & Scoped Enum Variant Sugar
   ├─ [ ] T2.1   Default Generic Parameters (e.g. struct Vec<T, A = GlobalAllocator> in parser, resolved and typechecker instantiation)
   └─ [ ] T2.2   Implicit Enum Variant Dot-Notation Sugar (e.g. .Ok(val) using bidirectional expected-type inference in typechecker)
[ ] T3     DX: Import Sem Aspas para Módulos Internos (LSP-friendly path tokens)
   │
   │  Motivação: `import "std.core.mem" as mem` usa uma string opaca que o LSP
   │  não consegue completar sem tratamento especial de "cursor dentro de string".
   │  Com `import std.core.mem as mem` o lexer emite tokens limpos (IDENT + DOT)
   │  que o LSP completa com o mesmo mecanismo de qualquer expressão pontuada.
   │  Strings com aspas continuam válidas para paths de filesystem com caracteres
   │  inválidos como identificadores (barras, hifens, domínios).
   │
   ├─ [ ] T3.1   AST: Novo variant ImportDecl::ModuleAlias
   │             Arquivo: crates/arandu_parser/src/ast/decl.rs
   │             Adicionar variant ao enum ImportDecl:
   │               ModuleAlias {
   │                   span: Span,
   │                   path: Vec<String>,   // ex: ["std", "core", "mem"]
   │                   alias: String,       // ex: "mem"
   │               }
   │             Atualizar impl ImportDecl::span() para cobrir o novo variant.
   │             Atualizar todos os match exaustivos em dump/decl.rs, collect.rs e mod.rs.
   │
   ├─ [ ] T3.2   Parser: Reconhecer `import <path> as <alias>` sem aspas
   │             Arquivo: crates/arandu_parser/src/parser/decl.rs (fn parse_import)
   │             Lógica atual: após parse_module_path(), não testa `as`.
   │             Mudança: no branch de ImportDecl::Module, após parse_module_path(),
   │             verificar se o próximo token é KW_AS:
   │               let path = self.parse_module_path()?;
   │               if self.eat_name("KW_AS") {
   │                   let alias = self.expect_import_name()?;
   │                   // → ImportDecl::ModuleAlias { path, alias }
   │               } else {
   │                   // → ImportDecl::Module { path }  (comportamento atual)
   │               }
   │             Nenhuma ambiguidade sintática: KW_AS nunca é um segmento de módulo.
   │             expect_optional_semicolon_after_module_path() continua igual.
   │
   ├─ [ ] T3.3   Resolvedor: Tratar ModuleAlias igual ao External
   │             Arquivo: crates/arandu_resolve/src/name_resolution/collect.rs
   │             Em collect_import(), adicionar braço para ModuleAlias:
   │               ImportDecl::ModuleAlias { path, alias, span } => {
   │                   if let Some(sym) = self.define(scope, alias, SymbolKind::Module, *span) {
   │                       self.record_import_symbol(sym, alias.clone(), *span);
   │                   }
   │                   let source = path.join(".");
   │                   self.import_aliases.insert(alias.clone(), source);
   │               }
   │             Arquivo: crates/arandu_resolve/src/name_resolution/mod.rs
   │             Em load_stdlib_transitively() e load_stdlib_signatures(),
   │             adicionar match arm para ImportDecl::ModuleAlias { path, .. }:
   │               let source = path.join(".");  // "std.core.mem"
   │               // reutilizar a mesma lógica de strip_prefix de External
   │
   ├─ [ ] T3.4   Migrar stdlib para a nova sintaxe sem aspas
   │             Arquivos: stdlib/core/*.aru, stdlib/alloc/*.aru
   │             Antes:  import "std.core.mem" as mem
   │             Depois: import std.core.mem as mem
   │             Busca/substitui. O resolvedor continuará funcionando pois
   │             ModuleAlias produz a mesma chave `import_aliases` que External.
   │
   ├─ [ ] T3.5   Testes de contrato parser
   │             Arquivo: crates/arandu_parser/tests/parser_contract.rs
   │             Adicionar: import_module_alias_no_quotes — verifica que
   │             `import std.core.mem as mem` produz ImportDecl::ModuleAlias
   │             com path=["std","core","mem"] e alias="mem".
   │             test_all_stdlib_files_parse_cleanly() já cobre a stdlib migrada.
   │
   └─ [ ] T3.6   Impacto no LSP futuro (documentar no RFC, não implementar agora)
                 Com tokens limpos, o servidor LSP pode:
                 - Completar segmentos de caminho (std. → core, alloc)
                 - Completar módulos internos do projeto pelo grafo de módulos
                 - Emitir diagnósticos de import inválido com span preciso (IDENT)
                   em vez de span que abrange a string inteira
                 - Reutilizar o mesmo mecanismo de "complete após DOT" que
                   já funcionará para acesso a campos e membros de módulos
[ ] SL_S   Stdlib de Sistema: arandu_std (io, fs, process, env, path, time, random, sync, thread, ffi)
[ ] SL_R   Async Runtime: arandu_std::runtime (scheduler cooperativo/work-stealing e reactor OS epoll/kqueue/io_uring)
[ ] SL_T   Testing Harness: arandu_std::testing (test runner integrado e benchmark engine)

Fase 4 — Expressividade de Linguagem e Tipagem (v0.35) · [NÃO INICIADA]
[ ] SYN.1  Retorno implícito na última expressão de bloco (parser: parse_block)
[ ] SYN.2  Interpolação de String ($name e ${expr}) no Lexer
[ ] SYN.3  Açúcar Sintático para Opcionais (T? mapeando para Option<T> e nil para .None)
[ ] SYN.4  Pattern Matching Avançado (wildcards, bindings e ranges)
[ ] TYP.1  Interfaces Implícitas / Structural Typing (Go-style duck typing no Type Checker)
[ ] TYP.2  Constraints de Generics (cláusula `where` e sintaxe `<T: Trait>`)

Fase 5 — Otimização Global, CodeGen & Ecossistema (v0.4+) · [NÃO INICIADA]
[ ] LLVM   Backend LLVM (Release Optimizer, LTO, PGO profile-guided optimization pipeline)
[ ] REG    Register Allocation (Linear Scan para Cranelift, Graph Coloring para LLVM)
[ ] GEN    Adaptive Monomorphization (Witness tables para cold paths vs Lazy Monomorphization para loops)
[ ] ABI    ABI & Layout Stability (repr(C) garantido, fat pointers, stable calling conventions)
[ ] PAN    Panic & Error Model sem unwinding (abort nativo UD2/BRK, zero metadata overhead)
[ ] CACHE  Stable Serialization & Cache (.air, .amir, .ameta, reproducible DET builds)
* Mover json e xml para arandu_ext::serialization
[ ] EXT    Ecosystem Extensions: arandu_ext (ecs, game loop, renderer, audio, media, physics, gui)

Fase 6 — Bootstrap & Auto-Hospedagem (v1.0) · [NÃO INICIADA]
[ ] HOST   Self-Hosting: compilador Arandu compilando a si mesmo de forma convergente (3-passos)
[ ] BOOT   Remoção total de dependências do compilador Rust para build releases
[ ] MS     Completa compilação paralela usando o runtime nativo de concorrência com compilação < 3 segundos
```

---

## 🏛️ Os 6 Invariantes Arquiteturais do Arandu

Para evitar o inchaço de binários do Rust e o overhead de runtimes pesados tradicionais, o compilador do Arandu assume seis premissas fixas de design:

1. **Dataflow-First (Semantics-First)**: O compilador não tenta encaixar otimizações ou regras de memória após a geração de código. A linguagem converte a semântica em fatos propagáveis sobre um Grafo de Fluxo de Controle (CFG) estrito.
2. **InternPool Centralizado (ID-Based)**: Proibido o uso de estruturas recursivas baseadas em ponteiros (`Box`, `Rc`, `Vec<Box<Node>>`) nas IRs intermediárias. Toda a árvore sintática e de tipos é armazenada em arrays contíguos na memória e referenciada por IDs compactos de 32 bits (`NodeId`, `TypeId`, `LiteralId`).
3. **Polimorfismo Híbrido Adaptativo**: Rejeita witness tables como default absoluto (evitando o gargalo de inlining do Swift) e recusa monomorfização total por padrão (evitando a explosão de tamanho de binário do Rust). O compilador decide de forma adaptativa.
4. **Ownership no OSSA (Ownership Semantic SSA)**: O gerenciamento de memória não é um validador de tipos na AST. Ele vive no AMIR através de instruções explícitas de fluxo de posse: `move`, `copy`, `borrow_shared`, `borrow_mut`, e `destroy`.
5. **Zero-Metadata Runtime**: Abort imediato via instruções nativas do processador (`UD2`/`BRK`) elimina a necessidade de tabelas gigantescas de stack unwinding (`.eh_frame`) e strings de pânico embutidas no binário.
6. **Identidade Única sob Incrementalidade**: A engine incremental (Salsa) nunca introduz um sistema de identidade paralelo ao já existente no compilador. Toda query usa como chave os IDs nativos do Arandu (`FileId`, `SymbolId`, `BlockId`, `TypeId`) diretamente, atuando apenas como camada de memoização.
---

## 🏛️ Linguagem & Runtime Semantics

Abstrações que prejudiquem a otimização e a análise estática são expressamente proibidas no core da linguagem.

### Estilo de Código e Nomenclatura

Constantes globais usam `SCREAMING_SNAKE_CASE` para diferenciação explícita de fluxo estático:

* `MAX_INLINE_SIZE`
* `DEFAULT_STACK_SIZE`

### Closures e Scoped Blocks

Closures utilizam a sintaxe de parenthesized closures para clareza visual e simplicidade de análise:

```arandu
thread::scope (scope) {
    scope.spawn {
        compile_file(job)
    }
}
```

**Motivação:**

* Parser simples e previsível;
* Sem necessidade de introduzir tokens especiais complexos;
* Reduz ruído visual na leitura de código;
* Favorece a legibilidade de fluxos concorrentes/estruturados;
* Evita indireções sintáticas excessivas.

### Async/Await

A sintaxe canônica e oficial na linguagem é de prefixo:

```arandu
user = await fetch_user(id)
```

A forma sufixada:

```arandu
fetch_user(id).await
```

é aceita estritamente como açúcar sintático opcional. O formatter oficial converte automaticamente qualquer uso de sufixo para a forma prefixada.

**Motivação:**

* Linearização visual clara do fluxo de controle;
* Melhor leitura do grafo de dataflow;
* Lowering mais intuitivo para o CFG/AMIR;
* Reduz o encadeamento (chaining) visual excessivo que esconde pontos de suspensão.

---

## 🛠️ O Novo Pipeline de Dados do Arandu

```text
Source (.aru)
    ↓
  Lexer           → Error recovery + String Interning
    ↓
  Parser          → AST estruturada em Pools Lineares (NodeId)
    ↓
  Name Resolver   → Symbol Table Hierárquica O(1) via IDs
    ↓
  Type Checker    → Bidirecional HM + TypeInterner (TypeId)
    ↓
  AHIR            → Typed AST + Preservação de Interfaces
    ↓
  AMIR (CFG/SSA)  → Construção do Grafo + SSA Locals
    ↓
  OSSA Engine     → Definite Init (Lattices) + Move Checker + Escape Analysis
    ↓
  Middle-End Opt  → Constant Folding + Tree-Shaking DCE na AMIR
    ↓
  Backend Selector
       ⚡ Dev/Debug   → Cranelift (Compilação Instantânea em Memória)
       🚀 Release     → LLVM IR (Otimização Extrema)
       🔌 Portability → C Puro (Fallback)
```

---

## 🚀 Fases e Detalhamento de Subsistemas

### Fase A — Compiler Infrastructure & Core Subsystems (v0.2)

Antes de expandir as capacidades de otimização, o compilador do Arandu constrói sua fundação infraestrutural. A Fase A cubre tanto a semântica e efeitos da linguagem (A1–A4) quanto a **arquitetura de execução** do compilador em si (A5–A11).

#### A1 — Query System (Incremental Semantic Database via Salsa)

Inspirado por Salsa e o request-evaluator do Swift, o compilador é estruturado como um banco de dados de consultas (queries) puras e memoizadas:

* **ParseCache (precursor, Fase 2)**: `HashMap<PathBuf, &Program>` mantido em `CompileSession` evita re-parsing de arquivos stdlib entre resolução de nomes e type-check. É o primeiro passo de memoização no pipeline e será absorvido pelo Salsa database como uma query `parse(path) -> Program`.
* **Grafo de Dependências Estáticas**: Rastreia de forma fina quais consultas dependem de quais arquivos fonte.
* **Compilação Incremental O(1)**: Alterações em um método em uma classe só invalidam as queries daquele bloco de código específico, mantendo feedback de compilação abaixo de 50ms.
* **Queries Determinísticas**: Garante que compilações repetidas com os mesmos inputs gerem binários idênticos byte a byte.

**Roteiro de migração para Salsa:**
1. `ParseCache` → query `parse(path: Path) -> Program` no Salsa database
2. `CompileSession` → `salsa::Database` com todos os recursos (type_interner, symbol_table, etc.)
3. Queries de name resolution e type-check → queries Salsa com dependências finas entre arquivos
4. Cancelamento automático de queries obsoletas em edições LSP

#### A2 — Effect System (v0.3)

Um sistema de efeitos estrito e rastreável pelo compilador que decora as assinaturas de funções e garante propriedades semânticas:

* `pure`: Garante ausência de efeitos colaterais e mutações globais. Permite otimização agressiva de GVN (Global Value Numbering) e eliminação total de sub-chamadas redundantes.
* `readonly`: Permite ler dados arbitrários mas proíbe qualquer mutação. O compilador usa isso para promover borrows mutáveis em compartilhados de forma segura.
* `noalloc`: Proíbe alocações na heap. Ideal para kernels, drivers e hot-paths de alta performance.
* `nothrow`: Garante que a função nunca pânico/abort, eliminando caminhos de erro nas análises de controle de fluxo do AMIR.
* `nosuspend`: Garante que a função é síncrona e nunca suspende controle, permitindo chamadas diretas sem overhead de corrotinas.

#### A3 — Modelo Async Semântico e Colorless (v0.3)

O Arandu resolve o "Color Problem" das linguagens modernas (onde funções síncronas e assíncronas não se misturam facilmente) através de uma semântica flexível e de baixo nível no compilador:

* **Sintaxe Colorless Adaptativa**: O parser aceita tanto a notação de prefixo quanto de sufixo:

  ```arandu
  user = await fetch_user(10)  // Prefixo
  user = fetch_user(10).await  // Sufixo
  ```

  O formatter oficial padroniza a escrita de forma uniforme, mas a flexibilidade é garantida nativamente.
* **Coroutine-Based Type System**: Para o sistema de tipos (`ArType`), o compilador possui a variante embutida `ArType::Coroutine(TypeId)` (representada como `Coroutine[T]`).
  * `async func f() -> T` é açúcar sintático idêntico a `func f() -> Coroutine[T]`, e o compilador infere os tipos de forma equivalente.
  * O interface `Future[T]` (com `poll` e `TaskContext`) existe apenas em `arandu_core` para bibliotecas de runtime, sendo implementado debaixo do capô pelo compilador para todas as `Coroutine[T]` geradas.
* **Colorless Async & @nosuspend**: É possível invocar uma corrotina diretamente em contexto síncrono. Se o compilador provar estaticamente ou dinamicamente que ela não suspende (ou se o desenvolvedor decorar com `@NoSuspend`), ela executa síncrona e imediatamente sem overhead de agendamento de tarefas.
* **Coroutining Lowering & State Machine**: Toda função `async` ou bloco `async { ... }` é quebrado em blocos básicos no CFG do AMIR contendo pontos de suspensão explícitos (`suspend` e `resume`). O compilador realiza o *coroutine splitting* transformando variáveis locais que atravessam suspension points em slots de uma struct de estado da tarefa.
* **Zero Heap Alloc por Padrão & OSSA-Aware Suspension**: As structs de estado das corrotinas utilizam *stack-first allocation* na pilha do chamador por padrão. A alocação na heap só ocorre se a tarefa escapar do escopo corrente (via escape analysis).
* **Pin-free Self-References via OSSA Indices**: O Arandu elimina a necessidade de `Pin` para corrotinas auto-referenciais. Toda variável que atravessa um suspension point é guardada na struct da corrotina. Para evitar ponteiros auto-referenciais diretos na RAM (que quebrariam se a corrotina fosse movida de posição), o compilador converte as referências internas em índices locais (`LocalId(u32)`). A struct pode assim ser movida livremente da Stack para a Heap sem quebrar ponteiros. A análise de ownership OSSA (Ownership and State Stack Allocation) valida e rastreia ownership através dos suspension points, proibindo moves parciais e borrows ativos incompatíveis que atravessem um `await`.

#### A4 — Memory Layout Optimization Engine

Um subsistema dedicado a rearranjar dados na pilha e na memória física para garantir máxima eficiência de cache e pegada zero:

* **Struct Field Reordering**: Organiza campos de structs automaticamente para eliminar padding de alinhamento desnecessário, minimizando o consumo de cache L1.
* **Niche Optimization (Option/Enum Packing)**: Enums como `Option<T>` e `Result<T, E>` aproveitam valores inválidos do tipo base (como ponteiros nulos ou patterns de bits inválidos) para codificar tags, mantendo a representação de `Option<&T>` no mesmo tamanho de um ponteiro cru.
* **Pointer Tagging**: Codifica metadados ou tags de variantes de enums nos bits menos significativos não utilizados de ponteiros alinhados de 64 bits.
* **Small Object Optimization (SOO)**: Evita alocações para structs ou vetores pequenos armazenando seus dados diretamente inline dentro do próprio container se o tamanho for menor ou igual a 24 bytes.

---

### Fase A (cont.) — Execution Architecture (A5–A11)

Os subsistemas A5–A11 definem **como o compilador em si executa**: como os dados fluem pela CPU, como evitar stalls de pipeline, como paralelismo escala e como cada traversal acontece. Isso é o que separa um compilador acadêmico de um compilador industrial.

#### A5 — Data-Oriented Layout Engine

O compilador do Arandu prioriza layouts contíguos e traversal linear de memória para minimizar cache misses e pointer chasing.

**Status v0.2:** implementado no AMIR/CFG. Instruções AMIR agora vivem em uma tabela densa por função (`AmirStmtTable`) com IDs compactos (`InstrId`) e blocos básicos guardam apenas ranges contíguos (`DenseRange`). Traversals de CFG usam `BlockId` e RPO explícito; A6-A8 permanecem responsáveis por parsing table-driven, SIMD e scheduler.

* **Struct-of-Arrays (SoA)**: Subsistemas de alta densidade computacional (dataflow, liveness, SSA analysis, dominators, register allocation) utilizam layouts SoA em vez de árvores orientadas a objetos. Exemplo: em vez de `Vec<Instruction>` onde cada `Instruction` contém opcode, operands e span intercalados, o compilador armazena `opcodes: Vec<Opcode>`, `operands: Vec<OperandPair>`, `spans: Vec<Span>` como arrays paralelos. Isso permite que um pass que só precisa de opcodes itere apenas sobre a memória dos opcodes, sem poluir cache com operands e spans.
* **Dense ID-Based Graphs**: O AMIR evita ponteiros crus entre instruções e blocos básicos. Relações são representadas por IDs compactos (`InstrId(u32)`, `BlockId(u32)`, `TempId(u32)`) indexando tabelas densas contíguas. Comparação, cópia e hashing de qualquer entidade são O(1) por inteiro.
* **Pointer Compression**: Estruturas persistentes evitam ponteiros de 64 bits sempre que possível, utilizando offsets compactos de 32 bits dentro de arenas contíguas. Isso dobra a densidade efetiva de cache L1 para grafos e árvores.
* **Traversal Linearization**: Passes do middle-end reorganizam blocos básicos e instruções em ordem de Reverse Post-Order (RPO) para maximizar prefetching automático da CPU e locality durante análises iterativas de ponto fixo.

#### A6 — CPU-Oriented Execution Model

O Arandu trata o compilador como um pipeline intensivo em cache locality e previsibilidade microarquitetural. O design do frontend e middle-end prioriza:

* Redução de cache misses (L1d/L1i/L2);
* Redução de branch misprediction;
* Redução de pointer chasing;
* Maximização de linear traversal;
* Maximização de prefetching automático da CPU.

**Estratégias adotadas:**

| Técnica | Onde Aplicada | Impacto |
|---------|--------------|--------|
| Table-driven parsing e dispatch | Lexer, Parser | Elimina cascatas de `if-else`/`match` longas, converte decisões em lookups indexados por tabela O(1) |
| Branchless token classification | Lexer | Classifica categorias de caracteres (alpha, digit, whitespace, operator) via aritmética de inteiros sem branches condicionais |
| Dense bitsets para dataflow | Move checker, Definite Init, Liveness | Operações vetoriais de set (`union`, `intersect`, `diff`) em palavras de 64 bits, processando 64 locais por instrução CPU |
| Intrusive structures no IR | AMIR instructions | Metadados de encadeamento (`prev`/`next`) vivem dentro do próprio nó, eliminando alocações separadas de lista |
| Compact CFG ordering | AMIR blocks | Blocos em RPO sequencial garantem que travessias de dataflow iterem linearmente na memória |
| Flat hash tables com probing linear | Symbol tables, Type interner | Robin Hood / Swiss Tables minimizam probes e maximizam cache locality em lookups de alta frequência |
| Hot/Cold path separation | Todo o compilador | Caminhos raros de erro, recovery e diagnósticos são separados fisicamente dos hot paths principais, reduzindo pressão sobre I-cache e melhorando branch prediction |
| Arena recycling | Optimization passes | Páginas físicas de arenas transientes são reutilizadas entre passes sem devolver ao SO, evitando TLB churn e page faults |

#### A7 — Portable SIMD Infrastructure

O frontend textual do Arandu (lexer, UTF validation, keyword matching) suporta aceleração vetorial opcional baseada em capacidades da CPU.

**Backends SIMD suportados:**

| Backend | Plataforma | Largura | Uso Principal |
|---------|-----------|---------|---------------|
| Scalar fallback | Universal | 1 byte | Baseline garantido em qualquer CPU |
| SSE2 | x86_64 baseline | 16 bytes | UTF-8 validation, whitespace skip |
| AVX2 | x86_64 moderno | 32 bytes | Scanning léxico de alta vazão, keyword matching |
| NEON | ARM64 (Apple Silicon, mobile) | 16 bytes | Paridade com SSE2 em ARM |

**Runtime Dispatch:** O compilador detecta as capacidades da CPU em runtime (`cpuid` no x86, `/proc/cpuinfo` ou feature registers no ARM) e seleciona automaticamente o backend vetorial mais capaz disponível.

**Objetivos mensuráveis:**

* Reduzir branch pressure no lexer em ~4x comparado com classificação escalar;
* Acelerar scanning textual para ~2 GB/s em AVX2;
* Validar UTF-8 em blocos de 32 bytes por instrução;
* Manter fallback escalar com performance aceitável (~400 MB/s).

#### A8 — Parallel Task Scheduler

O compilador utiliza um scheduler baseado em DAG de tarefas independentes para escalar linearmente com núcleos físicos.

**Modelo de execução:**

```text
                    ┌──────────┐
                    │  Source  │
                    │  Files   │
                    └────┬─────┘
                         │
              ┌──────────┼──────────┐
              ▼          ▼          ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │ Parse  │ │ Parse  │ │ Parse  │   ← Thread-local arenas
         │ file_a │ │ file_b │ │ file_c │
         └───┬────┘ └───┬────┘ └───┬────┘
             │          │          │
             ▼          ▼          ▼
         ┌──────────────────────────────┐
         │    Merge Symbol Tables       │   ← Lock-free union
         └──────────────┬───────────────┘
              ┌─────────┼─────────┐
              ▼         ▼         ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │Typecheck│ │Typecheck│ │Typecheck│  ← Per-worker allocators
         │ mod_a  │ │ mod_b  │ │ mod_c  │
         └───┬────┘ └───┬────┘ └───┬────┘
             │          │          │
              ▼         ▼         ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │Codegen │ │Codegen │ │Codegen │   ← Per-core NUMA arenas
         │ mod_a  │ │ mod_b  │ │ mod_c  │
         └────────┘ └────────┘ └────────┘
```

**Estratégias:**

* **Work-stealing queues**: Threads ociosas roubam pacotes de compilação de outras threads sem sincronização pesada;
* **Thread-local arenas**: Cada worker opera sobre sua própria arena, eliminando mutex e false sharing;
* **Lock-free scheduling**: O DAG de dependências é resolvido com contadores atômicos — quando todas as dependências de uma tarefa completam, ela é enfileirada automaticamente;
* **Affinity-aware worker assignment**: Workers são fixados em cores físicos (CPU affinity / pinning) para evitar migração de threads e maximizar reuso de cache L1/L2.

#### A9 — Dense Bitset Infrastructure

As análises de fluxo de dados (dataflow), tempo de vida (liveness), dominadores, checagem de empréstimo (borrow checker), eliminação de código morto (DCE) e detecção de invalidade de consultas do compilador dependem de um motor de manipulação de bits de altíssima performance:

**Status v0.2:** implementado como infraestrutura densa compartilhada. `BitSet<T>` e `BitMatrix<R,C>` usam `Vec<u64>` e IDs densos; definite-init, move-state tracking, liveness local, reachability de CFG, dominance frontiers e DCE já usam a base densa. O escopo de **Borrow Tracking** em A9 é somente representacional: os conjuntos densos necessários para rastrear regiões/locais vivos ficam disponíveis para OSSA, mas as regras semânticas completas de conflito entre `borrow_shared`, `borrow_mut` e `end_borrow` continuam no marco **F2 — OSSA borrow completo**.

* **Representação Vetorial**: Estados são armazenados em `Vec<u64>` contíguos de memória, garantindo acesso linear e tirando proveito do prefetching automático da CPU;
* **Throughput Microarquitetural**: Operações lógicas fundamentais (`union`, `intersect`, `diff`) operam em palavras de 64 bits, processando até 64 elementos de dados por ciclo de instrução;
* **Cache Locality**: Consome apenas ~128 bytes para rastrear 1024 locais, comparado aos ~8 KB exigidos por `HashSet<T>` baseados em ponteiros;
* **Vetorização Automática**: Compilações release tiram proveito de instruções AVX2/NEON para processar bitsets de 256 bits em uma única operação lógica de hardware.

**Utilização planejada:**

| Análise | Bits por Local | Operações Dominantes |
|---------|---------------|---------------------|
| Definite Initialization | 1 bit | `union`, `intersect`, bit test |
| Liveness Analysis | 1 bit | `union`, `diff`, bit test |
| Move State Tracking | 2 bits (Available/Moved/MaybeMoved) | `join`, bit test |
| Dominance Frontiers | 1 bit por bloco | `union`, membership |
| Borrow Tracking | 1 bit por região | Infraestrutura representacional; regras semânticas completas em F2 |
| DCE Reachability | 1 bit por instrução | `union`, sweep |

**Estratégia de Pipeline Cache-Aware acoplada:**

* **Compact CFG ordering**: Blocos básicos são renumerados em Reverse Post-Order (RPO) após construção e após cada transformação significativa para garantir que as travessias de dataflow baseadas em bitset acessem memória sequencialmente.
* **Reverse Post-Order traversal**: Todas as análises de ponto fixo iteram os blocos em RPO, garantindo convergência de liveness/init mais rápida.
* **Dataflow batching**: Múltiplas análises independentes são mescladas no mesmo traversal para maximizar reuso de cache L1.
* **Arena recycling**: Chunks das arenas temporárias são resetados instantaneamente ao término do pass (bump pointer para 0) sem retornar páginas ao OS, eliminando page faults.

#### A10 — Stable ID Infrastructure

O compilador do Arandu evita expressamente ponteiros brutos e referências cruzadas que inviabilizariam a compilação incremental e criariam pointer chasing complexo. Toda a infraestrutura do compilador (AST, HIR, AMIR, caches e queries) baseia-se em IDs Estáveis:

* **IDs inteiros estáveis** (`NonZeroU32` em `ExprId`, `TypeId`, `SymbolId`, `FileId`, etc.) indexando `IndexVec`s contíguos — **implementado e em uso em todo o compilador**.
* **Generational IDs** (`Index (u32)` + `Generation (u32)`) para detecção de referências stale no LSP — **não implementado ainda**. A implementação manual (`stable_id.rs`) foi removida por ser código morto. Quando o LSP (DX.6) for iniciado, adotar a crate `slotmap` que resolve isso com API idiomática e zero `unsafe`.
* **Stable Handles**: removidos (código morto, nunca integrados ao compilador). A identidade estável entre sessões de compilação é provida pelos IDs inteiros determinísticos do Salsa.
* **Zero Overhead de Serialização**: Como os IDs não dependem do endereço de memória virtual, salvar e restaurar caches de compilação do disco é um dump contíguo e direto de bytes.

#### A11 — Token & String Storage Engine

O frontend textual evita alocações individuais de tokens e strings, tratando o fluxo léxico como um problema de throughput de dados:

| Componente | Técnica | Benefício |
|-----------|---------|----------|
| **Token Buffer** | Packed contiguous arrays (`Vec<Token>` onde `Token` é 12 bytes: `kind: u32` + `start: u32` + `len: u32`) | Zero alocação individual, locality perfeita |
| **String Interning** | Pool global de strings deduplicado via `HashMap<&str, StringId>` | Comparação de identificadores vira comparação de inteiros O(1) |
| **UTF-8 Validation** | Validação vetorizada (SIMD quando disponível, via A7) durante o scan do lexer | Custo amortizado: validação integrada ao scanning, sem segunda passada |
| **Small-String Optimization (SSO)** | Identificadores ≤ 23 bytes armazenados inline sem alocação heap | ~95% dos identificadores reais cabem inline |
| **Buffer Reuse** | Buffers temporários de diagnósticos e formatação são arenas scratch reutilizadas | Zero pressão sobre o alocador global |

---

## 🧠 Arquitetura de Memória & Modelo de Alocação (Memory-First)

O Arandu assume oficialmente a diretriz **"Memory Architecture First"**. A performance e escalabilidade de um compilador dependem da redução de pointer chasing, cache misses, fragmentação e contention de threads. Portanto, o pipeline é desenhado com estratégias de memória e alocadores sob medida para cada etapa.

### 1. Base do Compilador: Arenas de Scratch por Passe

> **Nota de implementação (2026-07):** A implementação manual de `VmReservation` (`mmap`/`VirtualAlloc`) e `BumpArena` foi removida do codebase por ser código morto — nenhuma fase do compilador a utilizava. Os dois arquivos continham bugs de segurança (integer overflow em bounds check e granularidade errada de commit no Windows). A estratégia de memória adotada é:

* **Alocador padrão do sistema** para todas as estruturas persistentes (AST pools, tabelas de símbolos, AMIR). O design SoA com `IndexVec` já garante localidade de cache L1 sem precisar de arena customizada.
* **`bumpalo`** (crate portável, segura, zero `unsafe` exposta, suporta WASM) como arena de scratch nos passes de otimização onde dados temporários são alocados e descartados em massa: monomorphization graph, scratch buffers do move checker, grafos temporários de CFG. **[ ] A implementar — próxima etapa da Fase 3.**
* **NUMA Awareness** e **thread-local arenas por worker** permanecem como objetivo de longo prazo para quando o scheduler paralelo por arquivo (A8) for expandido além do estado atual.

---

### 2. Frontend Allocation Model

O modelo de memória no frontend é focado em alta densidade de dados e eliminação de alocações na heap global:

| Subsistema / Fase | Estratégia de Alocação | Descrição Técnica & Vantagens |
|-------------------|-------------------------|-------------------------------|
| **Lexer** | Stack Buffers + Temp Arenas | Tokens não são alocados individualmente. São emitidos linearmente em packed buffers contíguos (`SmallVector<Token>` ou buffers estáticos reusáveis). Strings temporárias e diagnósticos rápidos utilizam alocações scratch curtas. |
| **Parser** | Hierarchical Growable Arena | Nós da AST são alocados consecutivamente em chunks de arenas. Como a árvore sintática vive até o lowering/análise semântica e morre junta, toda a arena é descartada em O(1) ao fim do ciclo de parsing. |
| **AST / AHIR** | VM-Backed Bump Arenas | A árvore sintática utiliza *Pointer Compression*. Em vez de ponteiros brutos de 64 bits (`Node*`), os nós referenciam uns aos outros através de offsets numéricos compactos de 32 bits (`NodeId`), dobrando a densidade de cache L1. |
| **Type System** | Interned Canonical Types | Proibido estruturação de tipos redundantes. Todo tipo inferido ou resolvido é canônico e registrado no `TypeInterner`. O compilador manipula apenas `TypeId(u32)`, reduzindo comparações estruturais profundas a comparações simples de inteiros. |
| **Semantic Analysis** | Temp Arenas + Scoped Rollback | A inferência e overload resolution geram milhões de fatos intermediários. Escopos de funções usam *Temporary Arenas* com checkpoints. Ao sair do escopo semântico, o bump-pointer retrocede (rollback) instantaneamente. |
| **Symbol Tables** | Stable IDs + Dense Hash Storage | Símbolos não carregam ponteiros diretos que seriam invalidados por compilação incremental. São armazenados em Slot Maps compactos indexados por `SymbolId` e mapeados por tabelas Hash densas com Robin Hood Hashing / Swiss Tables. |

---

### 3. Middle-End Allocation Model

O middle-end trabalha com modificação e transformação frequente de código, exigindo alocação dinâmica mas controlada:

| Subsistema / Fase | Estratégia de Alocação | Descrição Técnica & Vantagens |
|-------------------|-------------------------|-------------------------------|
| **AMIR (SSA / CFG)** | Arena + Slab Allocator | A representação intermediária é mutável por natureza (otimizações apagam e recriam instruções). Instruções e operandos utilizam um alocador Slab integrado a *Free Lists*, reciclando slots mortos para evitar vazamentos de memória na arena. |
| **CFG Graph** | Contiguous Dense Storage | Blocos básicos (`AmirBasicBlock`) e arestas de dominadores são armazenados em vetores densos e contíguos (`IndexVec`). Isso garante buscas lineares rápidas e cache locality excelente durante travessias de análise de fluxo de dados. |
| **SSA Nodes** | Intrusive Linked Structures | Fluxo de instruções e dependências SSA utilizam estruturas encadeadas intrusivas (onde os metadados de links vivem dentro do próprio objeto instrução). Isso elimina alocações e indireções extras em listas padrão como `std::list`. |
| **Optimization Passes** | Scratch Arenas | Passes como Dominator Analysis e Liveness Analysis reservam uma Transient Arena dedicada. Todo grafo temporário de arestas e conjuntos liveness morre e é resetado instantaneamente ao término do pass. |
| **DCE & Dataflow** | Arena Recycling | Análises iterativas que exigem recriação frequente de mapas utilizam reciclagem de páginas da arena. A memória física associada nunca é devolvida ao SO entre passes, evitando overhead de alocação de página e TLB misses. |

---

### 4. Parallel Compilation Model

O suporte a compilação paralela maciça exige isolamento de memória absoluto para evitar contenção de travas globais:

| Subsistema / Fase | Estratégia de Alocação | Descrição Técnica & Vantagens |
|-------------------|-------------------------|-------------------------------|
| **Parsing** | Thread-Local Arenas | Cada thread de parsing lê arquivos fonte independentes e aloca sua AST em uma arena exclusiva. Zero lock contention global e ausência total de falsos compartilhamentos (false sharing) de linhas de cache. |
| **Typechecking** | Per-Worker Allocators | Trabalhadores semânticos resolvem classes e métodos em paralelo. Cada um opera sobre sua própria arena temporária e interner local, unificando os símbolos no pool principal de forma controlada apenas ao término da fase. |
| **Codegen** | Per-Core Arenas | A geração de código de máquina final subdivide os módulos por núcleos físicos (cores). As estruturas geradoras e buffers de escrita de binários operam em arenas NUMA-aware alinhadas à CPU física local. |
| **Job System** | Lock-Free Queues | O agendamento de tarefas do compilador utiliza filas sem travas (lock-free rings) com algoritmos de *Work-Stealing*. Threads ociosas roubam pacotes de compilação de outras threads sem forçar sincronização pesada no kernel. |

---

### 5. Incremental & IDE/LSP Model

Ambientes de longa execução como IDEs e Language Servers (LSPs) exigem persistência de dados históricos sem fragmentação de memória:

| Subsistema / Fase | Estratégia de Alocação | Descrição Técnica & Vantagens |
|-------------------|-------------------------|-------------------------------|
| **Syntax Trees** | Persistent Immutable Trees | Em vez de destruir a AST a cada digitação do usuário, o compilador IDE utiliza estruturas de dados persistentes e imutáveis (Green Trees / Ropes). Células inalteradas da árvore sintática são compartilhadas estritamente por referência. |
| **Handles** | Generational IDs | Entidades e tipos persistentes no banco semântico são referenciados por `GenerationalId` (ID composto de `index` + `generation`). Evita referências dangling e detecta instantaneamente dados invalidados por edições de código. |
| **Queries** | Salsa-like Dependency Graph | Todo o estado semântico do compilador IDE é modelado como queries memoizadas em um grafo de dependências estáticas. Apenas queries cujos arquivos de entrada sofreram alterações diretas ou indiretas são recomputadas. |
| **Snapshots** | Copy-On-Write (COW) | Mapeamentos virtuais de arquivos e registros semânticos de builds antigos coexistem com a versão ativa usando proteção de página Copy-On-Write do sistema operacional, duplicando dados físicos somente quando editados. |

---

### Fase 3 — Otimização Baseada em Fatos Semânticos (v0.3)

#### 3.1 Polimorfismo Híbrido Adaptativo (Adaptive Monomorphization)

O compilador do Arandu rejeita abordagens extremas e escolhe a estratégia ótima baseada no local de uso:

* **Witness Tables (Caminhos Frios & Fronteiras)**: Por padrão, genéricos geram uma única implementação compartilhada que opera sobre ponteiros opacos e recebe uma tabela de metadados (`ValueWitnessTable`). Isso reduz drasticamente o tamanho do binário e acelera o tempo de compilação.
* **Lazy Monomorphization (Caminhos Quentes)**: O compilador realiza a monomorfização cirúrgica (duplicação e especialização de código concreto) para:
  * Loops e hot-paths identificados por PGO ou análise estática.
  * Tipos primitivos numéricos e tipos pequenos de dados.
  * Funções explicitamente anotadas com `@specialize`.
  * Candidatos ideais para inlining de performance.

#### 3.2 Otimizações Avançadas na AMIR

* **DCE Agressivo por Alcançabilidade (Tree-Shaking)**: A partir do ponto de entrada `main`, varre o grafo de chamadas estático do AMIR. Qualquer código da stdlib ou bibliotecas que não possua arestas ativas é eliminado.
* **Stack Promotion via Escape Analysis**: O compilador rastreia a posse de objetos alocados. Se a posse não escapar do bloco de ativação local da função, a alocação que iria para a Heap é promovida para um slot contíguo na Stack física.

---

### Fase 4 — Geração e Execução Multitarget (v0.4+)

#### 4.1 Pipeline de Duplo Backend

O compilador do Arandu abandona o acoplamento exclusivo a um único backend:

* **arandu build --dev**: Utiliza o backend **Cranelift** gerando código de máquina diretamente em memória de forma quase instantânea para testes e iteração rápida.
* **arandu build --release**: Utiliza o backend **LLVM** aplicando vetorização avançada, PGO (Profile-Guided Optimization) e LTO (Link-Time Optimization) para desempenho máximo de produção.
* **arandu build --portability**: Utiliza o backend **C** puro para transpilar o código linearizado 1:1, servindo estritamente como fallback para plataformas de nicho, embarcados de arquiteturas exóticas e bootstrapping.

#### 4.2 Register Allocation Strategy

A alocação de registradores é o ponto onde a qualidade do código gerado vive ou morre. O Arandu adota estratégias diferentes por backend:

| Backend | Algoritmo | Prioridade | Descrição |
|---------|-----------|------------|----------|
| **Cranelift (Dev)** | Linear Scan | Velocidade de compilação | Alocação em tempo linear sobre intervalos de vida, minimizando latência de compilação para ciclos edit-compile-run < 100ms |
| **LLVM (Release)** | Graph Coloring global | Qualidade do código | Alocação baseada em interferência com coalescing agressivo, minimizando spills e maximizando reuso de registradores físicos |
| **C (Portability)** | Delegado ao compilador C host | Portabilidade | O backend C emite variáveis locais e confia no GCC/Clang para alocação |

**Objetivos mensuráveis:**

* Dev builds: zero spills para funções com ≤ 12 variáveis live simultaneamente;
* Release builds: spill pressure ≤ 5% para hot loops identificados por PGO;
* Locality de registradores: priorizar reuso do mesmo registrador físico para variáveis com lifetimes não-sobrepostos.

#### 4.3 Controlled Generational Fallback

Onde a análise de tempo de vida estática do OSSA falhar em garantir a liberação automática sem overhead, o Arandu insere tags geracionais dinâmicas. Contudo, essa inserção é restrita e controlada pelo desenvolvedor:

* **Bloqueio Explícito**: O desenvolvedor pode proibir qualquer fallback de heap geracional ou alocação dinâmica anotando o escopo com `@no_fallback` ou passando a flag global `--no-generational-fallback`.
* **Diagnósticos Informativos**: O compilador emite a nota informativa **O004** detalhando onde e por que o fallback dinâmico foi inserido, fornecendo hints claros de como refatorar o código para se manter stack-first.

---

### Fase DX — Diagnostics & Tooling Infrastructure

#### DX1 — Rich Diagnostics Engine

O compilador do Arandu implementa um sistema moderno de diagnósticos inspirado nas melhores práticas visuais do Rust, Swift e Clang.

##### Recursos

* **Multi-span diagnostics**: Aponta múltiplos locais no código envolvidos no mesmo erro semântico;
* **Labels encadeadas**: Inline annotations explicativas no próprio trecho de código fonte;
* **Fix-it hints**: Sugestões automáticas de correção sintática e semântica;
* **Notes hierárquicas**: Explicações conceituais acopladas aos códigos de erro;
* **Rendering colorido**: Terminal output rico com cores e indicadores de coluna;
* **Mensagens estruturadas**: Representação interna unificada para fácil serialização.

##### Exemplo de Output Técnico

```text
error[O002]: cannot move borrowed value
  --> src/main.aru:5:10
   |
 3 | x = &y;
   |         -- value borrowed here
 4 |
 5 | z = y;
   |         ^ move occurs here
   |
note: borrow later used here on line 7
```

#### DX2 — Recovery Architecture

O parser e as análises semânticas são estruturados para resiliência a falhas, permitindo o máximo de utilidade em IDEs e Language Servers:

* **Error Nodes na AST**: Em vez de parar na primeira falha, construções sintáticas inválidas produzem nós de erro específicos sem interromper o parsing do restante do arquivo;
* **Synchronization Points**: O parser avança até delimitadores de escopo conhecidos (como `;` ou `}`) para sincronizar e continuar a análise;
* **Partial AST Continuation**: Análises de tipo operam sobre árvores sintáticas parcialmente inválidas;
* **Speculative Recovery**: Correções heurísticas simples de digitação ou tokens faltantes são assumidas temporariamente para continuar capturando erros subsequentes.

**Objetivo:** IDE responsiveness ultra rápida, exibindo múltiplos erros em uma única compilação e mantendo o LSP resiliente a código inacabado.

#### DX3 — Structured Diagnostics

Todos os diagnósticos emitidos pelo compilador possuem formato serializável nativo (JSON), permitindo integrações ricas com ferramentas externas de CI/CD e suporte a LSPs de forma consistente.

#### DX4 — CFG & IR Visualization

O compilador inclui suporte nativo para emissão visual de fluxo de controle (CFG) e estruturas de IR intermediárias em formato Dot/Graphviz. Permite que o desenvolvedor depure caminhos de OSSA, liveness, dominância e transformações de otimização de forma imediata e visual.

#### IDE — Native LSP Engine

Uma camada unificada de Language Server (LSP) nativa no compilador que expõe consultas eficientes de autocompletar, goto definition, busca de referências e diagnósticos inline. Ao compartilhar o mesmo banco de dados Salsa e as Persistent Green Trees de parser, a engine LSP responde a alterações de código em menos de 5ms de forma incremental.

---

### Fase PERF — Compiler Instrumentation & Profiling

Para garantir que o compilador do Arandu permaneça sub-segundo à medida que o projeto escala, ele possui infraestrutura nativa de observabilidade interna e profiling.

#### PERF1 — Pass Timing

Medição em nanossegundos de cada passagem lógica do compilador, incluindo Lexer, Parser, Type Checker, lowering de AMIR, OSSA/Move Checker e otimizações de Backend.

#### PERF2 — Allocation Tracking

Mapeamento preciso de recursos de memória consumidos:

* Monitoramento de páginas das Arenas e Slabs transientes;
* Taxa de reciclagem de slots nas Free Lists de instruções;
* Identificação de hotspots de alocação;
* Rastreamento de cache pressure e desperdício de padding em estruturas de IR.

#### PERF3 — Query Profiling

Instrumentação fina do motor incremental (Salsa-like):

* Grafo de invalidação de queries ativo em tempo real;
* Custo de rebuild incremental por query;
* Memoization hit rate de análises de tipo e resolução de escopos.
* **ParseCache hit-rate** no self-profile: quantos `parse_with_file_id` foram evitados pelo cache (`-Zself-profile` mostra 6 em vez de 11 chamadas para um build single-file típico).

#### PERF4 — Debug Flags (-Z)

Flags internas ativadas em compilações debug/nightly para inspeção microarquitetural e dumping de IRs:

* `-Ztime-passes`: Exibe tempo detalhado gasto em cada pass de otimização;
* `-Zdump-amir`: Imprime a representação AMIR SSA por função;
* `-Zdump-ossa`: Dump do grafo de liveness e estados de moves rastreados pelo OSSA;
* `-Zprofile-queries`: Exibe hit rate e custos do cache de queries incrementais;
* `-Zdump-cfg`: Emite grafos de fluxo de controle dos blocos básicos em formato `.dot`.

---

### Geração de Código de Máquina & Perfilamento (Fase 4)

#### PGO — Profile-Guided Optimization Pipeline

O compilador implementa suporte a otimizações guiadas por perfilamento (PGO). O desenvolvedor compila um binário de instrumentação que coleta métricas de execução reais em caminhos quentes (hot paths). Na compilação final:

* O compilador prioriza a monomorfização agressiva de genéricos e o inlining de funções apenas em hot loops com profiling documentado;
* Branches condicionais frios de erro são marcados para ordenação física distante na geração final do LLVM IR, maximizando o I-Cache de loops quentes.

---

## ⚙️ Runtime Philosophy

O runtime do Arandu segue uma diretriz de minimalismo e isolamento absoluto de dependências.

### Modelo Oficial

* **Corrotinas Stackless**: Suspensões do async/await geram splits de blocos básicos na AMIR e salvamento de estado em structs locais compactas (zero stack overhead de threads);
* **Lowering para State Machines**: O compilador gera código linear e determinístico para a transição de estados das tarefas;
* **Scheduler Cooperativo**: Tarefas cedem a CPU voluntariamente em pontos de suspensão explícitos (`await`), eliminando custos de preempção;
* **Work-Stealing Executor**: Roteia e distribui a carga de tarefas dinamicamente sobre um pool de threads de forma lock-free com NUMA awareness.

### Rejeição de Tracing Garbage Collectors (GC)

O Arandu exclui expressamente o uso de um Garbage Collector clássico (tracing, stop-the-world, mark-sweep ou compactador). A decisão de rejeitar GCs é sustentada por dez objeções técnicas principais:

1. **Perda de Previsibilidade**: GCs introduzem pausas (stop-the-world) e heurísticas de varredura temporais imprevisíveis, violando a diretriz de previsibilidade semântica;
2. **Destruição do Stack-First Design**: GCs incentivam a alocação irresponsável na heap. O Arandu promove a alocação na Stack via Escape Analysis;
3. **Piora de Locality (Pointer Chasing)**: A movimentação ou espalhamento de objetos por coletores aumenta a fragmentação de memória e deteriora a eficiência de cache L1/L2;
4. **Poluição de Hot Paths com Barriers**: GCs modernos exigem barreiras de escrita (write barriers) e leitura (read barriers) nas instruções da CPU, poluindo o fluxo e reduzindo o throughput de execução;
5. **Inchaço de Metadata**: Coletores de lixo exigem cabeçalhos de objetos gigantes, mark bits e metadados de RTTI implícitos, colidindo com o objetivo de "zero-metadata runtime";
6. **Ineficiência Multicore**: Concorrer a threads globais de marcação causa gargalos e contenção de locks de memória, anulando a escalabilidade NUMA e thread-local do Arandu;
7. **Enfraquecimento do OSSA**: O ownership-first faz do tempo de vida (lifetime) um fato explícito e determinístico. O GC esvazia o valor semântico de instruções `destroy` e `move`;
8. **Complexidade no Async**: GCs acoplam a alocação de frames assíncronos ao heap global. O Arandu realiza coroutine splitting stack-first;
9. **Footprint Gigante**: O suporte a runtime de tracing exige adicionar dezenas de megabytes ao binário final;
10. **Inviabilização de Bare-metal/no_std**: Runtimes com GC não rodam com eficiência e segurança em microcontroladores e sistemas embarcados com restrição severa de recursos.

A gerência de memória do Arandu baseia-se exclusivamente em **Semantics-driven memory**: `ownership + stack + arenas + escape analysis + controlled fallback`.

### Objetivos Principais

* Evitar stacks gigantescas de sistema por tarefa assíncrona;
* Proibir alocações de heap implícitas durante suspensões de rotinas;
* Garantir independência absoluta de garbage collectors globais;
* Manter o custo de runtime invisível em compilações embarcadas.

### Heap Allocation Policy

Nenhuma operação built-in da linguagem realiza alocação de heap implícita. Quando o compilador detecta escape semântico inevitável de um valor, ele exige um alocador explícito ou insere fallbacks controlados geracionais. Em caso de fallback automático, o compilador emitirá o diagnóstico informativo **O004**, com notas de rodapé de refatoração para stack-first.

---

## 🧱 ABI e Garantias de Layout

O Arandu define explicitamente suas regras de ABI para garantir robustez em FFI, builds incrementais e interoperabilidade limpa entre backends.

### Garantias de Layout do Compilador

* **Struct Layout Determinístico**: O reordenamento de campos para eliminação de padding segue um algoritmo canônico fixo. Caso o desenvolvedor precise de compatibilidade C pura, ele deve anotar a struct com `@repr(C)`;
* **Enum Tagging Estável**: Tags de enums com valores de dados acoplados (como `Result`) utilizam nichos de bits nulos ou tags de tamanho previsível;
* **Pointer Alignment & Calling Convention**: Alinhamento estrito baseado na plataforma de destino e passagens de parâmetros por registradores por padrão para Cranelift e LLVM.

### Representações Internas

Abstrações de tipos e referências dinâmicas usam witness tables compactas e ponteiros duplos (fat pointers) explícitos contendo ponteiro do objeto + ponteiro de metadados de interface.

### Async ABI

Os frames e estados das tarefas assíncronas gerados pelo compilador têm tamanho e layout resolvidos em tempo de compilação, permitindo que a OSSA rastreie ownership e liveness dos empréstimos através dos suspension points com segurança.

---

## 🚨 Modelo de Falhas e Tratamento de Erros

O Arandu adota uma filosofia pragmática dividida entre erros recuperáveis e falhas fatais não recuperáveis.

### Recuperação de Erros (`Result<T, E>`)

Todo erro que pode ser contornado pelo chamador é retornado explicitamente via tipo monádico `Result`. O compilador otimiza caminhos de erro para manter overhead zero em caminhos quentes.

### Abort Imediato (Falhas Fatais)

Falhas que representam quebra de invariantes (como out-of-bounds ou asserções violadas) abortam a execução imediatamente. O Arandu **não realiza stack unwinding**.

* **Traps Nativas**: O runtime emite instruções de hardware como `ud2` (x86) ou `brk` (ARM) para encerrar o processo imediatamente;
* **Custo Zero**: Sem unwinding, elimina-se a necessidade de metadados `.eh_frame`, tabelas de exceção complexas e código de limpeza invisível no binário, simplificando o Grafo de Fluxo de Controle e reduzindo drasticamente o tamanho do executável.

---

## 🔥 Hot/Cold Path Separation

O compilador separa fisicamente seus caminhos quentes de processamento de dados dos caminhos frios (exibição de erros e logs).

* **Hot Paths**: Lexer, Parser, SSA traversal, análises CFG do OSSA, alocadores das arenas e passes de otimização de instruções. Esses blocos são mantidos compactos, lineares e em loops densos para maximizar o cache de instruções da CPU (I-cache) e reduzir branch mispredictions;
* **Cold Paths**: Geração de formatação visual de diagnósticos, pretty printers do AMIR para depuração, escrita de arquivos de dump metadata e logs de instrumentação. Todo esse código é compilado com atributos de "cold coldness", instruindo o linker a movê-los para seções de memória distantes.

---

## 💾 Stable Serialization & Incremental Cache

O compilador define formatos e extensões serializáveis estáveis para garantir persistência determinística e reutilização de cache entre builds incrementais ou compartilhados em rede:

* `.air`: Representação serializada compacta da AST em formato binário estável;
* `.amir`: Grafo serializado de instruções SSA do middle-end;
* `.ameta`: Metadados exportados de módulos com assinaturas de tipo e definições públicas;
* `.aobj`: Código objeto final gerado pelo backend de compilação.

### Garantias de Cache & Hashing Estável

* **Stable Hashing**: O compilador computa hashes estáveis baseadas no algoritmo criptográfico **BLAKE3** para cada arquivo fonte e query semântica intermediária. Isso permite invalidar e reconstruir o grafo de dependências incrementais Salsa de forma instantânea sem reprocessar blocos de código inalterados;
* **Versioned Schema**: Os metadados `.ameta` e as IRs intermediárias possuem cabeçalhos com esquemas binários versionados para prevenir erros ou conflitos de desserialização em atualizações de ferramentas do compilador.

### DET — Deterministic & Reproducible Builds

O Arandu garante a reprodutibilidade de compilação byte a byte (byte-by-byte binary convergence):

* **Deterministic Ordering**: Hashes e ordenação de queries incrementais no banco de dados utilizam ordenação lexicográfica estável nos identificadores de símbolos, garantindo que o compilador emita o mesmo binário final independentemente do paralelismo ou ordem de leitura dos arquivos no sistema multi-core;
* **Stable Compilation Outputs**: A ordenação determinística garante que builds distribuídos em CI/CD ou compilações remotas/compartilhadas produzam artefatos idênticos, acelerando o hit-rate em caches remotos.

---

## 📋 Tabela de Códigos de Diagnóstico (DiagCodes)

| Código | Categoria | Descrição Técnica e Gatilho Arquitetural |
|--------|-----------|-----------------------------------------|
| **N001** | Name Resolution | Identificador não declarado (com sugestão) |
| **N002** | Name Resolution | Redeclaração no mesmo escopo |
| **N003** | Name Resolution | Tipo usado erroneamente como valor |
| **N004** | Name Resolution | Valor usado erroneamente como tipo |
| **N005** | Name Resolution | Import não encontrado |
| **N006** | Name Resolution | Conflito de símbolos entre imports |
| **T011** | Type Checker | Generic constraint ou cláusula `where` inválida |
| **T019** | Warning | `Result<T,E>` ignorado na atribuição sem handling (`?`) |
| **T025** | Error | Interface não satisfeita (métodos faltantes no Go-style) |
| **P006** | Parser | Uso sintático inválido de tupla para retorno de erro |
| **O001** | Ownership | Uso de variável local após comando de move no CFG |
| **O002** | Ownership | Move/consume while borrowed (`O002MoveWhileBorrowed`) |
| **O003** | Ownership | Empréstimo mutável concorrendo com referências compartilhadas |
| **O004** | Info | Generational/escape fallback (G2/F2.3) — not shared-borrow conflict |
| **O005** | Ownership | Dupla liberação de memória (Double Free) |
| **O006** | Ownership | Destroy/free while borrow still active (`O006DestroyWhileBorrowed`) |
| **O007** | Ownership | Estado de move inconsistente entre branches no merge do CFG |
| **O008** | Ownership | Leitura ou cópia de slot local não inicializado |

---

## 12. Histórico de Revisões

| Data | Autor / Agente | Mudança Realizada |
|------|----------------|-------------------|
| 2026-05 | Antigravity | Roadmap v0.1 criado; invariantes e decisões canônicas |
| 2026-05 | Antigravity | **Grande Expansão v0.2**: Inclusão formal de Phase A (Salsa, Effects, Colorless Async, Memory Layout Engine), Hybrid Generics, Dual-Backend pipeline e Controlled Fallbacks. |
| 2026-05 | Antigravity | **Integração Memory-First**: Inclusão formal do subsistema detalhado de alocação de memória por estágio do compilador (Lexer, Parser, AST, SSA, CFG, Parallel, Incremental e IDE/LSP). |
| 2026-05 | Antigravity | **Execution Architecture (A5–A11)**: Inclusão formal dos subsistemas de Data-Oriented Layout (SoA, pointer compression), CPU-Oriented Execution Model (branchless, table-driven), Portable SIMD (SSE2/AVX2/NEON), Parallel Task Scheduler (work-stealing DAG), Cache-Aware Optimization Pipeline (RPO, arena recycling), Dense Bitset Engine, Token & String Storage Engine, Register Allocation Strategy e Hot/Cold Path Separation. |
| 2026-05 | Antigravity | **Semantics, DX & Tooling**: Inclusão formal das especificações de Semântica e Sintaxe da Linguagem (closures, async canônico), Filosofia de Runtime, ABI/Layout Stability, Abort/Panic Model, Fase DX (Rich Diagnostics Engine, Recovery, JSON output), Fase PERF (Compiler instrumentation), Hot/Cold separation e Stable Serialization. |
| 2026-07 | Antigravity | **Auditoria de Honestidade A10/A11/VM**: Removidos `vm.rs`, `arena.rs`, `stable_id.rs` e `string_pool.rs` (~1.080 LOC, 16 blocos `unsafe`) — código morto nunca integrado ao compilador. A10 corrigido para `[~]` parcial: IDs inteiros estáveis em uso, Generational IDs aguardam LSP (Fase 3) com `slotmap`. VM Reservation substituída por plano `bumpalo` para arenas de scratch nos passes de otimização. A11 permanece `[x]` via `smol_str`. |

---

*Mantenha este documento atualizado a cada avanço estratégico do compilador.*