# RFC 0013: CTFE determinístico e `comptime` core via AMIR

- **Número da RFC:** 0013
- **Título:** CTFE determinístico e `comptime` core via AMIR
- **Autor(es):** Bruno e Equipe do Compilador Arandu
- **Data de Início:** 2026-09-12
- **Status:** `Draft`
- **Área Principal:** `Frontend` / `Middle-end` / `Incrementalidade`
- **PR da RFC:** N/A (In-Tree RFC)
- **Issue de Acompanhamento:** N/A

> Esta RFC continua sendo uma proposta. Exemplos de sintaxe são ilustrativos
> até a seção de gramática ser revisada e a RFC ser aceita. Nenhuma capacidade
> descrita aqui é apresentada como já implementada.

---

## 1. Resumo

Esta proposta define uma primeira etapa de avaliação de expressões Arandu em
tempo de compilação (CTFE), executadas sobre AMIR e integradas ao grafo
incremental. O objetivo é permitir que código puro e explicitamente requerido
em compilação produza valores constantes, sem introduzir uma linguagem de macro
separada nem efeitos ocultos no compilador.

O candidato a escopo da 0.1.9 é deliberadamente menor que uma plataforma completa
de metaprogramação:

1. Um domínio canônico de valores CTFE e um interpretador determinístico de um
   subconjunto documentado da AMIR.
2. `comptime` em expressões e blocos, `comptime if` e iteração finita sobre
   valores conhecidos em compilação.
3. Integração de parâmetros de valor com os const generics escalares já
   existentes, evitando mecanismos independentes e preservando compatibilidade.
4. Intrínsecos de layout como `@sizeOf` e `@alignOf`, calculados para o layout
   configurado do alvo suportado.
5. Memoização no Salsa, orçamento de execução, cancelamento cooperativo e
   diagnósticos estruturados.

`@typeInfo` estrutural amplo, introspecção de OS/arquitetura/capabilities,
modelo de memória virtual geral, `quote`, splicing, `@Derive`, geração de items,
inclusão de arquivos e JIT não fazem parte do primeiro núcleo. Cada um depende
de contratos adicionais e deve ser avaliado em etapa ou RFC própria.

## 2. Motivação

CTFE pode sustentar avaliação de constantes, especialização limitada por valores,
validação estática e, posteriormente, reflexão e geração de código. Seu valor
arquitetural está em reutilizar a semântica tipada da linguagem e não em compilar
um valor isolado antes do backend.

O projeto já tem componentes que ajudam — AMIR, queries Salsa, `DataLayout` e
const generics escalares —, mas eles não constituem ainda uma VM CTFE nem um
modelo completo de alvo. Em particular:

- `TargetInfo` do type checker atualmente expressa apenas a largura de ponteiro;
- `TargetConfig` no middle-end guarda `DataLayout`, não um triple canônico com
  OS, arquitetura, ABI e capabilities;
- const generics atualmente aceitam tipos inteiros escalares;
- `func_amir` é uma projeção sobre o lowering program-wide, não uma cadeia real
  de lowering incremental por instância.

Essas limitações orientam a divisão em etapas. Não se deve prometer que CTFE
evitará toda reexecução incremental: Salsa pode cortar propagação quando uma
saída permanece igual, mas a query de avaliação pode precisar rodar novamente
quando uma dependência semântica muda.

## 3. Guia proposto

Os exemplos abaixo demonstram a intenção da feature, não congelam a gramática.

### 3.1. Expressões e blocos

```arandu
let table_size: usize = comptime choose_table_size()

comptime {
    assert(table_size > 0)
}
```

Uma avaliação CTFE só pode chamar operações e funções admitidas pelo subconjunto
de execução. Chamadas com I/O, efeitos de runtime ou acesso ambiental ao sistema
de arquivos são rejeitadas; não são executadas parcialmente.

### 3.2. Decisões e iteração estáticas

```arandu
comptime if (TABLE_SIZE <= 256) {
    use_small_table()
} else {
    use_large_table()
}

comptime for index in 0..TABLE_SIZE {
    initialize_entry(index)
}
```

`comptime for` começa limitado a intervalos e agregados finitos conhecidos pelo
interpretador. Fuel limita também a expansão resultante; não há iteração
arbitrária ou permissão para travar o compilador.

### 3.3. Parâmetros de valor

```arandu
func matrix<T, comptime ROWS: usize, comptime COLS: usize>() {
    // dimensões conhecidas durante a instanciação
}
```

A forma exata da sintaxe permanece em aberto. A semântica deve se integrar aos
const generics escalares existentes e não criar uma segunda representação de
argumentos constantes. Mudanças à sintaxe atual exigem plano de compatibilidade.

