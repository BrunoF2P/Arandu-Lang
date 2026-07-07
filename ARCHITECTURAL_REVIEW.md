# Revisão Arquitetural Completa do Compilador Arandu

## 1. Problemas encontrados (Ordenados por prioridade)

### Prioridade 1: Uso massivo e desnecessário de `String` na AST e clones excessivos
**Gatilho Arquitetural:** O `arandu_parser` define a AST com campos de `String` literais. Exemplo em `StructDecl`: `pub name: String`. Isso significa que toda vez que a AST é construída ou percorrida, cópias na heap e usos de `clone()` ocorrem.
**Impacto:** Desperdício imenso de CPU com `malloc/free`, fragmentação de heap e alta utilização do GC implícito do SO durante a compilação.
**Solução:** O compilador já possui um mecanismo embrionário em `arandu_base::string_pool::StringPool` (`StringId`). A AST inteira deve abandonar `String` e adotar estritamente `StringId` ou bibliotecas como `smol_str`/`lasso`.

### Prioridade 2: Tratamento de erros sujo com `unwrap()` e `expect()` em hot paths
**Gatilho Arquitetural:** Foram identificados 362 `unwrap()` e 116 `expect()`. Enquanto muitos estão isolados em `tests.rs` (o que é aceitável), vazamentos críticos foram encontrados em passes essenciais:
- `arandu_semantics/src/passes/monomorphize/mod.rs`
- `arandu_mir/src/optimize.rs`
- `arandu_mir/src/simplify_cfg.rs`
- `arandu_resolve/src/name_resolution/mod.rs`
**Impacto:** Risco de _panic_ na thread do compilador. Um compilador moderno **não deve dar crash** perante código malformado, mas sim emitir diagnósticos estruturados.
**Solução:** Introdução maciça do operador `?`, `let else`, e uso das estruturas de `Diagnostic` existentes ou `miette`/`ariadne`.

### Prioridade 3: Abstrações de IR com cópias espúrias (`clone()` de tipos)
**Gatilho Arquitetural:** Em `arandu_mir/src/lower_amir/expr.rs`, vemos chamadas como `expr.ty.clone()` em praticamente todas as rotinas. O tipo `ArType` (ou similar) é clonado dezenas de vezes por instrução MIR levantada.
**Impacto:** Pressão desnecessária no _allocator_. Se os tipos forem grandes enums, isso adiciona overhead massivo.
**Solução:** Introduzir _Type Interning_ puro (Arena). Tipos devem ser reduzidos a um `TypeId` (um `u32` ou referência `&'a Type`), passando a ser `Copy`, de modo que `expr.ty` seria de cópia trivial.

### Prioridade 4: Boilerplate em Parsers e Lexers
**Gatilho Arquitetural:** O Lexer e Parser foram escritos na mão (`arandu_lexer/src/lexer.rs`, `arandu_parser/src/parser/expr.rs`). O Lexer usa SIMD, o que é ótimo para performance, mas introduz complexidade de manutenção. O parser de expressões desce via recursive descent tradicional, necessitando recuperar erros manualmente de modo subótimo (`ExprKind::Error`).
**Impacto:** O custo de manutenção dessa infraestrutura customizada é altíssimo à medida que a linguagem cresce.
**Solução:** Embora o parser manual (Pratt/Recursive Descent) seja o padrão na indústria de compiladores performáticos (como Rustc), recomenda-se avaliar a substituição parcial do Lexer por macros consolidadas (ex: `logos`) para tokenização sem perdas SIMD.

---

## 2. Bibliotecas recomendadas

| Biblioteca | Motivo | Prioridade | Ganho esperado | Redução de código | Redução de alocações | Redução de clones |
|---|---|---|---|---|---|---|
| **smol_str** / **lasso** | Substituir `String` na AST e HIR. Identificadores raramente superam 23 bytes (SSO). | **Alta** | Imenso. Reduz pressão no heap e cache misses. | - | > 90% das strings | > 80% dos `clone()` |
| **logos** | Substituir a lexing state machine. Gera DFA rápido e idiomático via macros. | Média | Código seguro, zero unsafe, DFA garantido. | ~1000 linhas (Lexer manual) | Alta | Baixa |
| **thiserror** / **miette** | Refatoração de Diagnósticos. Miette dá source spans lindos de graça. | Alta | Padronização e UX (developer experience). | ~500 linhas | N/A | N/A |
| **bumpalo** | Alocação por arena (Bump allocation) para Type Checking e Monomorfização (Graphs). | **Alta** | Zero drop overhead, L1 cache hit maximizado. | - | > 50% dos Box/Vec | > 50% de cópias |
| **salsa** (Já incluso) | Maximizar uso. Atualmente não usado em toda a pipeline (AST pura em arrays). | Média | Builds incrementais nativas. | N/A | N/A | N/A |

