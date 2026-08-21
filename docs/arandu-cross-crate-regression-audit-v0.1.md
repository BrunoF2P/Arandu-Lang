# Arandu — auditoria de regressões por crate

**Status:** S1 `gold` desde 2026-08-21 (PR #8); matriz residual alimenta S2.
**Objetivo:** transformar falhas reais de compiladores e ferramentas em testes
proporcionais ao risco do Arandu, sem importar casos de recursos inexistentes.

## Método

Cada candidato precisa ter: fonte primária, caminho Arandu alcançável, oráculo
determinístico e crate proprietário. Um teste só conta como evidência Gold se
falhar diante da regressão que pretende impedir. Casos de segurança de um
backend externo são adaptados ao limite controlado pelo Arandu; não alegamos
testar internals que pertencem ao Cranelift, Salsa ou Rowan.

## Fontes iniciais e decisões

- O [changelog do Salsa](https://github.com/salsa-rs/salsa/blob/master/CHANGELOG.md)
  registra perda de acumuladores em cache, estado stale após cancelamento,
  problemas de ciclos e reutilização de slots. Arandu testa cache de
  diagnósticos, revisões, ciclos, concorrência e identidades geracionais.
- A [IR do Cranelift](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md)
  exige terminador por bloco e correspondência entre parâmetros e argumentos.
  Arandu testa isso antes do backend e diferencialmente entre C e JIT.
- O advisory de [miscompilação de endereço no Cranelift/AArch64](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-jhxm-h53p-jm7w)
  reforça que cálculo de índice, shift, bounds check e load devem usar a mesma
  semântica. O caso Arandu aplicável é array/index/layout, não sandbox WebAssembly.
- A especificação LSP define posições em UTF-16; regressões de offsets e
  resultados stale são tratadas nas fronteiras de conversão e publicação.
- O `rustc_abi` usa `SizeOverflow` e um limite exclusivo por address space;
  `std::alloc::Layout` também torna `repeat`/`extend` falíveis. Arandu agora
  aplica o limite positivo de `isize` do alvo a arrays, offsets e padding, e
  propaga `LayoutError` até MIR/C/Cranelift sem saturação.

## Matriz executável

| Crate | Falha grave aplicável | Evidência atual | Próxima regressão Gold |
|---|---|---|---|
| `arandu_base` | overflow/offset no meio de UTF-8; ID reutilizado | UTF-16 básico e registry denso | offsets inválidos nunca cortam code point; limite de conversão testado |
| `arandu_lexer` | Unicode/escape/comment truncado causa loop ou unwind | recovery e goldens | corpus adversarial determinístico e CRLF/LF equivalente |
| `arandu_parser` | nesting truncado causa unwind; recovery engole item seguinte | recovery/CST goldens | corpus sem unwind, nesting limitado e irmão preservado após erro |
| `arandu_diagnostics` | código sem documentação; ordem/span não determinístico | bijeção e renderer | render LF/CRLF e Unicode repetido byte a byte |
| `arandu_middle` | CFG/SSA inválida aceita; layout usa ABI host | validator cobre IDs, ownership de statements, aridade e tipos por aresta/parâmetro; layout falível testa limites/offsets 32/64 | ampliar layouts recursivos e limites específicos dos backends |
| `arandu_resolve` | ciclo/HashMap muda diagnóstico; namespace stale | ciclo permutado produz diagnósticos idênticos | ampliar permutações para grafos com três módulos |
| `arandu_typeck` | recovery produz cascata; tipo recursivo explode | aliases/ciclos/goldens | profundidade adversarial e corpus sem unwind |
| `arandu_mir` | DCE apaga efeito/jump arg; pass não converge | CFG, DCE, ICE tipado e validação pré/pós-pass | tipos incompatíveis nos argumentos e convergência adversarial |
| `arandu_semantics` | composição das fases perde spans ou entra em panic | recovery/HIR/AMIR | corpus transversal sem unwind (iniciado) |
| `arandu_query` | acumulador some no cache; ciclo/cancelamento deixa stale | cutoff, cache, ciclos, snapshots, revisão pós-cancelamento e ciclo estável por 16 revisões | ciclo concorrente repetido e revisão após panic de evento |
| `arandu_backend_c` | IR inválida gera C parcial; signed/layout diverge | validator compartilhado antes da emissão; rejeição idêntica de aresta SSA, tipo poison e faixa inválida; `ICE-GEN-001`; emissão byte-idêntica | matriz signed/layout e atomicidade no CLI |
| `arandu_backend_cranelift` | bloco/parâmetro inválido ou index/shift miscompila | validator compartilhado antes de mutar o JIT; mesma rejeição do C; verifier e diferencial índice+shift | limites de shift e índices negativos/fora da faixa conforme contrato |
| `arandu_cli` | erro operacional vira sucesso/artefato parcial | exit codes e projetos | atomicidade de build/emit e caminhos Unicode/CRLF |
| `arandu_lsp` | UTF-16/CRLF desloca edição; worker morre ou publica stale | revisão/debounce; panic isolado por job; snapshot falho descartado; `ContentModified`; close elimina edição pendente | roundtrip astral+CRLF e campanha obrigatória em hosts adicionais |
| `arandu_fmt` | segunda formatação muda saída; string altera indentação | smoke estrutural | idempotência Unicode/CRLF e fallback inválido (iniciado) |
| `arandu_test_support` / `xtask` | snapshot errado é atualizado silenciosamente | comparação e bijeção | update exige opt-in e arquivos órfãos falham deterministicamente |

## Critério de promoção

S1 foi promovido a `gold` no PR #8 após regressões automatizadas ou
justificativas explícitas de não aplicabilidade por linha, execução de
`S1 / Recovery` em Linux e Windows e preservação do `S0 / Gate` obrigatório.
As colunas “Próxima regressão Gold” agora são entradas do corpus e das campanhas
de endurance do S2; não reabrem o contrato S1 já satisfeito.
