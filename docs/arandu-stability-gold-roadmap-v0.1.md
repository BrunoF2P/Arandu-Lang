# Arandu — Roadmap de Estabilização Gold

**Status:** ativo — bloqueia a abertura de uma nova grande fase do roadmap mestre.  
**Escopo:** compilador, CLI, queries, diagnósticos, backends no escopo declarado e processo de release.  
**Fora do escopo:** completar LLVM, estabilizar uma ABI 1.0 ou implementar todo o ecossistema futuro.

Este roadmap transforma “estável” em uma promessa verificável. Ele não exige que
todo subsistema futuro exista; exige que cada capacidade publicada declare seus
limites, falhe de forma controlada e seja protegida por gates reproduzíveis.

## Estados canônicos

| Estado | Significado |
|--------|-------------|
| `gold` | Escopo completo, limites documentados, regressões automatizadas e todos os gates verdes. |
| `done` | Implementação prevista existe e está coberta, mas ainda falta ao menos um requisito de produto/gold. |
| `partial` | Caminho útil existe, porém há lacunas relevantes dentro do escopo desejado. |
| `experimental` | Disponível para validação; compatibilidade e comportamento podem mudar. |
| `planned` | Ainda não há implementação integrada ao caminho canônico. |

`done` não promove automaticamente um item a `gold`. Dependências futuras podem
melhorar uma implementação antiga; nesse caso o item permanece `done` ou
`partial` até que seu contrato gold seja satisfeito, sem reescrever a história da
fase em que apareceu.

## Baseline auditado

| Área | Estado | Evidência e limite atual |
|------|--------|--------------------------|
| Lexer/parser/CST | `done` | Goldens, recovery e CST-first; falta campanha de robustez/release explicitamente versionada. |
| Resolve/typeck | `done` | Multi-file, privacidade, generics e testes oficiais; estabilidade da superfície da linguagem ainda é v0.x. |
| AHIR/AMIR/dataflow | `done` | Invariantes, CFG, move/borrow e otimizações cobertos; IR pública/serializada não é estável. |
| Salsa incremental | `done` | Cutoff por arquivo/item/bloco e guardrails; falta orçamento de performance contínuo para gold. |
| CLI de projeto | `done` | `new/check/run/build/doctor` e instalação cobertos; matriz de release cross-platform é residual. |
| Diagnósticos | `done` | Bijeção `DiagCode` ↔ docs e goldens; preservar determinismo contínuo. |
| Cranelift JIT | `experimental` | Backend host dev/debug, não backend release nem promessa 32-bit. |
| Backend C | `experimental` | Caminho portátil correto no escopo testado; runtime freestanding e polimento não são gold. |
| ABI/runtime | `partial` | `DataLayout` e runtime host existem; ABI pública estável não existe. |
| Contratos e recuperação transversal | `gold` | Entrada inválida, ICE, CLI/LSP, backends e determinismo protegidos por `S1 / Recovery` em Linux/Windows; PR #8. |

## S0 — Baseline reproduzível

- [x] `cargo fmt --all -- --check`.
- [x] `cargo check --workspace --locked`.
- [x] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
- [x] `cargo test --workspace --locked`.
- [x] `cargo run --locked -p xtask -- check-diag-docs`.
- [x] CI executa exatamente os gates acima em ordem e sem exceções silenciosas (`S0 / Gate` validado em runner GitHub).
- [x] Toolchain verificado fixado em `rust-toolchain.toml`; MSRV ainda não é prometido.
- [x] Clippy e rustfmt pertencem ao mesmo toolchain da compilação.
- [x] `Cargo.lock` é obrigatório e os gates de build usam `--locked`.
- [x] Stable futuro roda como aviso e não altera silenciosamente o gate obrigatório.
- [x] Actions usam SHA imutável e permissões mínimas; Dependabot acompanha atualizações.

**DoD S0:** um checkout limpo reproduz todos os gates localmente e em CI.

