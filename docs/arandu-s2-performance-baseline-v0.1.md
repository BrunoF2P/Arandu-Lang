# Arandu S2-C — baseline de performance incremental v0.1

**Protocolo:** 1  
**Contrato executável:** `tests/perf/s2-baseline.txt`  
**Comando:** `cargo run --locked -p xtask -- check-project-performance`

## Decisão de gate

Tempos absolutos são publicados para tendência, mas não bloqueiam PR em runners
compartilhados. O gate funcional exige:

- zero execução de query no rebuild noop;
- p95 de no máximo 64 execuções para edição isolada de item ou bloco;
- registro estável em `4 paths / 2 FileIds vivos / 2 FileIds alocados` durante
  duas mil revisões do mesmo conjunto de inputs;
- crescimento de RSS de no máximo 256 MiB onde uma leitura comparável está
  disponível (`/proc/self/status` no Linux).

RSS é `unavailable` no Windows em vez de misturar working set, private bytes e
peak working set como se fossem a mesma métrica. Heap exato também permanece
indisponível sem instrumentar/substituir o allocator; RSS e contadores Salsa são
tratados separadamente.

No GitHub, a perna Linux de `S2 / Endurance` publica o relatório por 30 dias.
As regras de promoção e de encerramento/recriação de sessão estão em
[`arandu-s2-endurance-promotion-v0.1.md`](./arandu-s2-endurance-promotion-v0.1.md).

## Protocolo estatístico

- 3 execuções de warm-up;
- 9 amostras por cenário;
- mediana e p95 pelo nearest-rank;
- cenários cold, noop, edição de corpo importado e edição de bloco local;
- 2.000 revisões de endurance após as amostras;
- relatório escrito em `target/s2-performance-report.txt` com toolchain, SO,
  CPU e `GITHUB_SHA` (ou `local-working-tree`).

## Calibração inicial local

Coletada em 2026-08-21, Rust 1.97.1, Windows, Intel64 Family 6 Model 62,
working tree `codex/s2-roadmap`:

| Cenário | Mediana | p95 | p95 queries executadas |
|---|---:|---:|---:|
| cold | 607 µs | 797 µs | informativo |
| noop | 0 µs | 1 µs | 0 |
| edição de item importado | 210 µs | 337 µs | 9 |
| edição de bloco local | 310 µs | 416 µs | 11 |

Esses tempos não são promessa de hardware. A baseline reproduzível é o
protocolo; tendências finas exigem host controlado e histórico por commit.

## Retenção e Salsa

`RebuildLog::counts` mede execuções e validações observadas por janela. A API
pública da Salsa não expõe uma contagem completa de valores/memos retidos; por
isso o relatório não apresenta uma estimativa falsa. `registry_metrics`
distingue paths vivos, `FileId`s vivos e identidades historicamente alocadas.

Não foi habilitado LRU. Segundo a documentação da Salsa, o padrão conserva
valores sem limite, mas LRU remove valores e preserva chaves/metadados de
dependência; ele só será considerado se RSS e workload controlado demonstrarem
retenção problemática sem degradar early-cutoff.