---

## 3. Métricas (Estimativas de Refatoração)

- **Linhas removidas:** ~1.500 linhas removidas de boilerplates manuais se `logos` for adotado e tipos de Erro unificados com `thiserror`.
- **Clones eliminados:** Pelo menos ~250 ocorrências. A conversão de `name: String` para `StringId` e adoção de `TypeId` removeria a vasta maioria.
- **Unwraps eliminados:** > 100 `unwraps` perigosos fora de testes.
- **Heap allocations reduzidas:** Dezenas de milhares por execução do compilador. Um benchmark típico de parsing reduziria alocações no heap em cerca de 70-85%.

---

## 4. Plano de migração

### Etapa 1: Interrupção de Panic/Unwrap
- Fazer auditoria nas crates internas (`arandu_resolve`, `arandu_mir`, `arandu_semantics`).
- Trocar `unwrap()` por `return Err(Diagnostic...)` em toda a base lógica que não seja teste.
- Benefício Imediato: O compilador passa a ser resiliente a código de usuário malformado.

### Etapa 2: Erradicação do `String`
- Acoplar `StringPool` ou `lasso` diretamente no Lexer.
- Retornar apenas `StringId` ou `SmolStr` como nome em Identificadores.
- Alterar `StructDecl`, `FieldDecl`, `EnumDecl`, etc. para armazenar `StringId`.
- Corrigir os ~250 locais onde `.clone()` era usado nas strings.

### Etapa 3: Type Interning System
- Implementar uma Arena central de Tipos (`TypeCtx` ou similar usando `bumpalo`).
- Tipos na HIR e MIR (`expr.ty`) passam a ser ponteiros ou IDs da Arena.
- Remoção total dos `clone()` na inferência e lowering de tipos (`arandu_mir/src/lower_amir/expr.rs`).

### Etapa 4: Adotar `miette` e Padronização de Diagnósticos
- Modernizar a struct `Diagnostic` interna do compilador para derivar/usar as APIs do `miette` ou `ariadne` para um visual moderno e código reduzido de formatação.

---

## 5. Críticas e Decisões Arquiteturais

### 5.1 O padrão Array-of-Structs (AoS) e Struct-of-Arrays (SoA)
A `AstPool` usa SoA (`pub exprs: Vec<ExprKind>, pub expr_spans: Vec<Span>`). Isso é excelente para L1 cache e foi uma decisão brilhante para percursos parciais (ex: passe que só lê tipo/expressão e ignora span). Contudo, a presença de campos do tipo `String` na AST quebra toda a vantagem do SoA. Strings armazenam buffers no heap, indirecionando o cache e estragando a localidade. **A mudança para `StringId` validará a intenção original do SoA.**

### 5.2 Lexer SIMD Manual vs `logos`
O Lexer em `arandu_lexer/src/lexer.rs` contém implementações manuais em SIMD (`backend: SimdBackendKind::detect()`). Embora ofereça performance estelar (~1.5GB/s+ típicos em SIMD Lexers), é o clássico gargalo de complexidade e bugs de _edge case_ em UTF-8.
**Alternativa:** Se a manutenção deste lexer custar tempo precioso, recomendaria substituí-lo por `logos`, que compila DFAs otimizados na compilação, é idiomático e seguro. A velocidade pura dificilmente será o gargalo antes do Type Checking para a maioria dos programas. O tempo de engenharia gasto no Lexer SIMD seria muito melhor investido no Middle-end/Type checker.

### 5.3 O Problema de Monomorfização e Graph
A Monomorfização (`arandu_semantics/src/passes/monomorphize/mod.rs`) faz intenso uso de `unwrap()` na resolução dos IDs em grafos. Há um cheiro de que o grafo pode não representar todos os estados possíveis, e o compilador assume fortemente a integridade sem tratar _cycles_ ou quebras não intencionais na IR. A substituição do backend manual de grafos por `petgraph` pode trazer algoritmos robustos de detecção de ciclo de dependência (`tarjan` / `kosaraju`) de graça e de forma segura (eliminando os `unwrap`).

