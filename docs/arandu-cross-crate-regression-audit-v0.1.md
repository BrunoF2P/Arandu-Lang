# Arandu — auditoria de regressões por crate

**Status:** ativo; primeira campanha S1 iniciada em 2026-08-20.  
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

## Matriz executável

| Crate | Falha grave aplicável | Evidência atual | Próxima regressão Gold |
|---|---|---|---|
| `arandu_base` | overflow/offset no meio de UTF-8; ID reutilizado | UTF-16 básico e registry denso | offsets inválidos nunca cortam code point; limite de conversão testado |
| `arandu_lexer` | Unicode/escape/comment truncado causa loop ou unwind | recovery e goldens | corpus adversarial determinístico e CRLF/LF equivalente |
| `arandu_parser` | nesting truncado causa unwind; recovery engole item seguinte | recovery/CST goldens | corpus sem unwind, nesting limitado e irmão preservado após erro |
| `arandu_diagnostics` | código sem documentação; ordem/span não determinístico | bijeção e renderer | render LF/CRLF e Unicode repetido byte a byte |
| `arandu_middle` | CFG/SSA inválida aceita; layout usa ABI host | validator e layouts 32/64 | argumentos de todas as arestas e overflow de layout adversarial |
| `arandu_resolve` | ciclo/HashMap muda diagnóstico; namespace stale | ciclos e imports | permutar ordem de registro e comparar diagnósticos completos |
| `arandu_typeck` | recovery produz cascata; tipo recursivo explode | aliases/ciclos/goldens | profundidade adversarial e corpus sem unwind |
| `arandu_mir` | DCE apaga efeito/jump arg; pass não converge | CFG, DCE e ICE tipado | predecessor/param incompatível e validação pré/pós-pass |
| `arandu_semantics` | composição das fases perde spans ou entra em panic | recovery/HIR/AMIR | corpus transversal sem unwind (iniciado) |
| `arandu_query` | acumulador some no cache; ciclo/cancelamento deixa stale | cutoff, cache, ciclos, snapshots | cancelamento seguido de nova revisão e ciclo concorrente repetido |
| `arandu_backend_c` | IR inválida gera C parcial; signed/layout diverge | paridade e `Result` tipado | rejeição `ICE-GEN-001` e emissão repetida byte a byte |
| `arandu_backend_cranelift` | bloco/parâmetro inválido ou index/shift miscompila | verifier e JIT | diferencial C/JIT de índices, shifts e limites aplicáveis |
| `arandu_cli` | erro operacional vira sucesso/artefato parcial | exit codes e projetos | atomicidade de build/emit e caminhos Unicode/CRLF |
| `arandu_lsp` | UTF-16/CRLF desloca edição; worker publica stale | revisões/debounce | roundtrip astral+CRLF (iniciado) e resposta descartada após close |
| `arandu_fmt` | segunda formatação muda saída; string altera indentação | smoke estrutural | idempotência Unicode/CRLF e fallback inválido (iniciado) |
| `arandu_test_support` / `xtask` | snapshot errado é atualizado silenciosamente | comparação e bijeção | update exige opt-in e arquivos órfãos falham deterministicamente |

## Critério de promoção

Esta matriz não promove S1 sozinha. S1 torna-se `gold` apenas quando todas as
linhas têm regressão automatizada ou justificativa explícita de não
aplicabilidade, o job `S1 / Recovery` roda em Linux e Windows, e S0 continua
obrigatório e verde.