### 3.4. Layout do alvo

```arandu
const bytes = @sizeOf(MyStruct)
const alignment = @alignOf(MyStruct)
```

Esses intrínsecos usam somente o layout do alvo selecionado e suportado pelo
backend. A sintaxe `target.os`, `target.arch`, `target.abi` ou
`target.has(feature)` não é definida nesta etapa: requer uma identidade de alvo
validada, além do layout.

## 4. Desenho e etapas propostas

### 4.1. CT.0 — decisões necessárias antes da implementação

Antes de mudar lexer/parser ou iniciar a VM, a RFC precisa fechar:

1. O domínio de `ConstValue`, incluindo representabilidade, igualdade, hashing e
   serialização determinística para os tipos aceitos.
2. Quais rvalues, operações, chamadas e instruções AMIR podem ser interpretados.
3. A política para overflow, divisão inválida, recursão, alocação e valores não
   inicializados.
4. Como parâmetros de valor interagem com const generics, monomorphização e
   inferência, sem quebrar programas existentes.
5. O modelo de alvo inicial e as operações de layout válidas para cada backend.
6. Chaves e dependências da query de avaliação, incluindo os limites reais do
   `lower_amir` program-wide atual.
7. Orçamento padrão, cancelamento, contexto CLI/LSP e diagnósticos públicos.

Se uma decisão exigir suporte que não existe, o item fica fora do núcleo até
essa dependência ser entregue; não deve ser simulado com dados do host.

### 4.2. CT.1 — valores e interpretador

O interpretador é uma função pura da AMIR, argumentos constantes, configuração
de alvo e orçamento. O conjunto inicial deve ser pequeno, explicitamente
enumerado e alinhado ao que a AMIR representa sem efeitos observáveis. O ponto de
partida recomendado é valores escalares e agregados imutáveis suportados pela
AMIR; ponteiros arbitrários, chamadas externas e efeitos ficam excluídos.

Uma operação não suportada retorna um erro CTFE estruturado com span; nunca
causa panic no compilador nem é silenciosamente tratada como constante.
Implementar um modelo completo de memória virtual à maneira de Miri não é
pré-requisito para esse subconjunto e não deve ser introduzido sem necessidade
demonstrada.

### 4.3. CT.2 — superfície de linguagem

Após o domínio de valores e a semântica de execução estarem testados, adicionar
as formas aprovadas de `comptime` em expressão/bloco, `comptime if` e iteração
finita. O type checker precisa rejeitar no ponto de origem construções que não
possam ser avaliadas com segurança, sem depender de falha tardia no backend.

Parâmetros `comptime` reutilizam a representação dos const generics atuais para
inteiros escalares no primeiro passo. Ampliação para tipos como argumentos,
valores arbitrários ou políticas é uma decisão futura, não implícita nesta RFC.

### 4.4. CT.3 — alvo, layout e reflexão mínima

O banco recebe uma descrição canônica e validada do alvo antes das queries
semânticas que dependem dela. A primeira superfície de introspecção limita-se a
operações de layout cujo resultado o compilador já calcula de forma confiável,
como `@sizeOf` e `@alignOf`.

O descritor deve separar identidade de alvo e `DataLayout`; ambos são dados de
entrada semânticos. OS, arquitetura, ABI e capabilities só podem ser expostos
quando forem obtidos de uma configuração explícita suportada — nunca inferidos
do host durante a avaliação.

Um `@typeInfo` amplo com campos, métodos, atributos ou acesso dinâmico por nome
fica para uma etapa posterior, com contrato próprio para identidade e visibilidade
de tipos.

### 4.5. CT.4 — Salsa, fuel e LSP

A avaliação é memoizada por query pura em `arandu_query`; crates de typeck, MIR e
backends não passam a possuir Salsa. A query deve depender de entradas
semânticas explícitas e retornar resultado estável e comparável.

Garantias exigidas:

- determinismo para as mesmas AMIR, argumentos, layout e configuração;
- limite de passos aplicado em cada operação/salto/chamada relevante;
- cancelamento cooperativo, especialmente durante análise interativa do LSP;
- nenhum I/O ou efeito global dentro da query;
- early-cutoff testado sobre o resultado, sem prometer que mudanças em
  dependências não reexecutam o interpretador;
- preservar a correção mesmo enquanto `func_amir` dependa do lowering
  program-wide. Granularidade por instância é melhoria arquitetural separada.

Fuel, defaults CLI/LSP e opções de configuração só são congelados após benchmark
e testes de responsividade; os números apresentados em versões anteriores desta
proposta eram exemplos, não contrato.

### 4.6. Critérios de saída do núcleo