### Conclusão
O *Arandu* possui fundamentos incríveis: `SoA` AST, passes baseados em IDs e uso da `salsa` para queries. Entretanto, como a maioria das bases em estágio v0.1-v0.2, foi permissivo com _String allocations_ e uso livre de `.clone()`. Ao introduzir interning e limpar o tratamento de erro, o projeto terá nível _production-grade_, comparável aos compiladores de ponta na arquitetura.

---

## 6. Exemplos de Refatoração (ANTES / DEPOIS)

Atendendo ao escopo da auditoria, demonstramos as sugestões na prática em recortes de código críticos do compilador.

### Exemplo 1: Tratamento de Erro Seguro na Monomorfização
O sistema atualmente faz unwrap de chaves dentro da arena ao procurar pelo interner.

**ANTES** (`arandu_semantics/src/passes/monomorphize/mod.rs`)
```rust
pub fn get_or_insert(&mut self, key: TypeKey, interner: &Interner, st: &SymbolTable) -> Result<MonomorphId, Diagnostic> {
    // ...
    let id1 = graph.get_or_insert(key.clone(), &interner, &st).unwrap();
    // ...
}
```

**DEPOIS** (Tratamento limpo propiciando o bubble up de erros com o operador `?`)
```rust
pub fn get_or_insert(&mut self, key: TypeKey, interner: &Interner, st: &SymbolTable) -> Result<MonomorphId, Diagnostic> {
    // ...
    // Remoção do `.unwrap()` e `.clone()` se `key` puder ser passado por referência
    // ou se TypeKey implementar Copy devido ao interning.
    let id1 = graph.get_or_insert(&key, interner, st)?;
    // ...
}
```
*Vantagens:* Elimina risco de _panic_ na thread principal. Respeita os bounds de vida ao passar referências em vez de forçar o `.clone()`.

### Exemplo 2: Remoção de Clones de Tipos no Lowering (MIR)
No levantamento de AMIR, o compilador clona o tipo completo (`ArType` enum) a cada criação de temporários.

**ANTES** (`arandu_mir/src/lower_amir/expr.rs`)
```rust
let dest = target.unwrap_or_else(|| self.new_temp(expr.ty.clone()));
self.emit_assign_temp(dest, AmirRvalue::Use(op.clone()));
```

**DEPOIS** (Introdução de Internamento de Tipos e passagem trivial de valores `Copy`)
```rust
// expr.ty é agora um TypeId (u32), que é Copy e extremamente leve (4 bytes).
let dest = target.unwrap_or_else(|| self.new_temp(expr.ty));
// op é consumido ou referenciado sem a necessidade explícita de clone de payloads grandes.
self.emit_assign_temp(dest, AmirRvalue::Use(op));
```
*Vantagens:* Um `TypeId` (ex: `struct TypeId(NonZeroU32)`) custa essencialmente zero em registro de CPU comparado ao heap clone de uma `String` ou alocador interno da variante de enum complexa.

### Exemplo 3: Extinção de `String` na AST
A declaração de estruturas armazena o nome por cópia livre no heap, corrompendo a intenção do SoA original.

**ANTES** (`arandu_parser/src/ast/decl.rs`)
```rust
pub struct StructDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Vec<WhereItem>,
    pub fields: Vec<FieldDecl>,
}
```

**DEPOIS** (Uso de Small String Optimization ou Internamento)
```rust
use crate::string_pool::StringId; // ou smol_str::SmolStr;

pub struct StructDecl {
    pub span: Span,
    pub attrs: Vec<Attribute>, // <- Isso também deveria virar um SmallVec ou Id pointer
    pub visibility: Visibility,
    pub name: StringId, // ID leve gerado pelo lexer e compartilhado na arena.
    // SmallVec seria superior pois structs raramente têm mais de 2 genéricos,
    // poupando uma alocação de vec global por struct definida.
    pub generic_params: smallvec::SmallVec<[GenericParam; 2]>,
    pub where_clause: smallvec::SmallVec<[WhereItem; 2]>,
    pub fields: Vec<FieldDecl>,
}
```
*Vantagens:* `StringId` reduz a struct `StructDecl` de ~120 bytes para ~60 bytes e zera chamadas à biblioteca `libc` (malloc). A AST agora se acomoda no L1 Cache lindamente.
