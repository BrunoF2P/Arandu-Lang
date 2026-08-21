# Contrato de distribuição Arandu v0.1

**Estado:** normativo para S3-A; implementação e promoção dependem de S3-B a
S3-E.  
**Modelo:** SDK autocontido, semelhante ao Flutter: CLI, servidor de linguagem
e biblioteca padrão formam uma unidade versionada; a extensão do editor é um
cliente separado.

## Promessa v0.x

O primeiro canal público é `0.1.0-rc.N`. Uma RC pode ser substituída apenas por
uma nova versão e nunca pela movimentação da mesma tag. `0.1.z` preserva os
comandos, formatos e superfícies explicitamente publicados neste documento;
uma quebra deliberada exige `0.2.0`. APIs Rust entre crates, HIR/AMIR internos,
queries Salsa e formatos não documentados não são API pública estável.

`0.1.0` só pode ser promovido quando os três hosts obrigatórios passarem o gate
de distribuição a partir dos archives publicados, sem checkout, Cargo ou
toolchain de desenvolvimento; nenhuma RC pode manter bloqueador conhecido.

## Componentes do SDK

Cada distribuição contém versões compatíveis de:

- `arandu` (`arandu.exe` no Windows), a CLI;
- `arandu-lsp` (`arandu-lsp.exe` no Windows), usado por editores;
- a árvore completa da stdlib;
- licença, manifest de release e hashes do conteúdo.

A extensão VS Code não incorpora outra cópia do compilador. Ela localiza o
`arandu-lsp` instalado, fala LSP e mantém seu próprio ciclo de publicação, mas
sua versão de compatibilidade é validada antes de uma release do SDK.

## Matriz inicial

| Host | Target Rust | Formato obrigatório | Estado no S3-A |
| --- | --- | --- | --- |
| Linux x86-64 glibc | `x86_64-unknown-linux-gnu` | `.tar.gz` | contrato aprovado; prova pendente em S3-C |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` | contrato aprovado; prova pendente em S3-C |
| Windows x86-64 | `x86_64-pc-windows-msvc` | `.zip` | contrato aprovado; implementação pendente em S3-B |

`.pkg`, `.msi`, Homebrew, Winget/Scoop e repositórios Linux são conveniências
posteriores. Eles não substituem os archives portáveis nem podem ampliar a
matriz suportada sem o mesmo smoke nativo. macOS Intel, Linux musl/ARM e
Windows ARM não possuem binário prometido no beta inicial.

## Comandos e dependências do host

Após colocar `bin` no `PATH`, o nome do comando é `arandu` em Bash, Zsh,
PowerShell e CMD. O sufixo `.exe` é transparente no Windows.

| Operação publicada | Ferramenta externa exigida |
| --- | --- |
| `arandu --version`, `doctor`, `new`, `check`, `fmt` | nenhuma |
| `arandu run` pelo Cranelift JIT do host | nenhuma |
| executar `arandu-lsp` | nenhuma; o editor/cliente LSP é separado |
| gerar fonte com `emit-c` | nenhuma |
| compilar e ligar a fonte C gerada | GCC/Clang compatível; caminho experimental |
| compilar o próprio Arandu a partir da fonte | Rust fixado e toolchain nativo do host |

No build da fonte, Windows MSVC requer Visual Studio Build Tools e Windows SDK;
macOS requer Xcode Command Line Tools; Linux requer linker, headers libc e
GCC/Clang. Isso não é requisito de uma instalação binária. `arandu doctor`
deve mostrar ferramentas do backend C como opcionais, não transformar sua
ausência em falha do SDK básico.

## Limites explícitos de v0.1

- `run` é host-JIT; não há promessa de cross-compilation ou executável AOT
  portátil.
- `build --release`/LLVM, ABI estável, freestanding, self-hosting e debugger
  estão fora do contrato.
- O backend C é experimental; gerar C não significa suporte ao ABI/toolchain
  MSVC.
- Binários não prometem MSRV. A fonte só é validada com `rust-toolchain.toml`.
- “Suportado” exige instalação e smoke em host limpo, não apenas build no CI.

## Identidade e verificação

A tag `vX.Y.Z[-rc.N]`, as versões dos crates publicáveis, CLI, LSP e extensão
devem coincidir. `cargo run --locked -p xtask -- check-release-contract` impede
divergência antes do empacotamento. S3-B adicionará o manifest interno à mesma
cadeia; S3-D exige BLAKE3 externo, provenance e conjunto completo de assets
antes de tornar uma release pública.
