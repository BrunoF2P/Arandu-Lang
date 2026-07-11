# Arandu — Plano Estratégico do Compilador v0.1

**Documento de síntese.** Consolida análise estrutural, backlog de bugs, pesquisa acadêmica e decisões de arquitetura.

| Documento | Papel |
|-----------|--------|
| [arandu-compiler-roadmap-v0.1.md](./arandu-compiler-roadmap-v0.1.md) | Checklist executivo, fases, DiagCodes, grafo de dependências |
| Este arquivo | **Por quê**, riscos, decisões fixas, pesquisa aplicável, bugs priorizados |
| [arandu-ir-architecture-v0.1.md](./arandu-ir-architecture-v0.1.md) | Referência técnica AHIR/AMIR |
| [arandu-amir-v0.1.md](./arandu-amir-v0.1.md) | Contrato AMIR + **invariantes formais** (§ Invariantes) |

---

## 1. Avaliação do estado atual

### 1.1 O que já está maduro

| Área | Avaliação |
|------|-----------|
| Separação por crates | Correta — lexer, parser, semantics, cli |
| Parser + recovery | Forte — goldens, contratos, semicolon insertion |
| Name resolution | Muito bom — 2 passes, namespaces, sugestões |
| AHIR/AMIR split | Decisão industrial (Rust HIR/MIR, Swift SIL) |
| Goldens (AST/HIR/AMIR) | Acima da média de projetos indie |
| Roadmap por fase semântica | Sustentável — não só “parser/checker/backend” |
| Disciplina de escopo | Raro: saber o que **não** implementar ainda |

### 1.2 Onde está o risco real (ordem) — atualizado pós F2/A3/T3

1. **Export/privacy multi-módulo** — `exported_symbols` deve filtrar `public` (root); stdlib e pacotes dependem disso.
2. **Hotspots typeck/mono** — `call.rs`, `expand.rs` ainda grandes; modularizar ao tocar (não monólito único de entrada).
3. **Residuals F2** — auto-ref, `CalleeArgModes`→`&T`, exclusive `BorrowMut` (janelas OSSA).
4. **Otimização middle-end** — O1 + SCCP/simplify parcial; inlining/DSE globais depois.
5. **Layout/ABI named types no Cranelift** — TODO multi-value residual.
6. **GenArena tipada / SL_R** — adiados honestamente (não bloqueiam F2/A3 gold).

O maior risco **não** é lexer nem parser. Typeck já está modularizado em subpastas; o risco é **export stub** e monólitos de mono/lower/backend.

---

## 2. Decisões arquiteturais fixas (v0.x)

Estas decisões absorvem a análise estrutural + papers. **Não reabrir** sem RFC.

| # | Decisão | Implicação |
|---|---------|------------|
| D1 | **Ownership vive no AMIR**, não no type checker | Checker só tipa; move/OSSA/gen-check são passes no CFG |
| D2 | **CFG + SSA antes de ownership completo** | Ordem: AHIR → AMIR → definite init → move → opt → backend |
| D3 | **Checker intraprocedural primeiro** | Sem NLL, sem Polonius-Datalog, sem interprocedural cedo |
| D4 | **Insight Polonius, não implementação Datalog** | Loans/origins como **dataflow esparso no CFG**, nativo em Rust |
| D5 | **Aliasing por proveniência** (influência Tree Borrows) | Não copiar Stacked Borrows; preparar `unsafe` com árvore de proveniência |
| D6 | **Backend C burro primeiro** | AMIR → C quase 1:1; sem otimização no backend inicial |
| D7 | **Sem Rowan/CST agora** | Manter spans, trivia, comentários preserváveis para migração futura |
| D8 | **Efeitos no AMIR como flags**, não effect system v1 | `can_throw`, `can_suspend`, etc. — prepara async/Result sem redesign |
| D9 | **Identidade de pesquisa (v0.2+)** | “Ownership pragmático por alcançabilidade + fallback geracional” — não copiar Rust nem Vale integralmente |

### 2.1 Tríplice nullable — **fechado**

Não misturar os três modelos (lição Swift/histórico):

| Conceito | Sintaxe / tipo | Semântica | Checker |
|----------|----------------|-----------|---------|
| **Nullable** | `T?` | Referência/opcional **na heap ou handle** — pode ser `nil` | `ArType::Nullable` |
| **Option** | `Option<T>` | Valor **semântico** opcional (algébra de tipos) | `ArType::Option` |
| **Result** | `Result<T, E>` | **Computação** com erro tipado | `ArType::Result` |

