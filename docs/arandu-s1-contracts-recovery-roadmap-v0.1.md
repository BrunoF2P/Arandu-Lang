# Arandu S1 — Contratos e recuperação

**Status:** em implementação; S1-A/S1-B/S1-C concluídos.
**Data da auditoria:** 2026-08-20.  
**Objetivo:** código Arandu inválido nunca encerra o compilador por `panic`; falhas
internas são ICEs identificáveis, e falhas operacionais pertencem à CLI/LSP.

Auditoria transversal de falhas reais e regressões por crate:
[`arandu-cross-crate-regression-audit-v0.1.md`](arandu-cross-crate-regression-audit-v0.1.md).

Este documento desdobra o S1 do roadmap Gold. Ele não promete ausência absoluta
de `panic`: distingue entrada inválida, falha operacional e quebra de invariante
do compilador. Apagar `unwrap` mecanicamente ou continuar usando uma DB Salsa
depois de uma quebra interna reduziria a confiabilidade em vez de aumentá-la.

## Decisão confrontada com o mercado

| Referência | Prática observada | Decisão Arandu |
|---|---|---|
| [rustc: saída JSON](https://doc.rust-lang.org/rustc/json.html) | Erro do programa e ICE são categorias distintas e estruturadas. | Preservar `DiagnosticKind::User` e `InternalCompilerError`; ICE nunca deve parecer erro do usuário. |
| [rustc: níveis de diagnóstico](https://doc.rust-lang.org/beta/nightly-rustc/rustc_errors/enum.Level.html) | Erro recuperável, fatal operacional, bug e bug atrasado têm contratos diferentes. | Adotar três vias: diagnóstico do usuário, erro operacional da camada de entrada e ICE reportável. |
| [rustc: depuração de ICE](https://rustc-dev-guide.rust-lang.org/compiler-debugging.html) | Panic interno encerra a compilação com identificação, versão e orientação de reprodução. | Invariante impossível pode encerrar a compilação, mas deve chegar à fronteira como ICE estável e contextualizado. |
| [rustc: incremental](https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html) | Early-cutoff pressupõe queries puras e determinísticas. | Não capturar panic dentro de query para continuar com estado possivelmente inválido; converter falhas esperáveis antes da query retornar. |
| [LLVM: builds determinísticos](https://blog.llvm.org/2019/11/deterministic-builds-with-clang-and-lld.html) | Repetir builds e comparar saídas é um gate, não uma suposição. | Testar ordem, conteúdo e hashes em repetições; separar determinismo local de reprodutibilidade universal. |
| [Cargo: contrato CLI](https://doc.rust-lang.org/cargo/commands/cargo-build.html#exit-status) | Sucesso e falha têm códigos de processo documentados. | Publicar códigos da CLI e impedir que retorno de programa compilado seja confundido com falha do compilador. |
| [LSP 3.18](https://microsoft.github.io/language-server-protocol/) | O servidor é um processo duradouro com respostas versionadas. | Falha de um worker descarta seu snapshot; a thread principal não publica resultado stale nem reutiliza análise quebrada. |

### Veredito

A arquitetura atual está alinhada com compiladores incrementais modernos:
diagnósticos tipados, `DiagCode`, queries puras, validação de AMIR e IDs
geracionais já são uma base correta. A lacuna não é trocar Miette, Salsa ou os
backends. A lacuna é fechar as fronteiras onde uma quebra interna ainda vira
panic cru ou texto ad hoc.

## Estado atual confrontado

### O que já está correto

- `Diagnostic::ice` e códigos `ICE-*` já distinguem bug do compilador.
- Lowering HIR/AMIR e Cranelift já retornam ICE em vários caminhos inválidos.
- `amir_validate` verifica SSA/CFG antes do backend.
- CLI retorna `2` para uso inválido e `1` para falha de compilação.
- Queries não fazem I/O e preservam limites de cutoff.
- Testes cobrem ciclos de import, IDs/revisões stale, CFG, layout e
  determinismo de diagnósticos.

### Inventário confiável de panic

A busca textual encontrou 259 ocorrências de `panic!/unwrap/expect`, mas esse
número inclui testes dentro de `src/` e o método recuperável
`Cursor::expect(TokenKind)`. Ele não representa risco de produção.

O lint semântico `clippy::panic` encontrou **9 panics reais em bibliotecas**:

| Dono | Quantidade | Classificação inicial |
|---|---:|---|
| `arandu_middle::ice` | 2 | Invariante denso deliberado; falta fronteira de relatório uniforme. |
| `arandu_mir::{dce,simplify_cfg}` | 5 | Quebra de transformação; deve retornar ICE, não abortar dentro do pass. |
| `arandu_backend_c::emitter` | 2 | AMIR inesperada; a API atual retorna `String` e impede propagar ICE. |

`unwrap_used` e `expect_used` já são lints do workspace. Testes podem permitir
esses lints localmente; produção não ganha `allow` amplo.

### Lacunas de contrato

1. `emit_c` retorna `String`; precisa retornar `Result<String, Diagnostic>`.
2. DCE e simplificação de CFG não possuem canal de erro para ICE.
3. `validate_hir_and_monomorphize` imprime uma quebra HIR como texto comum.
4. Os helpers fatais de pools densos têm prefixo estável, mas não incluem
   versão/comando/contexto para relato.
5. Limites de CLI, backends e códigos de saída estão espalhados no README e em
   documentos de arquitetura; falta um contrato público único.
6. Determinismo de diagnósticos está coberto, mas artefatos C/pacotes ainda não
   possuem comparação repetida dedicada no gate S1.

## Contrato de erro aprovado

### E1 — Entrada Arandu inválida

- Retorna `Diagnostic` de usuário com span real e código documentado.
- Pode recuperar para emitir erros independentes, sem cascata artificial.
- Nunca usa `panic`, `unwrap` ou `expect` alcançável pela entrada.
- CLI encerra com `1`; LSP publica apenas para documento/revisão ainda vivos.

### E2 — Falha operacional

- Exemplos: arquivo ausente, manifesto ilegível, stdlib não localizada,
  toolchain C ausente e falha ao criar worker.
- É erro da CLI/LSP, não diagnóstico semântico e não ICE.
- Mensagem contém operação, caminho quando seguro e causa encadeada.
- Uso inválido da CLI retorna `2`; falha operacional/compilação retorna `1`.

### E3 — Quebra de invariante

- Retorna `Diagnostic::ice` sempre que a API possui contexto e canal `Result`.
- O código ICE identifica a fase; a mensagem inclui invariante e span seguro.
- Não continua codegen nem reutiliza snapshot/DB depois da quebra.
- Panic fatal fica restrito a acessores densos infalíveis sem canal de erro,
  marcado com `#[cold]`, documentação `# Panics` e allow local justificado.

## Roadmap de implementação

### S1-A — Guardrail e classificação

- [x] Ativar `clippy::panic = "warn"` no workspace.
- [x] Remover os sete panics propagáveis antes de deixar `-D warnings` bloqueá-los.
- [x] Manter allow de produção apenas nos dois helpers densos, com justificativa local.
- [x] Criar inventário versionado por crate: recuperável, operacional, ICE ou teste.
- [x] Adicionar regressão que compila corpus inválido sob `catch_unwind` no
      harness de teste; o compilador deve retornar diagnóstico, não unwind.

**Saída:** novos panics de produção falham no S0 e as exceções são explícitas.

### S1-B — Passes de IR recuperáveis

- [x] Fazer DCE e simplificação CFG retornarem `Result<_, Diagnostic>` ou uma
      coleção de ICEs, preservando convergência e efeitos observáveis.
- [x] Propagar falha por `optimize` → CLI sem efeitos na query; a otimização
      permanece sobre a cópia pertencente à CLI, fora das queries Salsa.
- [x] Cobrir statement movido duas vezes, predecessor/argumento incompatível e
      terminador inválido com AMIR sintética adversarial.
- [x] Executar `amir_validate` antes e depois das transformações relevantes no
      caminho de otimização da CLI.

**Saída:** AMIR malformada falha como `ICE-O-001`/`ICE-GEN-002`, sem panic.

### S1-C — Backends com contrato tipado

- [x] Alterar backend C para `Result<String, Diagnostic>`.
- [x] Converter `NullCoalesce` residual e `Ref/RefMut` não baixado em
      `ICE-GEN-001`.
- [x] Garantir que C e Cranelift rejeitam as mesmas classes de AMIR inválida
      (aresta SSA, tipo poison e faixa de statements), antes de gerar artefato.
- [x] Documentar matriz de tipos, layouts, host/cross-target e recursos não
      suportados por backend em
      [`arandu-backend-contract-v0.1.md`](arandu-backend-contract-v0.1.md).
- [x] Tornar layout target-dependent falível, com overflow verificado em
      arrays/agregados e propagação sem artefato parcial em C/Cranelift.
- [x] Testar que backend inválido retorna `Err(ICE-GEN-001)`, sem sucesso
      contendo artefato parcial.

**Saída:** todo backend tem sucesso tipado ou diagnóstico tipado.

### S1-D — Fronteiras CLI e LSP

- [x] Centralizar `CliError { kind, exit_code, source }` ou equivalente local.
- [x] Renderizar falha de validação HIR/AMIR como ICE, não `eprintln!` ad hoc.
- [x] Publicar tabela dos comandos, backends, estabilidade e códigos de saída em
      [`arandu-cli-lsp-contract-v0.1.md`](arandu-cli-lsp-contract-v0.1.md).
- [x] No LSP, transformar panic de worker em falha isolada, descartar snapshot e
      nunca publicar resultado stale; não tentar continuar a mesma análise.
- [x] Adicionar testes de arquivo removido/fechado, URI inválida, stdlib ausente
      e cancelamento/revisão durante edição.

**Saída:** CLI e LSP aplicam o mesmo contrato sem misturar camadas.

### S1-E — Determinismo e guardrails obrigatórios

- [x] Repetir diagnósticos com ordens de registro e paralelismo diferentes.
- [x] Gerar C duas vezes e comparar bytes no mesmo ambiente/toolchain.
- [x] Empacotar duas vezes e comparar lista, permissões e conteúdo; timestamps
      de arquivo ficam explicitamente fora ou são normalizados.
- [x] Manter obrigatórios: ciclos de imports, stale IDs/revisions, CFG/SSA e
      layouts host/i686/pointer-width.
- [x] Criar job `S1 / Recovery` dependente de `S0 / Gate`.

**Saída:** falhas de recuperação ou ordem não podem regressar silenciosamente.

## Ordem recomendada

1. S1-A estabelece o guardrail sem mudar APIs públicas.
2. S1-B cria propagação de ICE desde AMIR.
3. S1-C adapta os backends ao canal tipado.
4. S1-D consolida UX e sessão longa.
5. S1-E fecha evidência repetível e promove S1.

Não iniciar pelo LSP: ele consome os contratos de query/pass/backend. Melhorar
o editor antes de tipar essas falhas duplicaria tratamento e tornaria o
servidor responsável por corrigir erros das camadas inferiores.

## Definition of Done

S1 será `gold` quando:

- todo código inválido do corpus e fuzz seeds termina em diagnóstico, nunca unwind;
- os únicos panics de biblioteca restantes forem helpers ICE locais auditados;
- CLI/backends tiverem limites e códigos de saída publicados;
- diagnósticos e artefatos forem determinísticos no escopo declarado;
- ciclos, IDs stale, CFG e layout continuarem verdes no gate obrigatório;
- `S1 / Recovery` passar em Linux e Windows, com smoke de instalação no macOS.
