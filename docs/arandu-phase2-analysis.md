# Análise Crítica do Arandu-Lang na Fase 2

Este documento consolida os problemas encontrados na revisão do workspace, com foco em segurança de memória, robustez do compilador, qualidade de código e aderência à proposta da linguagem.

O objetivo aqui não é listar tudo que existe no projeto, mas destacar os pontos que mais afetam a confiança na fase 2 do compilador: diagnósticos que podem falhar silenciosamente, caminhos de backend que podem derrubar o processo, e gaps entre o que o frontend anuncia e o que o pipeline realmente suporta.

## Visão Geral

O projeto tem uma estrutura boa para um compilador experimental: o workspace está dividido em lexer, parser, type checker, lowering para AMIR, backend Cranelift e CLI. O desenho geral aponta para a direção correta.

Os principais riscos que ainda aparecem são:

1. Análises de segurança que podem parar antes do fixpoint e perder diagnósticos.
2. Crash paths no backend em vez de erros diagnosticáveis.
3. Features que o parser e os exemplos divulgam como existentes, mas que ainda não fecham no lowering.
4. Várias oportunidades de reduzir clones, buscas lineares e alocações em caminhos quentes.

Em outras palavras: a base está boa, mas ainda há pontos que ferem a promessa central de uma linguagem de sistemas segura e previsível.

---

## 1. Riscos de Segurança e Correção

### 1.1 Move checker pode parar antes da convergência

Arquivo: [crates/arandu_mir/src/move_checker.rs](../crates/arandu_mir/src/move_checker.rs)

Trecho relevante:

```rust
let mut iterations = 0;
let max_iterations = num_blocks * 32 + 100;

while let Some(bid) = worklist.pop_front() {
    iterations += 1;
    if iterations > max_iterations {
        debug_assert!(false, "move checker failed to converge");
        break;
    }
```

Problema: o algoritmo usa um limite fixo de iterações. Em debug, isso acende um `debug_assert!`; em release, ele simplesmente corta a análise.

Impacto: isso pode produzir falso negativo nos diagnósticos O001, O005 e O007. Em uma linguagem que promete rastrear uso após move, double-free e inconsistências entre branches, parar antes do fixpoint é um problema sério de correção.

Sugestão: remover o limite heurístico e deixar o worklist convergir naturalmente. Se houver suspeita de loop infinito, isso deve virar um erro explícito do compilador, não uma parada silenciosa.

Exemplo de direção de correção:

```rust
while let Some(bid) = worklist.pop_front() {
    let bi = bid.as_usize();
    // processa normalmente até estabilizar
}
```

---

### 1.2 Definite initialization também pode perder O008

Arquivo: [crates/arandu_mir/src/definite_init.rs](../crates/arandu_mir/src/definite_init.rs)

Trecho relevante:

```rust
let mut iterations = 0;
let max_iterations = num_blocks * num_blocks + 10;

while let Some(bid) = worklist.pop_front() {
    iterations += 1;
    if iterations > max_iterations {
        break; // Safety: prevent infinite loops on malformed CFGs
    }
```

Problema: a análise de inicialização definitiva também usa um teto fixo. Se ele for excedido, a análise é interrompida.

Impacto: O008 pode deixar de ser emitido em CFGs mais complexos. Como essa análise protege contra uso de local possivelmente não inicializado, o efeito é direto sobre a promessa de segurança de memória.

Sugestão: mesma abordagem do move checker: convergir até estabilizar ou falhar explicitamente, mas não interromper a análise em silêncio.

---

### 1.3 Chamadas indiretas ainda podem derrubar o backend

Arquivo: [crates/arandu_backend_cranelift/src/translator/stmt.rs](../crates/arandu_backend_cranelift/src/translator/stmt.rs)

Trecho relevante:

```rust
let call_inst = match callee {
    AmirOperand::FunctionRef(sym_id) => {
        // ... tradução de chamada direta ...
        self.builder.ins().call(local_ref, &clif_args)
    }
    _ => unimplemented!("Indirect function calls not implemented yet"),
};
```

Problema: qualquer forma de chamada que não seja chamada direta cai em `unimplemented!`.

Impacto: isso é um panic user-triggerable. Se a IR chegar a esse formato, o compilador não devolve um diagnóstico; ele quebra.

Sugestão: substituir o `unimplemented!` por diagnóstico formal do compilador, ou bloquear a forma de IR antes de chegar no backend.

