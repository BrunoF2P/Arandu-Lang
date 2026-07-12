# Roadmap de Evolução da IDE Arandu

Este documento define os objetivos e prioridades para o suporte a IDE do Arandu (extensão do VS Code e servidor de linguagem `arandu-lsp`), organizados em pilares conceituais baseados na filosofia de design do compilador e nas melhores práticas do ecossistema de desenvolvimento moderno.

---

## Resumo Executivo

A evolução da IDE Arandu visa unir o melhor de dois mundos: **a velocidade e flexibilidade leve do VS Code** com **a consistência de ferramentas integradas e análise semântica profunda da JetBrains**. 

Evitamos o modelo fragmentado do VS Code (onde o desenvolvedor precisa caçar dezenas de extensões para formatação, testes e depuração) em prol de uma experiência **"Batteries-Included"** (pronta para uso e sem atrito de configuração) impulsionada pela arquitetura transparente do compilador Arandu (Salsa, OSSA, fallback geracional e múltiplos backends).

---

## Pilares de Design (Temas)

### Tema 1: Edição Confiável e Refatoração Semântica (UX "Nível JetBrains")
Eliminar a barreira clássica entre "editores de texto simples" e "IDEs robustas", garantindo refatorações 100% seguras que realmente compreendem o fluxo de dados do projeto.
*   **Refatoração Baseada em Árvore de Tipos (Semântica):** Usar a busca de referências O(1) baseada no `SymbolId` do compilador (identidade real de tipo/valor, e não baseada em mera correspondência de strings) para garantir renomeações e deleções seguras. Apresentar uma janela de preview com diff comparativo do projeto antes de aplicar qualquer refatoração (Rename, Safe Delete, Extract).
*   **Visualização de Valores no Debugger Inline:** Durante a depuração, exibir o valor em tempo real de cada variável e temporário SSA de forma suave bem ao lado da linha correspondente no editor (padrão de depuração visual das IDEs JetBrains).
*   **Preservação de Identidade de Variáveis (OSSA Debug):** Garantir que variáveis otimizadas na fase OSSA mantenham seus mapeamentos originais (`temp_origins`) gerando tabelas DWARF precisas, eliminando a dor comum de depurar código otimizado em C/C++ onde variáveis desaparecem como "optimized out".
*   **Geração Inteligente de Stubs de Interfaces:** Automação Go-style que implementa dinamicamente stubs de funções soltas e métodos necessários para que uma struct satisfaça uma interface, sem a necessidade de blocos implícitos redundantes.

### Tema 2: Memória Transparente ("Magia Inspecionável")
Tornar visualmente perceptíveis as decisões de análise estática e ciclo de vida do compilador diretamente no editor de código do programador.
*   **Explicação de Heap Escape Inline:** Dicas visuais interativas (inlay hints) que apontam na linha exata por que uma alocação não pôde ficar na stack e caiu em fallback geracional (heap). Clicar na dica abre a árvore de escape detalhada de forma gráfica.
*   **Borrow Windows no Gutter (Régua de Linha):** Representação visual espacial dos tempos de vida e empréstimos ativos (live ranges das variáveis SSA) através de colunas verticais coloridas ao lado dos números de linha, tornando a verificação de empréstimos intuitiva.
*   **Mapeamento de Tipos Globais Implicitados:** Inlay hints discretas revelando tipos deduzidos em declarações de variáveis implícitas, parâmetros de closures e retornos.

### Tema 3: Ferramental Unificado e Sem Atrito (Zero Configuration / Batteries-Included)
Prover todas as necessidades do programador em uma única extensão consistente desenvolvida e mantida em sintonia com o compilador.
*   **Formatador e Linter Nativos Integrados:** Integração direta de `arandu_fmt` e diagnósticos avançados dentro do próprio LSP, garantindo formatação instantânea ao salvar sem dependência de plugins de terceiros.
*   **Explorador de Testes e Cobertura Nativos:** Descobrir, executar e depurar testes unitários e de paridade de backends diretamente na interface de testes do VS Code com botões visuais ao lado do código.
*   **Indicador de Status do Compilador Salsa:** Informações em tempo real sobre a indexação e recomputação incremental Salsa na barra de status do editor, para que o desenvolvedor saiba exatamente se a IDE está pronta ou processando alterações de dependências.
*   **Postfix Templates e Assists Estruturais:** Atalhos para expansão rápida de código estruturado, como auto-geração de todos os braços de tratamento de variantes ao digitar `.match` no final de uma variável.

---

## Status e Prioridades (Status of Goals)

