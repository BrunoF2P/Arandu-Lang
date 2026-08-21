# Arandu S2 — Projetos reais e sessões longas

**Status:** ativo; campanha aberta em 2026-08-21 após promoção S1 Gold.
**Objetivo:** provar que o compilador incremental permanece correto, limitado em
recursos e previsível durante projetos multi-file e milhares de revisões, além
das fixtures unitárias.
**Pré-requisitos:** S0 e S1 `gold`; arquitetura Salsa/LSP e identidades atuais
permanecem normativas.

## Escopo Gold

S2 não adiciona features de linguagem. Ele transforma caminhos já publicados em
cargas versionadas, budgets mensuráveis e regressões de sessão longa. LLVM, ABI
1.0, debugger e features avançadas do editor continuam fora do escopo.

## Decisões confrontadas com implementações maduras

| Referência | Risco observado | Decisão Arandu |
|---|---|---|
| [rustc-perf](https://github.com/rust-lang/rustc-perf) | Performance de compilador precisa de corpus e coleta repetível, não microbenchmarks isolados. | Versionar projetos por classe e medir cold, noop e edição isolada com protocolo fixo. |
| [Salsa: database/runtime](https://salsa-rs.github.io/salsa/plumbing/database_and_runtime.html) | Escritas cancelam handles paralelos; clones vivos podem bloquear mutação. | Stress alterna snapshots, cancelamento, flush e edição sem manter handle durante `set_text`. |
| [Salsa: tuning](https://salsa-rs.github.io/salsa/tuning.html) | Memos são ilimitados por padrão e IDs de inputs vivem até a DB cair; LRU não remove toda metadata. | Medir retenção antes de escolher LRU/recriação de DB; orçamento separa memória viva, memos e identidades monotônicas. |
| [Salsa: cycles](https://salsa-rs.github.io/salsa/plumbing/cycles.html) | Ciclos cross-thread envolvem bloqueio e DAG entre workers. | Repetir grafos cíclicos com ordens e paralelismo diferentes, exigindo convergência e saída idêntica. |
| [Rust Fuzz Book](https://rust-fuzz.github.io/book/) | Fuzzing encontra falhas por entradas pseudoaleatórias. | Todo crash minimizado vira seed versionada e teste determinístico fora do fuzzer. |
| [LSP 3.18](https://microsoft.github.io/language-server-protocol/) | O servidor é duradouro e atende requests concorrentes sobre documentos mutáveis. | Corpus S2 alimenta stress do servidor; publicação exige documento e revisão vivos. |
| [Watchman: recrawl](https://facebook.github.io/watchman/docs/troubleshooting#recrawl) | Filas do SO podem perder eventos; confiar somente no delta deixa estado silenciosamente stale. | Checkpoints executam `rescan_listing` conservador e ainda precisam coincidir com rebuild limpo. |
| [Watchman: case-insensitivity](https://facebook.github.io/watchman/docs/casefolding) | Rename e caixa divergem entre filesystems; a mesma identidade pode aparecer como create/change/remove. | Normalizar caminhos verbatim do Windows e testar create/delete/rename após canonicalização. |

## Estados e métricas

- `correctness`: diagnósticos, símbolos e artefatos equivalentes ao rebuild limpo.
- `identity`: `FileId` nunca é reutilizado; handles stale nunca resolvem.
- `endurance`: nenhuma curva monotônica não explicada após warm-up.
- `performance`: budgets relativos a baseline versionada; PR comum usa limites
  funcionais amplos para evitar flake.
- `determinism`: mesma entrada e ambiente produzem lista ordenada e bytes iguais.

## S2-A — Corpus versionado e oráculo limpo

- [x] Criar `tests/projects/{small,medium,adversarial}` com manifesto de casos,
      features exercitadas, comando e resultado esperado.
- [x] Incluir projetos multi-file válidos, inválidos, ciclos, Unicode/CRLF,
      generics, ownership, async e backends dentro do contrato publicado.
- [x] Criar runner único no `xtask`; descoberta e ordem dos casos são
      determinísticas e arquivos órfãos falham o gate.
- [x] Para cada revisão incremental, comparar diagnóstico/HIR/AMIR relevante com
      uma DB limpa construída do mesmo estado final.
- [x] Registrar tamanho do corpus por arquivos, linhas, bytes e módulos, sem usar
      número de testes como proxy de cobertura.

**Saída:** toda campanha S2 usa cargas reais reproduzíveis e um oráculo limpo.

## S2-B — Churn de módulos e identidades

- [x] Executar pelo menos 10 mil operações determinísticas de editar, criar,
      renomear, remover e reabrir módulos em uma sessão.
- [x] Provar monotonicidade de `FileId` e invalidade de `DocumentId`,
      `AnalysisRevision` e `LspSymbolId` antigos.
- [x] Misturar imports cíclicos, rename de pacote e alterações de manifesto sem
      depender da ordem de `HashMap` ou de listagem do filesystem.
- [x] Alternar snapshots/workers com commits, verificando cancelamento sem
      deadlock e descarte de resultados stale.
- [x] Comparar cada checkpoint com rebuild limpo e repetir com seeds/ordens fixas.

**Saída:** sessões longas não confundem identidade histórica com análise atual.

### Falhas encontradas e eliminadas pela campanha

- caminhos existentes recebiam prefixo verbatim `\\?\` no Windows, mas eventos
  de arquivos já removidos não; o registro podia conservar um módulo fantasma;
- `HashEq<Program>` comparava apenas contagens e spans, deixando uma alteração
  literal em corpo importado reutilizar AMIR antigo;
- reload de manifesto reatribuía `FileId` pela ordem aleatória de `HashMap`;
  o lote agora é ordenado antes do registro.

## S2-C — Memória e budgets incrementais

- [ ] Instrumentar métricas estáveis de revisões, arquivos registrados, memos
      relevantes, RSS/heap quando disponível e contadores de execução de query.
- [ ] Medir cold build, noop rebuild e edição isolada por item/bloco no corpus.
- [ ] Definir warm-up, número de amostras, mediana e p95; guardar baseline com
      toolchain, SO, CPU e commit.
- [ ] Detectar crescimento não limitado após janelas de churn; distinguir o
      alocador monotônico de `FileId` de retenção indevida de conteúdo/IR.
- [ ] Só introduzir LRU, compactação ou reciclagem de DB depois da medição e com
      regressões de early-cutoff; nunca reutilizar `FileId`.

**Saída:** performance e memória têm orçamento explícito sem sacrificar correção.

## S2-D — Robustez adversarial e fuzz regressivo

- [ ] Consolidar targets de lexer/parser/CST, lowering e pipeline sem backend;
      cada target tem limite de tamanho/tempo e nenhuma I/O na query.
- [ ] Importar seeds atuais, minimizar duplicatas e registrar origem/bug coberto.
- [ ] Todo crash novo vira fixture determinística com `catch_unwind` apenas no
      harness; produção retorna diagnóstico/ICE conforme S1.
- [ ] Cobrir nesting/profundidade, UTF-8 truncado, comentários/strings abertas,
      tipos recursivos, CFG adversarial e grafos cíclicos concorrentes.
- [ ] Manter fuzz contínuo advisory e corpus regressivo obrigatório no gate.

**Saída:** descobertas aleatórias tornam-se proteção determinística permanente.

## S2-E — Gate de endurance e promoção

- [ ] Criar `S2 / Endurance` dependente de `S1 / Recovery`, com Linux e Windows.
- [ ] Executar corpus, churn, equivalência com rebuild limpo e determinismo em
      todo PR; budgets finos rodam em ambiente controlado e publicam tendência.
- [ ] Executar stress LSP relevante sem iniciar VS Code; Extension Host continua
      pertencendo ao roadmap L0–L3 do editor.
- [ ] Publicar relatório de baseline e limitações, incluindo o que é crescimento
      monotônico intencional e o que encerra a sessão/recria a DB.
- [ ] Manter `S0 / Gate` e `S1 / Recovery` obrigatórios e verdes.

**Saída:** falhas de sessão longa, escala ou ordem bloqueiam promoção silenciosa.

## Ordem de implementação

1. S2-A fornece corpus e oráculo.
2. S2-B aplica churn e valida identidades.
3. S2-C mede antes de otimizar retenção/performance.
4. S2-D amplia entradas adversariais e preserva crashes.
5. S2-E transforma as campanhas em garantia contínua.

## Definition of Done

S2 será `gold` quando:

- corpus pequeno, médio e adversarial estiver versionado e documentado;
- estados incrementais coincidirem com rebuild limpo nos checkpoints definidos;
- 10 mil operações de churn preservarem identidades, correção e ausência de deadlock;
- memória e latência tiverem baseline/budgets reproduzíveis, sem crescimento não
  limitado fora das exceções documentadas;
- crashes de fuzz estiverem preservados como regressões determinísticas; e
- `S2 / Endurance` passar em Linux e Windows dependendo de S1.