Boa prática de fase 2: backend deve falhar com erro controlado, não com panic.

---

### 1.4 JIT inicializa com `unwrap`/`expect`

Arquivo: [crates/arandu_backend_cranelift/src/jit.rs](../crates/arandu_backend_cranelift/src/jit.rs)

Trecho relevante:

```rust
flag_builder.set("use_colocated_libcalls", "false").unwrap();
flag_builder.set("is_pic", "false").unwrap();
flag_builder.set("opt_level", "none").unwrap();

let isa = cranelift_native::builder()
    .expect("Failed to create Cranelift isa builder")
    .finish(settings::Flags::new(flag_builder))
    .expect("Failed to build Cranelift isa");
```

Problema: o setup do backend assume sucesso em tudo.

Impacto: em arquitetura não suportada, configuração inválida ou falha de backend, a CLI aborta em vez de produzir uma mensagem útil.

Sugestão: devolver `Result` ou diagnóstico e deixar a CLI apresentar a falha ao usuário.

Contexto prático: o ecossistema Rust tende a tratar `unwrap`, `expect` e `unimplemented!` como sinais de fragilidade quando isso está em código de produção ou de compilador. Não é só estética; isso tem impacto real na robustez.

---

## 2. Problemas de Qualidade de Código

### 2.1 A linguagem anuncia `async`, mas o pipeline ainda não fecha a feature

Arquivos:
- [examples/stable/syntax/async.aru](../examples/stable/syntax/async.aru)
- [crates/arandu_parser/tests/parser_golden.rs](../crates/arandu_parser/tests/parser_golden.rs)
- [crates/arandu_mir/src/lower_amir/expr.rs](../crates/arandu_mir/src/lower_amir/expr.rs)

O exemplo estável usa `async` e `await`:

```aru
async func fetchText(path: str) : Result<str, Err> {
    if path == "" {
        return Result.Err(err.new("emptyPath"))
    }

    return Result.Ok("contents")
}

async func main() {
    let text, err = await fetchText("data.txt")
```

E a suíte de parser trata esse arquivo como parte obrigatória do corpus estável:

```rust
"examples/stable/syntax/async.aru",
```

Mas o lowering para AMIR ainda rejeita variantes relacionadas:

```rust
HirExprKind::Catch { .. } => Err(amir_unsupported(...)),
HirExprKind::Lambda { .. } => Err(amir_unsupported(...)),
HirExprKind::AsyncBlock { .. } => Err(amir_unsupported(...)),
HirExprKind::UnsafeBlock { .. } => Err(amir_unsupported(...)),
```

Problema: a documentação de exemplo e o pipeline não estão alinhados.

Impacto: o usuário vê uma feature exposta no corpus “estável”, mas ainda encontra bloqueios em fases posteriores. Isso degrada a confiança na linguagem e nos testes.

Sugestão: separar com clareza o que é sintaxe aceita, o que é semântica validada e o que é lowering realmente suportado.

---

### 2.2 `unsafe` fora do bloco seguro já tem cobertura de frontend, mas pouca prova de ponta a ponta

Arquivos:
- [examples/invalid/semantics/unsafe_outside_block.aru](../examples/invalid/semantics/unsafe_outside_block.aru)
- [crates/arandu_parser/src/parser/stmt.rs](../crates/arandu_parser/src/parser/stmt.rs)
- [crates/arandu_typeck/src/type_checker/check/validate.rs](../crates/arandu_typeck/src/type_checker/check/validate.rs)

O parser aceita `unsafe` como construção sintática:

```rust
if self.at_kind_name("KW_UNSAFE") {
    let start = self.mark();
    self.advance();
    let block = self.parse_block()?;
    return Ok(self.pool.alloc_stmt(Stmt::Unsafe {
```

E o type checker valida blocos `unsafe` normalmente:

```rust
ExprKind::AsyncBlock { block, .. } | ExprKind::UnsafeBlock { block, .. } => {
    validate_block(checker, checker.pool, checker.pool.block(*block));
}
...
Stmt::Unsafe { block, .. } => {
    validate_block(checker, pool, block);
}
```

Problema: a regra em si parece existir, mas a cobertura de teste local não é tão visível quanto deveria para provar o comportamento fim a fim.

Impacto: qualquer mudança futura no lowering pode quebrar a regra sem uma rede de segurança boa.

