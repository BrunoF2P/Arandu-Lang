# RFC: A1 — Query System Incremental (Salsa) para o Arandu Compiler

Status: proposto · Fase 3 (v0.3) · Depende de: nada bloqueante (pode começar em paralelo com A2/A3)

## 1. Motivação

O `CompileSession` atual (Fase 2) já tem um precursor de memoização (`ParseCache`,
`PERF.4`) — um `HashMap<PathBuf, Arc<Program>>` que evita reparsing de arquivos entre fases.
Isso funciona bem para single-file/stdlib, mas não escala para:

1. **Multi-módulo real**: editar um arquivo hoje força reprocessar o grafo de dependência
   inteiro, não só o que mudou de fato.
2. **LSP/IDE responsivo**: sem invalidação fina, cada keystroke recompila o arquivo inteiro,
   inviabilizando feedback sub-100ms prometido no roadmap.
3. **Reuso de análise entre consumidores**: liveness, por exemplo, hoje seria recomputada
   independentemente pelo borrow checker (F2) e pelo register allocator (backend), sem
   nenhum mecanismo de compartilhamento.

Este RFC define como Salsa se encaixa **sem** introduzir um segundo sistema de identidade
paralelo ao que o compilador já tem (`SymbolId`, `TypeId`, `BlockId`, `TempId`, `LocalId`,
todos `IndexVec`-based) — que é o risco arquitetural mais sério de adotar uma engine de
incrementalidade pronta sem disciplina.

## 2. O 6º Invariante Arquitetural

Adicionar formalmente aos 5 invariantes existentes no roadmap:

> **6. Identidade Única sob Incrementalidade**: Salsa nunca introduz um sistema de
> identidade paralelo ao já existente no compilador. Toda `#[salsa::input]`/
> `#[salsa::tracked]` usa como chave os IDs nativos do Arandu (`FileId`, `SymbolId`,
> `BlockId`, `TypeId`) diretamente. Salsa atua como camada de memoização e invalidação
> sobre estruturas que já existem — nunca como dono de uma nova identidade que precisaria
> ser sincronizada com a identidade nativa.

Isso é o oposto do padrão de rust-analyzer (que mapeia IDs de Salsa para IDs de AST via
tabelas de tradução bidirecionais — fonte conhecida de dessincronização em sessões longas de
IDE, conforme a própria documentação do projeto reconhece).

## 3. Camada de Input — o que entra no grafo de dependências

```rust
// crates/arandu_query/src/db.rs

#[salsa::db]
pub trait ArandCompilerDb: salsa::Database {
    fn source_text(&self, file: FileId) -> Arc<str>;
    fn file_path(&self, file: FileId) -> Arc<PathBuf>;
}

#[salsa::input]
pub struct SourceFile {
    pub file_id: FileId,       // ID nativo já existente — não um novo tipo
    #[return_ref]
    pub text: Arc<str>,
}
```

`FileId` é o mesmo tipo que já circula em `Span`/diagnósticos hoje (visto em
`Diagnostic::ice(DiagCode::ICEGEN001, message, Span::new(0, 0, 0))` — `Span` já carrega
noção de arquivo). Nenhum ID novo é criado aqui.

**Durabilidade** (herdada do design já esboçado em sessões anteriores):

```rust
fn durability_for(file: FileId, is_stdlib: bool) -> salsa::Durability {
    if is_stdlib { salsa::Durability::HIGH } else { salsa::Durability::LOW }
}
```

## 4. Camada de Queries Derivadas — granularidade por bloco básico

Este é o ponto onde o Arandu vai além do que rust-analyzer/rustc oferecem. A cadeia
tradicional (`parse → resolve → type_check → lower_amir`) memoiza no nível de arquivo/item —
aqui, a cadeia desce até `BlockId` individual, porque o dataflow do compilador já opera
nessa granularidade desde a Fase 1 (`A9` — bitsets densos por bloco, `A5` — CFG em RPO).

