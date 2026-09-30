# Diretrizes e Instruções para Agentes de IA (AGENTS.md)

Este documento estabelece as regras de escopo, segurança, boas práticas e limites operacionais para qualquer agente autônomo ou assistente de IA trabalhando neste repositório.

## 1. Escopo de Acesso e Limitação de Diretório (CRÍTICO)

> **Regra Primária de Isolamento:** O acesso e a atuação do agente estão **estritamente restritos** a esta pasta raiz (`/home/facrf/projetos/Leitor_pdf`) e suas subpastas.

### Restrições Obrigatórias:
1. **Sem Acesso Externo:** O agente **NÃO deve** ler, listar, criar, modificar, mover ou deletar arquivos/diretórios fora de `/home/facrf/projetos/Leitor_pdf` (ex.: proibido acessar `../`, `~`, `/home/facrf/`, `/tmp/` ou outros projetos).
2. **Execução de Comandos:** Todos os comandos de terminal (`run_command` / bash) devem ser executados com o diretório de trabalho (`Cwd`) fixado na raiz deste projeto ou em suas subpastas. É proibido executar comandos com alvos globais ou que referenciem caminhos externos ao projeto.
3. **Caminhos Relativos:** Todas as referências no código e nos scripts devem utilizar caminhos relativos à raiz do projeto ou ser configuráveis via variáveis de ambiente.

---

## 2. Visão Geral do Projeto

- **Nome do Projeto:** `Leitor_pdf`
- **Finalidade:** Biblioteca digital local para catalogação, leitura e organização de PDF, EPUB, MOBI/AZW, CBZ e formatos de texto.
- **Estrutura Esperada:**
  - Backend Rust modular em `src/`, persistência SQLite e interface estática em `web/`.
  - Separação clara entre catálogo/ingestão, leitores por formato, metadados, API e persistência.
  - Execução principal via Docker na porta 20000, com `/pdf` e `/data` configuráveis por variáveis de ambiente.
  - Tratamento robusto de arquivos inválidos, compactados malformados, páginas corrompidas e textos não estruturados.
  - Funcionamento offline-first; qualquer consulta externa de metadados exige habilitação e ação explícitas do usuário.

---

## 3. Diretrizes de Código e Arquitetura

1. **Linguagem & Padrões:**
   - Escreva código limpo, legível e aderente aos padrões da linguagem utilizada (ex.: PEP 8 e Type Hints caso seja Python).
   - Documente funções, classes e métodos com docstrings claras explicando entradas, saídas e comportamentos esperados.
2. **Resiliência e Tratamento de Erros:**
   - Sempre encapsule operações de I/O e leitura de PDFs com tratamento adequado de exceções.
   - Faça gerenciamento seguro de memória e fechamento adequado de descritores de arquivos (`context managers` / `with`).
3. **Modularidade:**
   - Mantenha funções pequenas e com responsabilidade única.
   - Separe testes unitários e de integração em uma pasta dedicada (`tests/`).

---

## 4. Segurança e Variáveis de Ambiente

- **Segredos e Chaves:** Nunca salve chaves de API, senhas, tokens ou credenciais no código-fonte.
- **Variáveis de Ambiente:** Utilize arquivos `.env` para configurações locais e mantenha um `.env.example` versionado com as chaves fictícias necessárias.
- **Versionamento:** Respeite rigorosamente as exclusões configuradas no [.gitignore](file:///home/facrf/projetos/Leitor_pdf/.gitignore).

---

## 5. Protocolo de Modificação e Testes


- Antes de aplicar alterações complexas, analise os impactos nos módulos existentes.
- Sempre verifique a sintaxe e rode os testes relevantes antes de finalizar uma tarefa.
- Mantenha a documentação (como `README.md` e este `AGENTS.md`) sempre sincronizada com as mudanças arquiteturais.

## 6. Retomada de trabalho

- Ao retomar uma tarefa, leia `CONTINUAR.md` (se existir) e confira `git status`/`git diff` antes de editar.
- Antes de encerrar trabalho incompleto, atualize `CONTINUAR.md` com mudanças realizadas, testes realmente executados, limitações e próximo passo concreto.
- Não registre segredos ou conteúdo privado do acervo; não marque itens pendentes como concluídos. O checkpoint não substitui a verificação do estado atual do código.

## 7. Contratos de paginação e validação

- Catálogo e OPDS retornam páginas (`limit` padrão 60, máximo 200; `offset` padrão 0).
  Preserve o total filtrado, desempate por ID e o link OPDS `next` ao alterar essas rotas.
- Use a validação compartilhada de posições em notas, progresso e restauração.
- Testes Rust de regressão ficam em `tests/rust/`, incluídos pelos módulos internos;
  o Docker precisa copiar essa pasta antes de executar `cargo test`.
- A autenticação exige ambas as credenciais ou ambas vazias. Geração de capas respeita
  `COVER_TIMEOUT_SECONDS` (1–600, padrão 60) e encerra o renderizador ao exceder o prazo.