O núcleo não está pronto para release até que haja testes que demonstrem:

1. Semântica e diagnósticos estáveis para todos os casos suportados e rejeitados.
2. Equivalência entre análise incremental e clean para os valores CTFE.
3. Resultados determinísticos entre execuções e hosts; resultados dependentes de
   layout variam conforme o layout-alvo selecionado, não o host.
4. Cancelamento e fuel funcionando sem bloquear indefinidamente CLI ou LSP.
5. Paridade com execução de runtime para o subconjunto puro compartilhado, onde
   ambos os modos forem definidos.
6. Regressões de invalidation/cutoff que comprovem quais queries são refeitas e
   quais consumidores downstream são preservados.
7. Diagnósticos novos registrados em `DiagCode`, catálogo e documentação de
   erros conforme as regras do workspace.

## 5. O que não faz parte desta RFC de núcleo

Os itens seguintes permanecem possibilidades futuras e exigem desenho separado:

- `quote`, `${...}`, geração de declarações, `@Derive` e DSLs;
- reflexão estrutural completa e `val.@field(name)`;
- APIs de target OS/arch/ABI/capabilities além do descritor suportado;
- inclusão de assets, `embedBytes`/`embedString` ou acesso indireto a arquivos;
- interpretação geral de ponteiros, memória virtual completa ou equivalência
  integral com Miri;
- execução JIT de CTFE.

Geração de itens precisa resolver higiene, atribuição de `SymbolId`, resolução de
nomes, re-typecheck, ciclos e invalidação; não é uma extensão pequena do avaliador
de constantes.

## 6. Invariantes e custos

1. A execução é determinística e não usa estado global mutável.
2. Queries são puras, não fazem I/O e permanecem em `arandu_query`.
3. Valores dependentes de layout usam dados explícitos do alvo e nunca o host.
4. Fuel/cancelamento são obrigatórios antes de habilitar CTFE no LSP.
5. A AMIR VM não pode divergir silenciosamente da semântica das instruções que
   interpreta; o subconjunto suportado fica documentado e testado.
6. O escopo da VM cresce por necessidade comprovada, sem antecipar alocadores,
   virtual memory ou otimizações específicas de desempenho.

O custo principal é manter o interpretador coerente com typeck, AMIR, layout,
ownership e incrementalidade. O ganho de ergonomia não justifica enfraquecer
diagnósticos, aumentar invalidações sem medição ou criar uma segunda linguagem de
metaprogramação antes do núcleo estar estável.

## 7. Alternativas consideradas

| Alternativa | Vantagem | Custo/risco | Situação |
| --- | --- | --- | --- |
| Avaliar AST diretamente | Começo aparentemente simples | Duplica semântica e precede contratos tipados/AMIR | Não recomendada como arquitetura final |
| Reutilizar somente folding atual | Escopo pequeno | Não executa blocos ou chamadas CTFE | Adequado como etapa de preparação, insuficiente como núcleo |
| VM geral com memória virtual desde o início | Base para ponteiros e programas mais ricos | Grande superfície de segurança antes de haver casos que a exijam | Adiada até necessidade demonstrada |
| Macros/geração de código na primeira entrega | Maior expressividade inicial | Higiene, resolução, ciclos e expansão complexos | Fora do núcleo |

## 8. Arte prévia

Zig, Rust/Miri, Rust procedural macros, Circle C++ e D oferecem experiências
relevantes para avaliar ergonomia, interpretação, reflexão e expansão. Esta RFC
não afirma superioridade sobre essas abordagens nem quantifica substituição de
macros; comparações futuras precisam de workloads e critérios reproduzíveis.

## 9. Questões em aberto

1. Qual é o conjunto inicial exato de tipos e operações de `ConstValue`?
2. `comptime` marca expressão/bloco/param, ou os const generics existentes
   continuam com sintaxe própria e apenas compartilham semântica?
3. Quais loops e chamadas são aceitos no primeiro interpretador e como o fuel é
   contabilizado de forma previsível?
4. Qual formato representa o triple e quais alvos têm codegen real suportado?
5. Quais consultas devem depender do resultado CTFE e como provar cutoff sem
   supor que a query não será reexecutada?
6. Quais intrínsecos mínimos oferecem valor sem comprometer estabilidade de
   `@typeInfo`?

## 10. Possibilidades futuras

Após o núcleo demonstrar semântica, segurança e incrementalidade, podem ser
propostas RFCs para reflexão estrutural, capabilities por alvo, inclusão
determinística de recursos e geração higiênica de itens. O avanço de cada etapa
depende de casos de uso concretos e de contratos específicos; nada disso é
prometido pela candidata 0.1.9.