### Prioridades Imediatas (Milestone 1: UX Essencial e Consistência)
Foco em usabilidade básica e robustez de edição:
- [x] Sincronização incremental e diagnósticos básicos do compilador.
- [x] Destaque de sintaxe via TextMate e cores semânticas completas (Semantic Tokens).
- [x] **Indicador de Status do Servidor (Status Bar)**: Exibição visual do estado do LSP (iniciando, ativo, erro) com clique rápido para abrir os logs.
- [ ] **Refatoração Segura com Preview**: Rename Symbol e Safe Delete com janela de confirmação de call-sites antes da aplicação.
- [ ] **Formatador Nativo Integrado**: Formatação imediata ao salvar utilizando `arandu_fmt`.
- [ ] **Debugger Inspecionável**: Geração de DWARF com mapeamento das variáveis de origem (`temp_origins`).
- [ ] **Valores Inline no Debugger**: Renderização visual do estado de variáveis ativas inline.
- [ ] **Geração de Stubs de Interfaces**: Ação rápida para implementar membros de interface estruturais ausentes.
- [ ] **Visualização de Quick-fixes**: Preview de diff inline ao passar o mouse sobre sugestões rápidas de código.

### Metas de Médio Prazo (Milestone 2: Análise de Ciclo de Vida e Ferramental)
Foco em expor análises de tempo de vida e unificar o ambiente:
- [ ] **Inlay hints de Heap Escape**: Dicas interativas apontando o motivo exato de um valor cair no fallback geracional.
- [ ] **Smart Inlay Hints**: Exibição de tipos inferidos em variáveis implícitas e parâmetros de closures.
- [ ] **Badges de Custo de Alocação**: Marcadores de complexidade de heap na assinatura de funções (`stack-only` / `1 escape` / `N escapes`).
- [ ] **Postfix Templates de Match**: Expansão automática de todos os ramos de um enum ao digitar `.match`.
- [ ] **Borrow Gutter Visualizer**: Delimitação visual vertical na régua de linha mostrando o live range dos empréstimos ativos.
- [ ] **Test Explorer Integrado**: Integração de testes e visualização de testes de paridade nos diferentes backends do Arandu.
- [ ] **Suporte a Position Encoding UTF-8 (LSP 3.17)**: Negociar `utf-8` no initialize do LSP para suportar nativamente caracteres não-ASCII sem conversões custosas de UTF-16.
- [ ] **Destaques de Tokens Multi-linha**: Quebrar spans de realces de múltiplas linhas no `encode_highlights` para preservar a integridade dos deltas do editor.

### Explorações de Longo Prazo (Milestone 3: Recursos Avançados de Compilador)
Ferramentas de engenharia avançadas e visualizações de estado:
- [ ] **CodeLens de Paridade de Backends**: Roda a função isolada nos backends ativos (Cranelift e C) e faz diff em tempo de execução.
- [ ] **Fix-it Lens (Mentor Idiomático)**: Sugestões interativas de otimização de tempo de vida e alocação de recursos.
- [ ] **Timeline de Corrotinas**: Mapeamento estruturado do estado de suspensão e variáveis capturadas de threads/tasks assíncronas.
- [ ] **Overlay de Consultas Salsa**: Mostrar a árvore de avaliação incremental Salsa e caminhos de invalidação de cache.

---

## 🌟 Evolução do Ecossistema Integrado (Propostas de Ferramental)

Estas propostas integram a IDE diretamente a novas ferramentas do compilador que facilitam o desenvolvimento e a exploração do ecossistema Arandu:

- [ ] **Console REPL Integrado (Terminal Arandu)**: Suporte para abrir uma sessão REPL interativa (`arandu repl`) diretamente no painel de terminal integrado do VS Code, permitindo rodar e inspecionar expressões dinamicamente com JIT.
- [ ] **Visualizador de Documentação Nativo (Live Docs)**: Integração com `arandu doc` para servir páginas de documentação de código do workspace de forma local e automática, exibindo visualizações rápidas de docs ao lado do código.
- [ ] **FFI Bindgen Wizard**: Assistente de interface gráfica ou comandos para acionar `arandu bindgen` no workspace, gerando declarações C/Arandu de forma automatizada ao importar arquivos `.h`.
- [ ] **Gerenciador de Dependências Visual (Package Explorer)**: Interface gráfica no painel lateral do VS Code para inspecionar dependências declaradas em `arandu.toml`, buscar pacotes online e gerenciar atualizações via `arandu pkg`.
- [ ] **Linter de Alocação e Performance (Memory Diagnostics)**: Integração com as análises do `arandu clippy` para exibir avisos no editor sobre alocações de escape sub-otimizadas ou escapes redundantes para a heap.