Regras:

- `?` em expressão: só `Result` e `Option` (não `Nullable` sozinho).
- `?.` / `??`: só sobre `Nullable` / safe navigation.
- `err` namespace (`err.new`) ≠ tipo `Err` primitivo — documentado; namespace para construção, `Err` para tipo de erro.

### 2.2 Copy types (v0.1)

**Copy** (move não obrigatório; `copy` no AMIR):

- inteiros, float, bool, char, byte
- raw ptr (quando existir)
- referência `shared` (leitura)

**Non-copy** (OSSA `move` / `destroy`):

- struct `own`, arrays owned, valores heap

Sem inferência automática de `Copy` em v0.1 — lista explícita no checker + AMIR.

### 2.3 Módulos e visibilidade (decidir antes do LSP)

Modelo alvo (proposta — fechar em RFC curta):

- `module path.to.file` no topo
- `import io` / `import err` (já existe)
- Visibilidade: `public` / `private` (v0.2); `internal` (v0.3)
### 2.4 Melhorias de DX no Resolvedor/Typechecker (Fase 3)

1. **Parâmetros Genéricos Padrão**: Permitir a omissão de tipos de alocadores (ex: `Vec<T>` em vez de `Vec<T, GlobalAllocator>`). Requer suporte no AST, parser e expansão de tipos omitidos no typechecker durante a instanciação.
2. **Açúcar Sintático para Enums (Dot-Notation)**: Permitir expressões curtas `.Ok(val)` e `.Some(val)` em expressões de atribuição, argumentos de função e retornos. Requer inferência bidirecional baseada no tipo esperado do contexto (expected type).

---

## 3. Pesquisa acadêmica — o que usar e o que evitar

| Paper / linha | Usar no Arandu | Não fazer agora |
|---------------|----------------|-----------------|
| **Polonius** (origins/loans) | Modelar facts no CFG; “de onde veio o empréstimo” | Engine Datalog; expectativa de performance do rustc alpha |
| **Tree Borrows** (PLDI 2025) | Filosofia de aliasing em `unsafe`; leitura obrigatória | Implementar semântica operacional completa |
| **Reachability types** | Direção de ownership **sem lifetimes** na superfície (v0.3+ pesquisa) | Implementar sistema formal completo |
| **Typestate (TSOP)** | **Light typestate** em recursos (`File<Open>`) + defer/RAII | Typestate geral com aliasing |
| **Algebraic effects (Affect)** | Separar interface/implementação na stdlib; flags no AMIR | Handlers + effect rows no tipo |
| **Liquid / refinement types** | Newtypes (`NonEmpty[T]`, `Positive`) sem SMT | Z3 no hot path do compilador |

### 3.1 Prioridade de leitura para o time

1. Polonius blog + modelo CFG-native (insight, não código)
2. Tree Borrows (unsafe / otimização de aliasing)
3. Reachability types (diferencial futuro)
4. Typestate gradual (protocolos de uso)

---

## 4. Riscos e mitigação

| Risco | Mitigação |
|-------|-----------|
| Type checker monólito | Modularizar **agora** (§5) |
| AMIR informal | Invariantes em [arandu-amir-v0.1.md](./arandu-amir-v0.1.md); validador no crate |
| Retorno `Result<T,E>` | Fase **E** fechada — só `Result` / `Option`; parser **P006** rejeita tupla-erro |
| Generational fallback opaco | **O004** sempre (inclusive release); nota com local e motivo |
| Backend antes do AMIR estável | C só após D2 + G + F1 + M1 |
| Performance do frontend | Arena + `SymbolId` + IndexVec (v0.2); ver [arandu-hir-indexvec-rfc.md](./arandu-hir-indexvec-rfc.md) |
| Match não exaustivo | Checker (T024) + `SwitchInt` no AMIR (`match_lower.rs`) |

---

## 5. Modularização do type checker (crítico — v0.1)

**Meta:** impedir que `check.rs` vire monólito antes de generics e flow diagnostics.

Estrutura alvo:

```text
type_checker/
├── mod.rs
├── context.rs          # TyCtx, return stack, loop depth
├── types.rs            # ArType, lowering de TypeExpr
├── constraints.rs      # ConstraintOrigin, flow diagnostics
├── infer.rs            # synth_expr (renomear de synth.rs)
├── check_stmt.rs       # check_stmt, check_block (extrair de check.rs)
├── coercions.rs        # widen, literal absorption
├── result.rs           # Result/Option/?, T019
├── patterns.rs         # match, if-is
├── methods.rs          # self receiver, method calls
├── interfaces.rs       # T017, T018 (v0.1 T)
├── generics.rs         # instanciação (v0.1 T)
├── nullable.rs         # T?, ?., ??
├── prelude.rs          # stdlib io/err (substituir hardcode em check.rs)
└── diagnostics.rs      # helpers de mensagem
```

**Critério de pronto:** nenhum arquivo > ~800 linhas; `check.rs` vira orquestrador fino.

**Dependência:** pode começar em paralelo com bugs críticos — refatoração sem mudança de comportamento + testes existentes.

---

## 6. AMIR como centro semântico

Tudo converge para AMIR:

| Feature fonte | Onde termina |
|---------------|--------------|
| `?`, `catch`, `??` | CFG + assigns explícitos |
| `defer` / `errdefer` | blocos de cleanup no CFG |
| `match` | `SwitchInt` / branches |
| Ownership | instruções OSSA |
| Gen fallback | chamadas inseridas + metadata O004 |

**Proibido:** nova semântica de memória só no type checker ou só no parser.

