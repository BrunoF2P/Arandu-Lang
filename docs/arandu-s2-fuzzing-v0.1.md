# S2-D — Fuzzing adversarial e regressões permanentes

## Decisão

Arandu separa descoberta probabilística de garantia de merge:

- `arandu_fuzz` executa campanhas semanais advisory com libFuzzer;
- `arandu_fuzz_support` contém a implementação compartilhada dos targets;
- `tests/fuzz-regressions/manifest.tsv` é o corpus pequeno, versionado e obrigatório;
- `cargo run --locked -p xtask -- check-fuzz-regressions` executa cada par
  seed/target em processo isolado no gate Linux e Windows.

Essa forma segue a recomendação do [OSS-Fuzz para integração ideal](https://google.github.io/oss-fuzz/advanced-topics/ideal-integration/):
targets vivem no repositório, compilam com o projeto e crashes importantes
viram regressões contínuas. Também usa os mecanismos de corpus e minimização
documentados pelo [libFuzzer](https://llvm.org/docs/LibFuzzer.html) sem fazer o
merge depender de uma campanha aleatória.

## Contrato e limites

| Limite | Gate regressivo | Campanha advisory |
| --- | --- | --- |
| Entrada | 64 KiB | `-max_len=65536` |
| Tempo | 2 s por seed/target; processo encerrado | `-timeout=2`; 30 min por target |
| Memória | isolamento por processo | `-rss_limit_mb=1024` |
| Efeitos | I/O apenas no `xtask` | corpus e artefatos pertencem ao harness |

`catch_unwind` existe somente no worker de regressão para transformar panic em
falha legível. Nenhuma query captura panic, lê corpus, consulta o filesystem ou
produz telemetria observável.

## Cobertura inicial

Os targets cobrem lexer, equivalência SIMD, CST/parser com recuperação,
programas estruturados, pipeline `syntax_tree → parse → resolve → type_check →
lower_amir` sem backend e grafos de import cíclicos consultados concorrentemente
por snapshots. O corpus inicial registra UTF-8 truncado, string/comentário
abertos, nesting, tipos recursivos, CFG adversarial e ciclos de módulos.

Cada linha do manifesto contém caminho, targets, origem e risco/bug. O runner
recusa caminhos fora do corpus, conteúdo duplicado, targets desconhecidos,
encoding ausente e seeds acima do limite.

## Promoção de uma descoberta

1. Reproduzir o artefato no target que falhou.
2. Minimizar com `cargo fuzz tmin <target> <artefato>`; para reduzir corpus,
   usar `cargo fuzz cmin <target>`.
3. Criar uma `.seed` com `encoding=utf8` ou `encoding=hex` e uma linha no
   manifesto explicando origem e bug/risco coberto.
4. Executar `cargo run --locked -p xtask -- check-fuzz-regressions`.
5. Corrigir o crate proprietário; o seed permanece depois da correção.

Crashes não são copiados em massa: minimização e deduplicação são parte do
processo de promoção. Artefatos brutos continuam no workflow advisory para
triagem.
