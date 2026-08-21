# S2-E — Gate de endurance e promoção

## Resultado

O job `S2 / Endurance` é uma matriz Linux/Windows dependente de `S1 / Recovery`.
Em todo pull request ele executa:

1. corpus de projetos e equivalência incremental ↔ rebuild limpo;
2. 10 mil operações de churn, identidades geracionais e checkpoints;
3. budgets funcionais de performance/retenção;
4. corpus adversarial regressivo isolado;
5. stress LSP de 2 mil mudanças, condensadas em 100 commits Salsa, com snapshots
   em workers e reabertura geracional de documento.

O teste LSP executa o servidor diretamente como biblioteca do binário. Ele não
inicia VS Code nem Extension Host; a integração visual continua no roadmap
L0–L3 do editor.

## Proteção da `main`

Depois do primeiro PR verde, configurar como checks obrigatórios:

- `S0 / Gate`;
- `S1 / Recovery (ubuntu-latest)`;
- `S1 / Recovery (windows-latest)`;
- `S2 / Endurance (ubuntu-latest)`;
- `S2 / Endurance (windows-latest)`.

Os nomes são parte do contrato: renomear um job exige atualizar primeiro a
ruleset do GitHub, para não bloquear merges esperando um check inexistente.
Jobs de fuzz aleatório, toolchain futura e tempos absolutos permanecem advisory.

## Baseline e tendência

O runner Linux publica `target/s2-performance-report.txt` por 30 dias. O
artefato contém commit, toolchain, SO, CPU, medianas, p95, contadores de queries,
registro de identidades e RSS quando disponível. Tempos absolutos não bloqueiam
PR em hardware compartilhado; os budgets funcionais amplos continuam
obrigatórios e estão versionados em `tests/perf/s2-baseline.txt`.

Uma regressão de tempo só vira limite fino depois de repetida em host controlado
e atualizada por PR revisado. Nunca se aceita snapshot/baseline automaticamente.

## Limitações e encerramento de sessão

- `FileId` é monotônico e nunca reutilizado dentro da DB. O crescimento do total
  histórico alocado é intencional; paths e IDs vivos devem permanecer limitados.
- `DocumentId` e `AnalysisRevision` são geracionais. Handles antigos devem
  falhar, não apontar para objetos novos.
- Salsa pode reter chaves e metadata mesmo com LRU; a API pública não fornece
  contagem completa de memos. O gate reporta somente métricas observáveis.
- RSS é comparável no Linux via `/proc/self/status`; no Windows permanece
  `unavailable`, evitando comparar métricas de memória semanticamente distintas.
- A sessão deve ser encerrada/recriar a DB quando o projeto raiz muda, quando a
  política do produto exige liberar todo o histórico de IDs/metadata ou quando
  um limite operacional futuro, medido em host controlado, for alcançado.
- Recriar a DB nunca ocorre silenciosamente durante edição normal e nunca pode
  fazer um handle antigo resolver na nova sessão.

## Critério de promoção Gold

S2 é `gold` local quando todas as validações do repositório e o runner de
endurance passam. Torna-se `gold` na branch principal após:

1. o PR ser aprovado e integrado;
2. as duas pernas de `S2 / Endurance` ficarem verdes na `main`;
3. os cinco checks acima estarem obrigatórios na ruleset.
