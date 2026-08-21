# Arandu S3 — Distribuição beta gold

**Status:** ativo; campanha aberta em 2026-08-21 após S2 Gold.  
**Objetivo:** entregar uma release candidate instalável, verificável e honesta
fora do monorepo nos hosts publicados.  
**Pré-requisitos:** S0, S1 e S2 `gold` e obrigatórios na `main`.

## Escopo Gold

S3 estabiliza distribuição e promessa de compatibilidade; não transforma os
backends experimentais em ABI 1.0. O produto beta inclui CLI, LSP e stdlib como
uma unidade versionada. LLVM, self-hosting, debugger e Intel macOS pré-compilado
continuam fora do escopo enquanto não houver builder verificável.

## Decisões confrontadas com implementações maduras

| Referência | Risco | Decisão Arandu |
| --- | --- | --- |
| [Cargo SemVer](https://doc.rust-lang.org/cargo/reference/semver.html) | Em `0.y.z`, Cargo trata `y` como fronteira incompatível; em `0.0.z`, cada release pode quebrar. | Beta público começa em `0.1.0-rc.N`; `0.1.z` preserva superfícies publicadas e quebra apenas em `0.2.0`. |
| [Cargo `rust-version`](https://doc.rust-lang.org/cargo/reference/rust-version.html) | Toolchain verificado não é automaticamente MSRV. | Binários não exigem Rust no host; builds da fonte usam somente o toolchain fixado, sem promessa de MSRV até existir CI específico. |
| [GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases) | Tag e assets podem divergir ou uma publicação parcial parecer completa. | Tag, versões internas, nomes, conteúdo e manifest de release são validados antes de publicar um draft completo. |
| [GitHub artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations) | Checksum detecta alteração, mas sozinho não prova origem/build. | Preservar BLAKE3 e adicionar attestation de provenance para cada archive público. |
| [GitHub immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases) | Assets ou tag mutáveis permitem substituição pós-publicação. | Construir draft, anexar tudo e só então publicar; habilitar immutable releases antes do primeiro beta final. |
| [rustup components](https://rust-lang.github.io/rustup/concepts/components.html) | Distribuições maduras separam componentes/targets e não inferem compatibilidade pelo host do build. | Manifest versionado declara host, componentes, hashes e layout; não anunciar target sem smoke nativo. |

## Matriz publicada inicial

| Host | Archive | Estado pretendido |
| --- | --- | --- |
| Linux x86_64 glibc | `.tar.gz` | obrigatório |
| macOS Apple Silicon | `.tar.gz` | obrigatório |
| Windows x86_64 MSVC | `.zip` | obrigatório |
| macOS Intel | nenhum binário | build da fonte, limitação explícita |
| Linux musl/ARM e Windows ARM | nenhum binário | não suportado no beta inicial |

“Suportado” significa: archive construído nativamente, checksum/proveniência,
instalação em prefixo vazio, execução fora do checkout, descoberta da stdlib e
projetos do corpus aprovados. Compilar no CI sem instalar não promove um host.

## S3-A — Contrato e fonte única de versão

- [x] Publicar política v0.x separando linguagem, manifesto, CLI, diagnósticos,
      LSP, stdlib, formato de archive e APIs internas sem estabilidade.
- [x] Criar verificador único para tag ↔ workspace ↔ CLI ↔ LSP ↔ extensão e
      impedir versões divergentes antes do build. O release manifest entra no
      mesmo verificador quando for criado em S3-B.
- [x] Definir `0.1.0-rc.N` como canal RC e critérios objetivos para `0.1.0`.
- [x] Publicar matriz de hosts, componentes e comandos realmente suportados.
- [x] Consolidar limitações conhecidas sem prometer LLVM/ABI/freestanding.

**Saída:** uma release não pode declarar uma promessa maior que seus contratos.

Contrato normativo: [`arandu-distribution-contract-v0.1.md`](arandu-distribution-contract-v0.1.md).
S3-A está concluído documentalmente; os artifacts e instaladores que provam
esse contrato são entregas S3-B/S3-C, não evidência presumida desta etapa.

## S3-B — Pacote autocontido e instaladores portáveis

- [x] Incluir `arandu`, `arandu-lsp`, stdlib, licença, release manifest e hashes
      internos em todo archive; nenhum componente depende do checkout.
- [x] Validar archive contra traversal, links absolutos, entradas duplicadas,
      tipo inesperado e conteúdo extra antes da publicação/instalação.
- [x] Manter `.tar.gz` reproduzível em Unix e criar `.zip` determinístico no
      Windows, sem fingir que Bash/symlink é instalação nativa.
- [x] Criar instalador PowerShell com publicação versionada/atômica e launcher
      adequado; Unix preserva prefixo versionado e links relativos.
- [x] Exigir checksum externo no modo release; ausência só é permitida em modo
      de desenvolvimento explicitamente opt-in.

**Saída:** o arquivo baixado contém tudo que o usuário executará e é validado
antes de tocar o prefixo final.

**Estado:** `gold` local em Windows; promoção depende dos packages e installers
verdes nos três runners do workflow de release. A prova de uso fora do checkout
continua pertencendo ao S3-C.

## S3-C — Matriz de instalação e RC no corpus

- [ ] Criar `S3 / Distribution` para Linux, macOS ARM e Windows x86_64,
      dependente dos gates gold anteriores.
- [ ] Em cada host, empacotar, instalar em prefixo temporário e remover checkout,
      `ARANDU_STDLIB`, Cargo e caminhos de build da execução.
- [ ] Executar `doctor`, `new`, `check`, `run`, LSP initialize/shutdown e todos
      os projetos publicáveis do corpus usando somente a instalação.
- [ ] Reinstalar a mesma versão, alternar versões e provar rollback/erro atômico
      sem instalação parcialmente visível.
- [ ] Testar archives adulterados, incompletos, de target errado e versão
      incompatível; todos abortam antes de publicar `current`.

**Saída:** a release candidate funciona como produto instalado, não como
checkout privilegiado.

## S3-D — Publicação verificável

- [ ] Workflow de tag cria draft, baixa todos os artifacts, valida conjunto
      exato e só então publica; falha parcial não cria release pública.
- [ ] Gerar BLAKE3, manifest agregado e attestation GitHub/Sigstore para archives
      e manifest; documentar `gh attestation verify`.
- [ ] Fixar actions por SHA, permissões mínimas (`contents`, `id-token`,
      `attestations`) e nenhuma credencial persistida nos builders.
- [ ] Habilitar immutable releases e documentar que tags/assets publicados não
      são substituídos; correção exige nova versão.
- [ ] Dry-run manual produz o mesmo conjunto, mas nunca publica nem atesta como
      release oficial.

**Saída:** consumidores verificam bytes, origem, workflow, commit e tag.

## S3-E — Promoção beta gold

- [ ] Publicar `0.1.0-rc.1`, instalar os três hosts e registrar relatório da RC.
- [ ] Triar bloqueadores; toda correção repete a RC com número novo, sem mover tag.
- [ ] Revisar documentação pública, comandos, matriz e limitações a partir dos
      artifacts reais, não do estado da `main`.
- [ ] Promover `0.1.0` somente com S3 verde, nenhum bloqueador conhecido e
      ruleset/release settings confirmados.
- [ ] Marcar S3 e roadmap de estabilidade `gold`; abrir fase futura separada
      para novos targets, package manager, assinatura adicional ou ABI 1.0.

**Saída:** Arandu é beta gold dentro de uma promessa pequena e verificável.

## Definition of Done

S3 será `gold` quando:

- os três hosts publicarem e instalarem artifacts autocontidos;
- tag, versões, manifest, checksums e provenance formarem uma cadeia verificável;
- CLI, LSP, stdlib e corpus funcionarem fora do checkout;
- política v0.x e limitações conhecidas estiverem publicadas;
- uma RC completa não tiver bloqueador conhecido; e
- `S3 / Distribution` estiver verde e obrigatório na `main`.