**Estado:** `gold` — DoD técnico validado localmente e no GitHub Actions;
`S0 / Gate` confirmado como check obrigatório da branch principal em
2026-08-20.

### Política de atualização do toolchain

1. O job semanal `Future stable (advisory)` antecipa incompatibilidades sem bloquear `main`.
2. A adoção ocorre em PR exclusivo que atualiza `rust-toolchain.toml`.
3. O PR executa S0 completo e os testes arquiteturais focados de query/LSP.
4. Novos lints são corrigidos na causa; não se adiciona `allow` global apenas para liberar o gate.
5. `Cargo.lock` só muda quando necessário e seu diff é revisado.
6. A versão anterior permanece um rollback simples até o merge do PR.

O toolchain verificado não é um MSRV. Um `rust-version` só será publicado
depois que a versão mínima for testada em CI e sua política de suporte estiver
documentada.

### Configuração externa necessária

- Branch protection exige `S0 / Gate`.
- Alterações em `.github/workflows/**` devem receber revisão explícita.
- Jobs advisory/fuzz não são requisitos de merge.

## S1 — Contratos e recuperação

Roadmap executável e auditoria de mercado: [`arandu-s1-contracts-recovery-roadmap-v0.1.md`](arandu-s1-contracts-recovery-roadmap-v0.1.md).

- [x] Classificar `panic!`, `unwrap` e `expect` em código de produção: teste/invariante local, erro recuperável ou ICE reportável.
- [x] Converter caminhos alcançáveis por código Arandu inválido em diagnóstico/ICE, nunca abort do compilador.
- [x] Documentar limites suportados de cada backend e comando da CLI.
- [x] Garantir ordenação determinística de diagnósticos e artefatos em execuções repetidas.
- [x] Manter testes de ciclos de imports, IDs stale, CFG e layout como guardrails obrigatórios.

**DoD S1:** entradas inválidas dentro da gramática suportada não derrubam o processo e os limites experimentais são explícitos.

**Estado S1:** `gold` — promovido em 2026-08-21 pelo PR #8 (`28fe6e8`), com
`S0 / Gate` e `S1 / Recovery` aprovados na branch principal protegida.

## S2 — Projetos reais e sessões longas

Roadmap executável: [`arandu-s2-real-projects-endurance-roadmap-v0.1.md`](arandu-s2-real-projects-endurance-roadmap-v0.1.md).

- [ ] Corpus versionado com projetos multi-file pequenos, médios e adversariais.
- [ ] Teste repetido de editar/renomear/remover módulos sem reutilização de `FileId`.
- [ ] Sessões longas de query/LSP sem crescimento não limitado de memória.
- [ ] Benchmarks com orçamento para cold build, noop rebuild e edição isolada.
- [ ] Fuzzing contínuo de lexer/parser/CST e seeds de crashes preservadas como regressão.
- [ ] Teste de determinismo repetido com ordens de registro e paralelismo diferentes.

**DoD S2:** o compilador permanece correto e previsível além das fixtures unitárias.

## S3 — Distribuição beta gold

- [ ] Matriz de instalação e smoke test nos hosts oficialmente suportados.
- [ ] Artefatos versionados, checksums e descoberta de stdlib testados fora do monorepo.
- [ ] Política de compatibilidade v0.x para linguagem, manifesto, CLI e LSP publicada.
- [ ] Release candidate usado em projetos do corpus sem bloqueador conhecido.
- [ ] Lista de limitações conhecidas revisada e ligada ao roadmap mestre.

**DoD S3:** Arandu é utilizável como beta gold dentro do escopo publicado.

## Regra de saída

Este roadmap fecha somente quando S0–S3 estiverem completos. Depois disso, o
roadmap mestre pode escolher a próxima grande fase. Itens LLVM, ABI 1.0,
self-hosting e ecossistema continuam `planned`; não bloqueiam o beta gold porque
não fazem parte da promessa publicada.