```rust
// Cadeia de granularidade grossa (arquivo/função) — igual ao estado da arte hoje
#[salsa::tracked]
fn parse(db: &dyn ArandCompilerDb, file: SourceFile) -> Arc<Program> {
    arandu_parser::parse(&file.text(db))
}

#[salsa::tracked]
fn resolve(db: &dyn ArandCompilerDb, file: SourceFile) -> Arc<ResolvedProgram> {
    let program = parse(db, file);
    arandu_resolve::resolve(&program)
}

#[salsa::tracked]
fn type_check(db: &dyn ArandCompilerDb, file: SourceFile) -> Arc<TypeCheckResult> {
    let resolved = resolve(db, file);
    arandu_typeck::check(&resolved)
}

#[salsa::tracked]
fn lower_amir(db: &dyn ArandCompilerDb, file: SourceFile) -> Arc<AmirProgram> {
    let checked = type_check(db, file);
    arandu_mir::lower(&checked)
}

// A partir daqui, granularidade fina — o diferencial real.
// AmirFunc já é referenciado por SymbolId (o símbolo da função); cada bloco
// dentro dela já tem BlockId estável (dense IndexVec, RPO-ordenado).

#[salsa::tracked]
fn func_amir(db: &dyn ArandCompilerDb, file: SourceFile, func_sym: SymbolId) -> Arc<AmirFunc> {
    let program = lower_amir(db, file);
    program.funcs.iter()
        .find(|f| f.symbol == func_sym)
        .expect("símbolo de função deve existir no AMIR já resolvido")
        .clone_into_arc()
}

/// A query central da granularidade fina: o resultado do transfer function de
/// UM bloco básico, memoizado independentemente dos outros blocos da mesma função.
#[salsa::tracked]
fn block_dataflow_facts(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    func_sym: SymbolId,
    block: BlockId,
) -> DataflowFacts {
    let func = func_amir(db, file, func_sym);
    let bb = func.block(block);

    // Predecessores no CFG — Salsa rastreia essa leitura automaticamente,
    // então se um predecessor mudar, apenas os blocos dependentes recomputam.
    let pred_facts: Vec<DataflowFacts> = predecessors(&func, block)
        .map(|pred_id| block_dataflow_facts(db, file, func_sym, pred_id))
        .collect();

    compute_transfer(bb, &pred_facts)
}
```

**Consequência prática**: se o usuário edita uma linha dentro de `bb3` de uma função com 12
blocos, a invalidação se propaga assim — `source_text` muda → `parse`/`resolve`/`type_check`
recomputam (ainda grossos, isso é aceitável, são baratos) → `lower_amir` recomputa a função
inteira (o AMIR de uma função não é fatiável abaixo do nível de função sem reconstruir SSA) →
mas **`block_dataflow_facts` só recomputa para `bb3` e os blocos alcançáveis a partir dele em
RPO**, não para os 11 outros blocos que não dependem transitivamente da mudança. Isso é o
ganho real: rust-analyzer reprocessaria o dataflow da função inteira; aqui só a fração do CFG
que realmente pode ter mudado é recomputada.

## 5. Liveness compartilhada — um único ponto de verdade para dois consumidores

```rust
#[salsa::tracked]
fn liveness_facts(
    db: &dyn ArandCompilerDb,
    file: SourceFile,
    func_sym: SymbolId,
) -> Arc<LivenessMap> {
    // Reusa exatamente os mesmos bitsets densos (A9) já usados por
    // definite_init/move_checker — não uma segunda implementação de liveness.
    let func = func_amir(db, file, func_sym);
    compute_liveness_rpo(&func)
}
```

**Consumidor 1 — borrow checker (F2.1/F2.2)**: a janela de empréstimo de uma referência é o
live range do valor SSA que a representa — já discutido e fechado como decisão de design em
sessão anterior. `liveness_facts` fornece exatamente isso, sem F2 precisar de motor de
liveness próprio.

**Consumidor 2 — register allocator do backend Cranelift**: mesma query, mesmo cache. Hoje
esses dois caminhos (se F2 existisse) recomputariam liveness de forma independente — depois
do A1, os dois leem a mesma entrada memoizada, e uma mudança de código invalida os dois
consumidores de forma consistente e simultânea (nunca um vendo dado velho enquanto o outro já
recomputou — questão de correção, não só performance).

## 6. `DX.5` via tracing existente, não introspecção do motor Salsa

Rejeitado explicitamente: usar `lookup_ingredient()`/APIs internas de introspecção do Salsa
para reconstruir a cadeia causal de invalidação. Motivo: essas APIs são de baixo nível,
instáveis entre versões do crate, e specific ao motor interno — acoplar `DX.5` a elas
significa que um upgrade de versão do Salsa pode quebrar silenciosamente essa feature.

Em vez disso, reaproveitar `PERF.2`/`PERF.3` (já implementados): cada `#[salsa::tracked]`
já roda dentro de um span de `tracing` (`#[instrument]`), então basta estampar a **chave da
query** (ex.: `block_dataflow_facts{file=..., func=..., block=bb3}`) como campo estruturado
do span:

```rust
#[salsa::tracked]
#[tracing::instrument(level = "trace", target = "arandu_query", skip(db), fields(
    query = "block_dataflow_facts",
    file = ?file.file_id(db),
    func = ?func_sym,
    block = ?block,
))]
fn block_dataflow_facts(/* ... */) -> DataflowFacts { /* ... */ }
```