Sugestão: manter um teste de regressão explícito para `unsafe` fora do bloco, do parser até a semântica, e um segundo para o caminho seguro dentro do bloco.

---

### 2.3 Falta uma suíte focada em crash paths do backend

Arquivo: [crates/arandu_backend_cranelift/tests/jit_tests.rs](../crates/arandu_backend_cranelift/tests/jit_tests.rs)

O arquivo cobre muitas operações e algumas chamadas entre funções, mas não vi um teste que provoque o caminho hoje marcado como não implementado no backend.

Problema: o teste atual é forte em paths felizes, mas não prova que o backend responde bem quando encontra algo fora do conjunto suportado.

Impacto: regressões de crash podem entrar sem um alerta claro.

Sugestão: adicionar um teste que force a forma de chamada ainda não suportada e verificar diagnóstico, não panic.

---

## 3. Oportunidades de Performance

### 3.1 Codegen faz buscas lineares em estruturas quentes

Arquivo: [crates/arandu_backend_cranelift/src/translator/mod.rs](../crates/arandu_backend_cranelift/src/translator/mod.rs)

Trechos relevantes:

```rust
pub(crate) fn get_temp_clif_type(&self, temp_id: TempId) -> Option<Type> {
    self.current_func
        .temps
        .iter()
        .find(|t| t.id == temp_id)
```

```rust
pub(crate) fn temp_span(&self, temp_id: TempId) -> Span {
    self.current_func
        .temps
        .iter()
        .find(|temp| temp.id == temp_id)
```

```rust
pub(crate) fn local_span(&self, local_id: LocalId) -> Span {
    self.current_func
        .locals
        .iter()
        .find(|local| local.id == local_id)
```

Problema: esses helpers percorrem vetores inteiros em caminhos de uso frequente.

Impacto: em funções grandes, isso acrescenta custo desnecessário ao codegen.

Sugestão: indexar por ID ou construir tabelas auxiliares por função.

---

### 3.2 Análises usam clones e bitsets repetidamente

Arquivos:
- [crates/arandu_mir/src/move_checker.rs](../crates/arandu_mir/src/move_checker.rs)
- [crates/arandu_mir/src/definite_init.rs](../crates/arandu_mir/src/definite_init.rs)

Os passes fazem vários clones de estado e varrem worklists repetidamente.

Problema: o desenho é correto, mas ainda bastante alocador/clonador.

Impacto: funções grandes e programas com muitos blocos vão sentir isso.

Sugestão: reutilizar buffers temporários e reduzir alocação em cada iteração do dataflow.

---

### 3.3 O lexer ainda pode ser afinado em entradas token-densas

Arquivo: [crates/arandu_lexer/src/lexer.rs](../crates/arandu_lexer/src/lexer.rs)

Trecho relevante:

```rust
tokens: Vec::with_capacity(source.len() / 4),
```

Problema: a heurística é aceitável, mas subdimensiona bastante em arquivos com muitos tokens curtos.

Impacto: mais realocações do que o necessário.

Sugestão: medir a densidade real de tokens do corpus e ajustar a pré-alocação ou usar uma heurística mais alinhada ao formato da linguagem.

---

## 4. Alinhamento com a Proposta da Linguagem

### O que está alinhado

- O projeto já separa bem lexer, parser, semântica, AMIR e backend.
- Há foco real em diagnósticos e em segurança de memória.
- A existência de testes golden e exemplos estáveis ajuda bastante na comunicação da linguagem.

### O que ainda está desalinhado

- Análises de segurança não podem parar silenciosamente antes do fixpoint.
- O backend não pode derrubar o processo em caminhos ainda não suportados.
- Exemplos e corpus estável não deveriam prometer features que não fecham no lowering.

### Direção recomendada para a fase 2

1. Fechar crash paths.
2. Garantir diagnósticos completos e estáveis.
3. Aumentar testes de borda e regressão.
4. Reduzir alocações e buscas lineares em codegen e dataflow.

---

## Conclusão

O Arandu-Lang está em uma base boa, mas ainda está no ponto em que robustez vale mais do que adicionar mais sintaxe. A fase 2 deveria priorizar:

- menos `unwrap`, `expect` e `unimplemented!`;
- menos limites artificiais em análises de segurança;
- mais testes cobrindo bordas e falhas;
- mais consistência entre exemplos, parser, lowering e backend.

Se a meta é uma linguagem de sistemas segura e confiável, esses pontos são mais importantes do que expandir superfície de feature antes da hora.