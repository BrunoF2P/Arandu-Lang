# AGENTS.md — Compilador Arandu

## Propósito

Arandu é um compilador incremental em Rust. Preserve as fronteiras entre fases,
o determinismo dos resultados e o early-cutoff das queries. Uma simplificação
local que aumenta a superfície de invalidação, duplica parsing ou mistura
camadas é uma regressão arquitetural.

## Mapa do workspace

| Área | Responsabilidade |
| --- | --- |
| `arandu_base` | Utilitários e dados fundamentais. Mantenha-o deliberadamente leve; não adicione dependências sem justificativa arquitetural. |
| `arandu_lexer` / `arandu_parser` | Lexer e CST-first com Rowan; `syntax_tree(file)` é canônico e `parse(file)` apenas baixa CST para AST. |
| `arandu_diagnostics` | `DiagCode`, diagnósticos e documentação longa em `docs/errors/`. |
| `arandu_middle` | Contratos entre fases, HIR/AMIR, `SourceDatabase`, IDs e layout. |
| `arandu_resolve` / `arandu_typeck` / `arandu_mir` | Lógica pura de resolução, tipos, ownership/dataflow e AMIR. Não são donos de Salsa. |
| `arandu_query` | Único dono de Salsa: DB, inputs, queries tracked, `AnalysisHost` e reparse incremental. |
| `arandu_backend_cranelift` / `arandu_backend_c` | Backends. |
| `arandu_cli` / `arandu_lsp` | Orquestram a DB; LSP usa `lsp-server`, VFS e snapshots. |
| `arandu_fmt` | Formatter puro, sem Salsa/LSP. |
| `arandu_test_support` / `xtask` | Infraestrutura de testes e tarefas do workspace. |

## Invariantes de arquitetura — não violar sem discussão explícita

- Pipeline: CST (`syntax_tree`) → AST (`parse`) → `resolve` → `type_check` →
  `lower_amir` → backend. Não coloque resolução ou tipagem no parser, nem
  faça re-lex/parse paralelo a partir de texto quando o CST já for disponível.
- Apenas `arandu_query` conhece Salsa. `arandu_resolve`, `arandu_typeck`,
  `arandu_mir`, lexer, parser, base e backends devem permanecer puros.
- Queries tracked são puras, determinísticas e sem efeitos observáveis:
  proibidos `println!`, `eprintln!`, I/O, polling de FS, mutação global e
  telemetria com efeito colateral. Instrumente com `#[tracing::instrument]`.
- `exported_symbols` e `resolve` são separadas deliberadamente. Preserve essa
  divisão e as saídas hash-estáveis: uma edição no corpo de uma função não
  pode invalidar importadores quando a superfície exportada não mudou.
- `local_symbols`, `exported_symbols`, `item_source_input`, typeck por item e
  diagnósticos IDE por item existem para early-cutoff. Não os transforme em
  resultados monolíticos nem faça deep-clone de `Program`/`AmirProgram` no hot
  path; use `Arc::clone` ou `HashEq::share`.
- `resolve` e `type_check` nunca fazem `fs::read`. Registro e leitura de
  módulos pertencem à DB/CLI/LSP; listagens de diretório passam por inputs
  Salsa (`DirectoryListing`), jamais `fs::exists`/`read_dir` no hot path.
- `SymbolId` é composto por `{ file_id, local_id }`. Não o achate, hasheie de
  modo instável ou substitua por offset de texto. O alocador de `FileId` é
  monotônico: IDs não podem ser reutilizados após unregister.
- Imports cíclicos usam `ResolutionResult` e devem convergir com resultados e
  diagnósticos determinísticos. Ordem de `HashMap` não pode afetar saída.

## LSP, snapshots e identidades

- Há três identidades distintas: `DocumentId` geracional para buffers LSP,
  `FileId` para a análise atual e `AnalysisRevision` geracional para handles.
  `LspSymbolId` só resolve quando sua revisão coincide com a do snapshot.
- Nunca mantenha `AnalysisSnapshot` nem clone de `DatabaseImpl` na mesma thread
  durante `set_text`: Salsa aguarda clones serem descartados e pode deadlockar.
- Workers LSP só analisam snapshots; a thread principal registra arquivos e
  publica resultados apenas se `DocumentId` ainda estiver vivo e a revisão
  coincidir. Não comite Salsa a cada tecla: preserve debounce/save/goto.

## IR, ownership e backends

- Preserve SSA/OSSA: definições dominam usos, parâmetros de bloco correspondem
  a cada predecessor e argumentos de `Goto`/`Branch`/`Suspend` continuam
  alinhados com os parâmetros do destino.
- AMIR tem DCE mark-sweep, CFG simplification, jump threading e análises por
  worklist até fixpoint. Mudanças devem conservar efeitos observáveis, valores
  de retorno de todos os caminhos e usos de terminadores — inclusive argumentos
  de salto — e devem convergir.
- Ao criar variante de rvalue/terminador, atualize os visitors compartilhados;
  DCE, move checker, liveness e backends não podem divergir.
- Layout é dependente do alvo. Use `DataLayout`/`TargetInfo` e
  `TargetInfo.float_size`; nunca presuma `Float` = `f64`, tamanho de ponteiro,
  alinhamento ou ABI do host.
- Código inválido deve recuperar e emitir diagnóstico, não `panic!`, `unwrap`
  ou `expect` em código de produção de crates. Quebras de invariantes internas
  devem virar `Diagnostic::ice(...)` reportável.

## Diagnósticos e testes dourados

- Os prefixos são `LX`, `P`, `N`, `T`, `O`, `W` e `ICE`. `DiagCode` é a fonte
  única da verdade para códigos voltados ao usuário.
- Todo código novo voltado ao usuário exige entrada em `DiagCode`, mapeamento,
  catálogo em `docs/diagnostics/SPEC.md` e `docs/errors/<CODIGO>.md` em inglês.
  ICEs não exigem documento em `docs/errors/`.
- A bijeção `DiagCode` ↔ `docs/errors/*.md` é obrigatória. Não mantenha lista
  paralela de códigos no build script.
- Preserve spans reais e a ordenação determinística dos diagnósticos. O
  renderer atual é Miette; não introduza outro renderer sem decisão explícita.
- Fixtures dourados cobrem lexer/parser/semântica/HIR/AMIR/UI. Só use
  `UPDATE_EXPECT=1 cargo test --workspace --locked` após inspecionar e aceitar cada
  alteração de snapshot.

## Regra de edição e validação

- Antes de editar, identifique a fase e o crate proprietário. Faça a menor
  mudança que preserve as APIs estreitas de query e acrescente regressões para
  qualquer invariante tocada (cutoff, ciclos, determinismo, CFG, layout ou LSP).
- Não reporte conclusão sem executar, nesta ordem, a partir da raiz:

  1. `cargo fmt --all -- --check`
  2. `cargo check --workspace --locked`
  3. `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  4. `cargo test --workspace --locked`
  5. `cargo run --locked -p xtask -- check-diag-docs`

- Para mudanças em diagnósticos, execute também
  `bash scripts/check-diag-determinism.sh arandu_typeck 8` quando Bash estiver
  disponível. Para queries/LSP, execute os testes de integração relevantes em
  `arandu_query/tests/` (por exemplo `architecture_invariants`,
  `salsa_imports`, `item_body_cutoff`, `ide_diag_delta` e `block_delta`).
- Não adicione dependências, altere IDs, una queries, remova guardrails ou
  atualize snapshots em massa sem justificar a decisão e cobrir o risco com
  teste de regressão.