`DX.5` (`-Zexplain-query`) vira, então, um pós-processamento do log de spans já emitido pelo
`SelfProfile` — reconstrói a árvore de chamadas de query a partir do que já é gravado, sem
tocar em nenhuma API interna do Salsa. Mais simples, mais robusto a atualização de versão, e
reaproveita infraestrutura já testada (`PERF.2`/`PERF.3`, 22 funções já instrumentadas).

## 7. Determinismo de diagnósticos sob avaliação paralela de queries

Salsa pode avaliar queries independentes em paralelo (dependendo do runtime configurado).
Isso não afeta o **valor** de cada query (pura, determinística por definição), mas pode
afetar a **ordem de chegada** de diagnósticos coletados durante avaliação paralela, se eles
forem simplesmente empilhados numa lista global conforme cada thread termina.

Regra fixada, coerente com a garantia `DET` (builds byte-a-byte reproduzíveis) já existente
no roadmap:

```rust
/// Diagnósticos de TODAS as queries são coletados independentemente, depois
/// ordenados uma única vez antes de qualquer output (CLI ou LSP) — nunca
/// confiar em ordem de conclusão de thread.
fn finalize_diagnostics(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags.sort_by(|a, b| {
        a.span.file_id.cmp(&b.span.file_id)
            .then(a.span.start.cmp(&b.span.start))
            .then(a.code.cmp(&b.code)) // desempate final estável
    });
    diags
}
```

Isso precisa ser aplicado em **todo** ponto de saída de diagnóstico (CLI, LSP, testes de
golden output) — não é responsabilidade de cada query individual ordenar nada, é
responsabilidade do consumidor final antes de apresentar.

## 8. Migração — passos concretos

```
1. Adicionar `salsa` e `blake3` como dependências do workspace.
2. Criar crate arandu_query/ com ArandCompilerDb + SourceFile (#[salsa::input]).
3. Portar parse/resolve/type_check/lower_amir como #[salsa::tracked], delegando
   para as funções já existentes em arandu_parser/arandu_resolve/arandu_typeck/arandu_mir
   (nenhuma lógica nova, só wrapping em query).
4. CompileSession::ParseCache é substituído pela query parse() — remover o
   HashMap manual, Salsa assume a memoização.
5. Implementar block_dataflow_facts/liveness_facts em cima do que já existe em
   move_checker.rs/definite_init.rs (reaproveitar os bitsets A9, não reescrever).
6. Adicionar #[tracing::instrument] com os campos estruturados (fields) descritos
   na seção 6, em todas as queries tracked.
7. Implementar finalize_diagnostics() e chamar em todo ponto de saída (CLI/LSP/testes).
8. Só depois de 1-7 estáveis: DX.5 (-Zexplain-query) como pós-processamento de log.
9. Watch mode (CLI --watch) via notify + próxima revisão do Salsa runtime.
```

## 9. Riscos e verificações antes de considerar A1 fechado

```
[ ] Confirmar que Salsa (versão escolhida) permite `#[salsa::tracked]` retornando
    Arc<T> onde T contém IndexVec/tipos do Arandu sem exigir Clone profundo
    a cada invalidação (verificar overhead de clone em AmirFunc grandes)
[ ] Teste de regressão: editar uma linha dentro de bb3 de uma função com N blocos,
    confirmar via contador de invocação de query que SÓ bb3 e blocos dependentes
    recomputam — não a função inteira nem os outros blocos
[ ] Teste de determinismo: rodar a mesma compilação com paralelismo habilitado
    múltiplas vezes, confirmar que finalize_diagnostics() produz sempre a
    mesma ordem de saída
[ ] Teste de identidade: confirmar que nenhuma query usa `#[salsa::interned]`
    para reintroduzir um ID paralelo a SymbolId/TypeId/BlockId — grep por
    salsa::interned no crate novo deveria retornar zero ou justificar exceção
[ ] DX.5: confirmar que o pós-processamento de log reconstrói corretamente
    uma cadeia de invalidação de exemplo (source_text dirty → parse dirty →
    resolve dirty → type_check clean-por-durability, se aplicável)
```

## 10. Definition of Done

```
[ ] arandu_query criado, ArandCompilerDb + SourceFile como único input
[ ] parse/resolve/type_check/lower_amir como queries tracked, ParseCache removido
[ ] block_dataflow_facts com granularidade de bloco confirmada por teste
[ ] liveness_facts compartilhada, usada por (quando existir) F2 e pelo backend
[ ] Todas as queries tracked com tracing::instrument estruturado
[ ] finalize_diagnostics() aplicado em todos os pontos de saída
[ ] 6º Invariante documentado no roadmap master, ao lado dos outros 5
[ ] cargo test --workspace passa; teste de granularidade de invalidação passa
```
