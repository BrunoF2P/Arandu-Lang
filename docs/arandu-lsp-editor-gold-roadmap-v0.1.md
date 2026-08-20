# Arandu — Roadmap Gold do LSP e Editor

**Status:** ativo em paralelo ao roadmap de estabilização.  
**Objetivo:** usar o editor como instrumento de hardening até oferecer uma
experiência beta gold previsível no VS Code.  
**Arquitetura normativa:** [`arandu-salsa-lsp-architecture-v0.1.md`](./arandu-salsa-lsp-architecture-v0.1.md).

## Estado auditado

| Capacidade | Estado | Limite para gold |
|------------|--------|------------------|
| VFS, debounce, snapshots e stale safety | `done` | Falta stress E2E de sessão longa/editor real. |
| Diagnósticos on-type | `done` | Provar ausência de publicação stale e orçamento de latência. |
| Goto, hover, completion e signature help | `done` | Cobertura E2E multi-file e Unicode. |
| References e rename | `done` | Rename precisa validação/preview de conflitos para gold. |
| Semantic tokens | `done` | Fechar encoding e spans multi-linha. |
| Formatter e quick-fix `;` | `done` | Integrar format-on-save e testes de idempotência E2E. |
| Extensão VS Code | `partial` | Compila, ativa e mostra status; não tem suíte E2E nem lint real. |
| Debugger e visualizações avançadas | `planned` | Fora do beta gold inicial. |

## L0 — Gate da extensão

- [ ] `npm ci` e `npm run compile` reproduzíveis.
- [ ] ESLint ou gate equivalente real; remover o placeholder de lint.
- [ ] Testes automatizados da extensão com VS Code Extension Host.
- [ ] Descoberta do `arandu-lsp` testada para PATH, configuração explícita e layout de release.
- [ ] Crash, restart e logs apresentam estado acionável ao usuário.

**DoD L0:** a extensão tem o mesmo nível mínimo de gate que o workspace Rust.

## L1 — Correção de protocolo e texto

- [ ] Negociar e testar position encoding suportado; UTF-16 permanece correto para clientes que o exigem.
- [ ] Cobrir Unicode antes/depois do cursor em todos os requests semânticos.
- [ ] Dividir semantic tokens multi-linha em tokens válidos por linha.
- [ ] Testar mudanças incrementais múltiplas, arquivo vazio e edição no fim do arquivo.
- [ ] Validar cancelamento/descarte de jobs obsoletos sob rajadas de edição.

**DoD L1:** nenhuma posição, token ou diagnóstico incorreto em ASCII/Unicode no protocolo suportado.

## L2 — Dogfooding multi-file

- [ ] Abrir, fechar, criar, excluir e renomear arquivos durante uma sessão.
- [ ] Imports locais e stdlib atualizam completion/goto/diagnósticos sem restart.
- [ ] Rename detecta nome inválido e conflitos; preview pertence à extensão, não à query pura.
- [ ] Formatter-on-save é opt-in, idempotente e não move o cursor de forma surpreendente.
- [ ] Testar vários documentos abertos e requests concorrentes em snapshots.
- [ ] Medir p50/p95 de diagnóstico, completion, goto e rename em corpus versionado.

**DoD L2:** o editor pode ser usado diariamente para desenvolver os projetos do corpus.

## L3 — Beta gold do editor

- [ ] Instalação da extensão e do servidor documentada e testada nos hosts suportados.
- [ ] Matriz de capabilities publicada com limitações conhecidas.
- [ ] Sem crash, deadlock ou publicação stale em campanha de stress definida.
- [ ] Diagnósticos, navigation, rename, tokens e format passam no Extension Host.
- [ ] Release candidate dogfood sem bloqueador conhecido.

**DoD L3:** VS Code + `arandu-lsp` formam uma experiência beta gold dentro da matriz publicada.

## Depois do gold

Debugger, valores inline, borrow gutter, Test Explorer, CodeLens de paridade,
timeline de corrotinas e overlay Salsa permanecem propostas pós-gold. Elas não
devem interromper L0–L3, salvo quando uma delas for necessária para reproduzir
ou diagnosticar um bloqueador de estabilidade.