Invariantes obrigatórios: ver [arandu-amir-v0.1.md § Invariantes](./arandu-amir-v0.1.md#invariantes-formais-v01).

---

## 7. Backlog de bugs (priorizado)

Integração da análise de código + sessão de correção AMIR/namespace.

### 7.0 Crítico — Fase 2 (correções de causa raiz, 2026-06)

| ID | Problema | Onde | Causa raiz | DiagCode/Nota |
|----|----------|------|------------|---------------|
| BUG-08 | Leitura de lixo em bind de pattern matching através de blocos | `lower_amir/ctx.rs` — `emit_store_place` | Não atualizava `write_variable`/`current_def` do tracker SSA quando a projeção era vazia (local puro); só variáveis com projeção de campo eram cobertas antes | Corrigido na função compartilhada; testes `parity_ssa_pattern_bind` e `parity_ssa_pattern_bind_multi_arms` |
| BUG-09 | `type 'Result' is not declared` mascarando erro real de sintaxe na stdlib | `arandu_resolve::load_stdlib_transitively`, `arandu_typeck::load_stdlib_signatures` | Erros de parse/leitura de arquivo da stdlib eram engolidos silenciosamente (`if let Ok(...)`) | Propagados como `Diagnostic`; falha de prelude embutida vira ICE (`ICEN001`/`ICET001`), distinto de `M001UnresolvedImport` (erro do usuário) |
| BUG-10 | `Ok`/`Err`/`Some`/`None`/`alloc`/`free` como keywords geravam patches acumulativos no parser | Lexer + `pattern.rs`/`expr.rs` | Palavras-chave reservadas para o que são, estruturalmente, construtores de valor ou funções builtin comuns | Removidas do lexer (BC.2.2); resolvidas via prelude + `#[builtin(alloc)]`/`#[builtin(free)]`; identidade protegida por `N003RedefinedName` |
| BUG-11 | Colisão de `SymbolId` entre declaração e referência qualificada da mesma variante de enum (`Color.Red`) | `resolve/collect.rs` | `define_associated_member` gerava um segundo símbolo para a mesma variante; `enum_variant_tags` só registrava o primeiro | Registro do tag para ambos os símbolos em `collect_type_shapes`; fallback textual por sufixo removido; teste `jit_enum_cross_variant_name_no_collision` |
| BUG-12 | `Option.None`/enum sem payload caía em `unimplemented!()` no backend | `translator/expr.rs` (Cranelift) | `AmirOperand::GlobalRef` nunca teve tradução; construção de variante sem payload não existia como caminho próprio no AMIR | Emitido como `Tuple{tag, Undef}`; teste `jit_none_payload_never_read` (20 iterações, detecta leitura indevida de payload) |
| BUG-13 | `Primitive::Int`/`Uint` sempre calculados como 8 bytes, mascarado por desenvolvimento em 64-bit | `arandu_middle/layout/mod.rs` | Agrupados incorretamente com `I64`/`U64`/`F64` (largura fixa), ignorando `pointer_width` | Separados e calculados dinamicamente; suíte de layout passou a testar `pointer_width` 4 e 8 |
| BUG-14 | Lixo em bits altos de registrador ao chamar função JIT com assinatura de teste incompatível | `jit_tests.rs` | `int`/`uint` agora dependem de plataforma; chamadas de teste com `unsafe fn(i32,...)` não estendiam sinal corretamente pra `i64` real esperado pela função JIT | Assinaturas de teste corrigidas para `i64`; nota de ABI: `int`/`uint` do Arandu equivalem a `intptr_t`/`uintptr_t` em C |
| BUG-15 | Análises de dataflow (move checker, definite init) paravam silenciosamente antes do fixpoint | `move_checker.rs`, `definite_init.rs` | Teto de iteração fixo; estouro cortava a análise sem diagnóstico (`debug_assert!` em debug, nada em release) | Corrigido para ICE explícito ao estourar teto generoso, em vez de corte silencioso |
| BUG-16 | `alloc`/`free`/chamada extern sem exigir bloco unsafe | `type_checker` | Nenhuma validação de contexto para operações de memória crua/FFI | `O012`/`O013`/`O014` (`AllocRequiresUnsafe`/`ExternRequiresUnsafe`/`FreeRequiresUnsafe`); rastreamento via `unsafe_depth` em `TyCtx` |

### 7.5 Nota de padrão observado
Os itens acima compartilham uma característica: o sintoma reportado inicialmente nunca era a causa raiz real, e a correção só se sustentou quando aplicada na camada de origem (SSA, resolução de identidade de símbolo, layout dependente de plataforma) em vez de no ponto onde o sintoma apareceu. Nenhum foi resolvido com `if` especial local sem generalização — ver `docs/ossa-virtual-anchoring.md` para a técnica que motivou esse padrão de investigação.

### 7.1 Crítico — antes de fechar v0.1 ✅ (2026-05)

| ID | Problema | Onde | DiagCode |
|----|----------|------|----------|
| BUG-01 | `break`/`continue` fora de loop sem erro | `check/stmt.rs` | **T022** |
| BUG-02 | `free` não exige `ptr[T]` | `check/stmt.rs` | **T023** |
| BUG-03 | `catch` retorna `ArType::Error` | `synth/expr.rs` | T002 / T005 |
| BUG-04 | `??` não unifica tipos dos lados | `synth/expr.rs` | **T002** / T006 |
| BUG-05 | array literal mismatch silencioso | `synth/expr.rs` | **T002** |
| BUG-08 | `emit_store_place` (ZST/local) não atualiza SSA tracker | `lower_amir/ctx.rs` | ICE (uninitialized read) |

### 7.2 Alto — v0.1 ✅ (2026-05)

| ID | Problema | Onde | DiagCode |
|----|----------|------|----------|
| BUG-06 | `ReturnType` `declared_span` errado | `TyCtx` + `check/stmt.rs` | **T004** (label no decl) |
| SCALE-02 | `global_scope()` no corpo da função | `TypeChecker::type_scope` | — |
| QUAL-01 | `Generic` silencioso → Error | `synth/expr.rs` | **T011** |
| — | Match exhaustiveness (enum) | `synth/match_exhaust.rs` | **T024** |
| — | Retorno só `Result<T,E>` | parser `types.rs` (**P006**) | **P006** |

### 7.3 Médio — v0.1/v0.2

| ID | Problema |
|----|----------|
| SCALE-01 | Prelude hardcoded em `check_program` |
| SCALE-03 | `SimpleStmt` duplicado no `for` C-style |
| SCALE-04 | String de erro `any` repetida 7× |
| QUAL-02/03 | Lambda / `await` silenciosos |
| BUG-07 | `nil` fora de contexto de retorno |

### 7.4 Testes faltantes

- **Property testing** (`proptest`): lexer recovery, parser nesting, strings interpoladas, generic ambiguity.
- **Multi-file**: resolver + typecheck com 2+ `.aru` (v0.2 bloqueador de “linguagem real”).

---

## 8. Roadmap por fases (consolidado)

### Fase 1 — Estabilização semântica (v0.1) — **AGORA**

**Objetivo:** compilador que **analisa** programas reais com semântica consistente, sem backend.

```text
[x] Bugs críticos (§7.1)
[x] Modularizar type checker (§5)
[x] AMIR: SwitchInt formal (int/enum/bool; `match_lower.rs`)
[x] AMIR: validador CFG (`amir_validate.rs`, goldens `tests/amir`)
[x] Generics: `where` + interface satisfaction (T011/T025, `types/interfaces.rs`)
[x] Result canônico (E) + goldens
[x] Definite initialization (G)
[x] OSSA mínimo: move, copy, destroy (F1)
[x] Docs (A)
[x] Move checker básico: O001, O005, O007 (M1)
[x] Constant folding + DCE (O1)
```

**Não fazer em v0.1:**

- LLVM, query/incremental, Rowan, comptime, e-graphs
- NLL, interprocedural ownership, lifetime inference
- OSSA completo (`borrow_*`, `end_borrow`) — v0.2
- Reachability types formais — v0.3+ pesquisa

### Fase 2 — Compilador real (v0.2)

| Entrega | Notas |
|---------|-------|
| Backend C 1:1 | Debug pipeline end-to-end |
| Multi-file + visibilidade | Bloqueador de escala |
| Arena + interning + IndexVec AHIR | Ver RFC indexvec |
| OSSA borrow + gen fallback (O004) | Nota sempre visível |
| Move interprocedural básico | Chamadas simples |
| `arandu_hir` público + serde | Tooling |
| Stdlib mínima (`arandu_std`) | Prelude externo, não hardcoded |

### Fase 3 — Tooling e performance (v0.3)

- Incremental / query system (Salsa-like)
- LSP mínimo
- Formatter (parser → texto; depois CST se necessário)
- Optimizer: inlining, escape analysis leve

### Fase 4 — Pesquisa e backend sério (v0.4+)

- Reachability-flavored ownership (experimental)
- Light typestate em stdlib
- LLVM backend
- e-graphs / equality saturation (só middle-end maduro)

---

## 9. O que adiar sem dó (reforço)

| Item | Motivo |
|------|--------|
| Polonius Datalog | Lento; insight já capturável em CFG |
| Borrow checker estilo Rust | Contradiz identidade Arandu |
| Generational refs everywhere | Custo + opacidade; só fallback |
| Liquid types + SMT | Compile time + UX |
| Algebraic effects completos | Anos de trabalho; Err/async separados em v1 |
| Backend LLVM cedo | Debug multiplicado |
| Macros / comptime | Semântica instável |

---

## 10. Identidade técnica (uma frase)

**Arandu:** linguagem de sistemas com ergonomia Swift/Vale, pipeline Rust/Swift (AHIR/AMIR), ownership pragmático no CFG com fallback geracional transparente — sem lifetimes na superfície.

---

## 11. Próximos marcos técnicos (ordem sugerida)

1. PR: **v0.2 design curto** para backend C, stdlib mínima e próximos passos de borrow/gen fallback.
2. PR: **memory checker / generational fallback** — definir O004 e a estratégia mínima de referências geracionais.
3. PR: **Backend C 1:1** — primeiro caminho executável, mantendo AMIR não otimizado como saída padrão de debug.
4. Manter o §Painel em [roadmap](./arandu-compiler-roadmap-v0.1.md) atualizado a cada merge.

---

## 13. Paridade de Execução do Backend (C / Cranelift JIT)

Com a consolidação do Backend C (CEmitter) com paridade estrutural completa, registramos as decisões conscientes de cobertura de testes de execução:

- **Cobertura Exaustiva**: O gerador de código C (`emit_rvalue`) e o scanner de variáveis ativas tratam todas as 16 variantes de `AmirRvalue` sem braços coringa (`_ => {}`), garantindo erro de compilação estática no compilador caso novas variantes sejam adicionadas.
- **Variantes Faltantes de Testes de Paridade**: As variantes `Unary`, `Len`, `Alloc` (como rvalue direto) e `Borrow`/`BorrowMut` estão cobertas pelas regras estáticas de match, mas **não possuem testes dedicados de paridade de execução** na Fase 1/Fase 2 por não possuírem caminhos de emissão direta correspondentes a partir da sintaxe de superfície atual da linguagem (por exemplo, a sintaxe de referências `&x` e ownership completo está planejada para a Fase 3/Fase 4).
- **Ação Futura**: Testes dedicados de paridade de execução para estas variantes devem ser criados assim que a respectiva sintaxe de superfície e abaixamento (lowering) forem introduzidos no compilador.

---

## 14. Histórico

| Data | Mudança |
|------|---------|
| 2026-05 | Plano estratégico v0.1 — síntese análise estrutural, bugs, papers, decisões |

---

*Mantenha este documento alinhado ao roadmap executivo; decisões novas entram aqui primeiro, depois viram checkboxes no roadmap.*
