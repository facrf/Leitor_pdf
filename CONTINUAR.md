# Checkpoint — 2026-10-15

## Revisão atual — Busca textual completa com SQLite FTS5

Implementação da busca textual completa no conteúdo dos livros com SQLite FTS5.

### Mudanças realizadas:

1. **Modelos (`src/models.rs`)**:
   - Adicionado campo opcional `content_only: Option<bool>` em `CatalogQuery`.
   - Adicionadas estruturas `SearchResultItem`, `SearchResponse` e `FtsStatus` para resposta de busca e status do índice FTS5.

2. **Extração de Texto Multi-Formato (`src/readers.rs`)**:
   - Implementado `extract_text_sample(path, format, max_bytes)` com amostragem de até 256 KiB:
     - PDF: extração de texto via `lopdf::Document::extract_text`.
     - EPUB: leitura dos arquivos HTML/XHTML via `archive_resource` e limpeza de tags.
     - MOBI / PalmDOC: descompressão e extração de texto com remoção de tags HTML.
     - TXT / MD / HTML / HTM / FB2: decodificação de texto bruto e sanitização com `strip_html_tags_to_text`.
   - Implementadas `strip_html_tags_to_text` (com descarte de blocos `<script>`/`<style>` e decodificação de entidades HTML) e `truncate_to_char_boundary` (garantindo cortes sem quebrar caracteres multi-byte UTF-8).
   - Teste unitário adicionado: `strips_html_tags_and_scripts_for_text_search`.

3. **Banco de Dados SQLite FTS5 (`src/db.rs`)**:
   - Tabela virtual `books_fts` criada com tokenizer `unicode61 remove_diacritics 2` no schema inicial e migração.
   - `validate_restore_schema`: shadow tables FTS5 (`books_fts*`) ignoradas na checagem estrita de schema obrigatório, preservando compatibilidade com backups anteriores.
   - Sanitização de query FTS5 (`sanitize_fts5_query`) protegendo contra operadores e caracteres especiais que causam erro de sintaxe SQL no engine FTS5.
   - Integração da cláusula FTS5 em `list_books` com suporte a filtro `content_only`.
   - Novos métodos: `search_content` (busca com snippet `<mark>` e BM25), `index_book_content`, `unindex_book_content`, `fts_status` e `books_needing_fts`.
   - Sincronização automática do índice `books_fts` em atualizações de metadados (`update_book_with_cover`), exclusões e indisponibilização de livros (`mark_unavailable`).
   - Teste de regressão adicionado em `tests/rust/catalog-regression.rs`: `fts5_indexing_and_search_regression`.

4. **API HTTP (`src/api.rs`)**:
   - Rota `GET /api/search?q=...&limit=...&offset=...`: busca por conteúdo com trechos destacados (`snippet`) e ordenação BM25.
   - Rota `GET /api/maintenance/fts`: estatísticas de cobertura do índice FTS5 (`total_books`, `indexed_books`, `percent`).
   - Rota `POST /api/maintenance/fts`: reindexação em lote sob controle de exclusividade de manutenção (`coordinate_restore`).

5. **Interface Web (`web/index.html`, `web/app.js`, `web/styles.css`)**:
   - `web/index.html`: adicionado seletor de modo de busca (Catálogo completo vs. Apenas conteúdo dos livros) na busca avançada; adicionado disclosure de Manutenção FTS com percentual e botão de indexação imediata.
   - `web/app.js`: suporte a `content_only` nos filtros de catálogo, renderização do status FTS em `loadMaintenance`, função `reindexFts()` e bindings de eventos.
   - `web/styles.css`: estilos para marcações de texto `<mark>` nos temas claro e escuro.

6. **Testes e Documentação**:
   - `tests/api-e2e.mjs`: testes de ponta a ponta para `/search`, `/maintenance/fts` e query `content_only`.
   - `docs/api.md` e `README.md` atualizados com as novas rotas e recursos.

### Verificações realmente executadas:
- `cargo clippy --locked --all-targets -- -D warnings`: passou sem nenhum aviso (0 warnings).
- `cargo test --locked`: 41 passaram, 0 falharam, 1 teste de volume ignorado.
- `npm run test:ui`: aprovado (contrato visual, IDs únicos e gravação de progresso).
- `npm run test:api`: aprovado (todas as rotas E2E incluindo FTS5 /search, /maintenance/fts e content_only).
- `npm run test:auth`: aprovado (saúde pública e proteção HTTP Basic).
- `npm run test:browser`: aprovado (Playwright E2E browser test completo).

### Limitações:
- A extração textual é amostral (até 256 KiB por livro) para manter o banco leve e responsivo.
- PDFs baseados exclusivamente em imagens escaneadas (sem camada de OCR) não geram texto extraível.
- Livros muito corrompidos geram conteúdo vazio e não impedem a indexação dos demais livros.

### Próximo passo:
- Apresentar o resultado ao usuário e verificar se há novas demandas ou se deseja realizar commit/push.

---

# Checkpoint — 2026-10-12

## Revisão atual — seis melhorias autorizadas

Implementação concluída. O usuário autorizou documentação, commit e push nesta sessão.
O resultado do envio deve ser conferido no relatório final e no estado atual do Git.
Nenhuma imagem Docker foi construída ou publicada manualmente nesta revisão.

### Mudanças

1. `bounded_io`: limites de bytes reais nas leituras ZIP de EPUB/CBZ, indexação e
   extração de backups (total extraído de até 2 GiB), além dos limites declarados.
2. Exclusão de duplicatas exige SHA-256 integral contra outra cópia e confirmação do nome.
   Originais ficam em `.estante-trash/` na raiz da biblioteca; interface lista/restaura,
   sem sobrescrita. Varredura e Git ignoram a lixeira; backups não incluem originais.
3. Progresso usa IndexedDB antes da rede, confirmação por versão e reenvio ao carregar,
   reconectar, voltar à aba e a cada 15 segundos. Web Locks coordena envios quando disponível.
   A fila é separada por catálogo/raiz; `X-Catalog-Id` recusa envios de abas antigas após
   restauração ou troca de raiz. Falhas de um livro não impedem os demais.
4. SQLite usa thread dedicada, conexão única e fila limitada a 64 operações aguardando.
   As travas de manutenção permanecem até o handler terminar, mesmo com desconexão HTTP.
5. Intervalos simples não satisfazíveis recebem 416, corpo vazio e `Content-Range` adequado.
   Sintaxes não suportadas e múltiplos intervalos continuam ignorados.
6. Prévia e limpeza explícita de capas sem referências e com ao menos sete dias.
   Referências de livros indisponíveis são preservadas; links/subpastas são ignorados.
   A remoção revalida nomes/idade/referências e aguarda outros handlers.

README.md e AGENTS.md atualizados com arquitetura, contratos, uso e limitações.
Os servidores E2E usam cópias isoladas de fixtures em suas pastas de runtime.

### Verificações realmente executadas

- cargo fmt --check: passou após formatação final.
- cargo clippy --locked --all-targets -- -D warnings: passou.
- cargo test --locked: 39 passaram, 0 falharam, 1 teste de volume ignorado.
- cargo test --locked indexes_2287_files -- --ignored: passou (2.287 arquivos).
- npm run test:ui: passou (sintaxe, IDs/layouts e ordem de gravação).
- npm run test:api: passou, incluindo falso positivo de duplicatas, lixeira/restauração,
  Range 416/206, limpeza efetiva de capa antiga e recusa de progresso de catálogo anterior.
- npm run test:auth: passou, incluindo proteção das novas rotas e configuração parcial.
- npm run test:browser: passou, incluindo falha de rede/reload, confirmação de versão
  durante envio pendente, exclusão/restauração pela lixeira e prévia de limpeza.
  Uma execução intermediária falhou por seletor de teste incorreto; corrigido e reexecutado.
- git diff --check: passou.

### Limitações

- IndexedDB não garante a última mudança se a aba fechar antes da transação local concluir.
  Limpar dados do navegador remove pendências. Falha de armazenamento usa tentativa direta
  de rede com aviso. Leitura dos livros ainda requer acesso ao servidor.
- Pendências de um catálogo anterior ficam isoladas no navegador e não são reaplicadas.
- SQLite continua serializado; operações longas ainda fazem outras consultas aguardar.
- Lixeira requer links físicos no mesmo filesystem; submontagens ou sistemas sem suporte
  falham preservando o original. A pasta original precisa existir para restaurar.
  Sem expurgo automático e sem transação durável conjunta entre arquivos e banco.
- Mudanças externas simultâneas no acervo não são coordenadas pela aplicação.
- Capas removidas pela limpeza são excluídas definitivamente; backups anteriores conservam cópias.
- Sem benchmark em acervo privado real ou construção Docker nesta revisão.

Sem pendências de implementação na solicitação atual. Para retomar, leia AGENTS.md,
confira git status/git diff e use tests/with-local-tools.sh com caches dentro do projeto.

---

## Histórico anterior

# Checkpoint — 2026-10-10

## Tarefa atual — verificação do código em busca de melhorias e otimizações

- Realizada auditoria ampla e otimizações no backend Rust, SQLite e frontend web.
- Melhorias e otimizações implementadas:
  1. `src/api.rs`: cabeçalho `Content-Disposition` formatado com RFC 6266 / RFC 5987 (`filename*`), preservando a disposição (`inline`/`attachment`) e nomes com acentos e caracteres UTF-8 (anteriormente nomes não-ASCII causavam falha de parse no HeaderValue e forçavam download via fallback `attachment`). Sanitização de upload para descartar separadores de caminho estilo Windows (`\`).
  2. `src/scanner.rs`: evitado truncamento de inteiros em sistemas 32-bit (ex.: ARMv7) no cálculo de buffer de `partial_content_hash` para arquivos maiores que 4 GiB, reutilizando o buffer na leitura da cauda. Otimizada função `filename_title` eliminando alocações intermediárias de strings e vetores durante varredura.
  3. `src/readers.rs`:
     - Corrigido `strip_active_html` para elementos void (`<embed>`) e tags auto-fechadas (`<tag .../>`), evitando truncamento indevido do restante do livro quando não há tag de fechamento correspondente, e evitando falsos positivos com prefixos de tags (ex.: `<scripture>`).
     - Descompressão PalmDOC em MOBI (`mobi_html`): implementado `palmdoc_decompress_append` para descompressão zero-allocation diretamente no buffer de saída, além de delimitar checagens LZ77 ao início de cada registro.
     - `escape_html`: substituído encadeamento de `.replace()` por escape em passada única com buffer pré-alocado.
     - `natural_key`: evitada alocação de string em `to_ascii_lowercase()` iterando diretamente sobre caracteres.
  4. `src/db.rs`:
     - Protegida adição de fontes de metadados (`validate_provider`) contra SSRF validando URLs públicas via `validate_public_http_url` (impedindo `localhost`, IPs privados ou metadados de nuvem).
     - Otimizada consulta de facetas (`catalog_facets`) delegando agrupamento e ordenação de assuntos (`json_each` + `json_valid`) e anos (`SUBSTR` + `GLOB`) diretamente ao SQLite sem alocação de `BTreeSet` em memória.
     - Varredura em lote (`sync_scan` e `save_scan_issues`): pré-preparadas instruções SQL (`select_stmt`, `upsert_stmt`, `insert_stmt`) fora do loop e timestamp `now` computado uma única vez por lote, acelerando a indexação de grandes acervos.
     - Configurado `PRAGMA synchronous = NORMAL` em conjunto com `journal_mode = WAL` em `open_connection`, reduzindo fsyncs sem comprometer a integridade do banco.
  5. `src/backup.rs`: limpeza de arquivo de destino incompleto em caso de falha durante a criação do pacote zip em `create`.
  6. `src/opds.rs`:
     - Substituído teste síncrono `exists()` por `tokio::fs::try_exists().await` não bloqueante em `unique_destination`.
     - `escape_xml`: substituído encadeamento de 5 `.replace()` por escape em passada única com buffer pré-alocado.
     - `catalog_xml`: evitada alocação desnecessária de `String` no MIME essence ao iterar sobre livros.
  7. `web/app.js`: pausa do temporizador de carrossel de sugestões ao abrir o leitor (`openReader`) e retomada ao fechar (`closeReader`).
- Verificações executadas:
  - `cargo fmt --check`: passou.
  - `cargo clippy --locked --all-targets -- -D warnings`: passou sem avisos (0 warnings).
  - `cargo test --locked`: 33 passaram, 0 falharam, 1 teste de volume ignorado.
  - `cargo test --locked indexes_2287_files -- --ignored`: passou em 0,14s (2.287 arquivos).
  - `npm run test:ui`: passou.
  - `npm run test:api`: passou.
  - `npm run test:auth`: passou.
  - `npm run test:browser`: passou.
  - `git diff --check`: passou sem problemas de espaço ou formatação.
- Próximo passo: pronto para commit ou novas orientações do usuário.

O histórico abaixo se refere às revisões anteriores.

## Retomar

Leia AGENTS.md, confira git status/git diff e este checkpoint antes de editar.
Trabalhe apenas neste projeto. O usuário autorizou commit e push em 2026-09-30.
Nenhum deploy manual foi realizado.

## Estado atual

Concluída a implementação dos cinco problemas e das três melhorias autorizadas
na revisão de 2026-09-30. Alterações preparadas para commit e push em main;
confira git status e o remoto ao retomar para confirmar o envio.

## Implementado nesta revisão

- Posições de notas, progresso e restauração usam models::validate_location:
  objeto de até 8 KiB, campos de página/capítulo inteiros seguros, percentual 0–100.
  Nota sem posição recebe {}; restauração conserva compatibilidade com null legado.
- Rótulos de anotações são escapados antes de inserir HTML, inclusive para dados legados.
- Hrefs EPUB codificados são decodificados antes de procurar as entradas ZIP;
  escapes inválidos e travessia acima da raiz são recusados.
- CSP de leitores permite CSS inline/local no sandbox, conservando bloqueio de scripts/conexões.
- Aplicar metadados prepara a capa antes da gravação. Metadados e referência de capa
  são atualizados juntos; PendingFile limpa arquivos novos em falha/cancelamento.
  Nomes únicos evitam sobrescrever capas existentes. Imagens antigas são conservadas.
- Catálogo e OPDS usam limit (padrão 60, máximo 200) e offset (padrão 0), com total
  filtrado e desempate por ID. Interface navega entre páginas e descarta respostas antigas.
  OPDS fornece link next conservando filtros. Clientes precisam percorrer páginas.
- Renderização PDF usa COVER_TIMEOUT_SECONDS (padrão 60, permitido 1–600),
  encerra/aguarda processo no timeout e permite continuar com o próximo arquivo.
- Inicialização recusa autenticação parcial e usuário com dois-pontos.
- Regressões Rust em tests/rust/, incluídas pelos módulos internos e copiadas pelo Dockerfile.
  Suites API/navegador/autenticação ampliadas; documentação e ambiente atualizados.

## Verificações realmente executadas

Com bash tests/with-local-tools.sh:
- cargo check --locked: passou durante a implementação.
- cargo test --locked: 28 passaram, 0 falharam, 1 teste de volume ignorado.
- npm run test:ui: passou (sintaxe JS, contrato visual e fila de progresso).
- npm run test:api: passou, incluindo paginação, posições inválidas, falhas de metadados
  sem alteração do livro e backup/restauração de nota com posição padrão.
- npm run test:auth: passou, incluindo recusa de configuração parcial na inicialização.
- npm run test:browser: 1 cenário passou, incluindo páginas, filtros, estilos computados
  do leitor e escape de HTML em posição legada.
- git diff --check: passou.
- cargo fmt não pôde executar: rustfmt não está instalado na ferramenta local.
  Nenhuma ferramenta foi instalada nesta revisão.
- A imagem Docker não foi construída nesta revisão.

## Ferramentas locais

Use tests/with-local-tools.sh para Rust, Node, Playwright e temporários dentro de .cache/.
O wrapper não instala ferramentas. Não alterar permissões de target/.

## Limites preservados

- PDF acompanha controles da aplicação, não a navegação interna do visualizador nativo.
  Texto/MOBI usa percentual manual; EPUB salva capítulo e CBZ salva página.
- Sem fila persistente offline nem coordenação entre abas; keepalive não garante entrega após crash.
- Não há transação durável única entre SQLite e imagens nem recuperação conjunta automática
  após crash. Pastas anteriores são preservadas até commit; mounts podem impedir renomeação.
- Validação de backup não é auditoria de todas as constraints; schemas com índices extras são recusados.
  Backups antigos com posições inválidas (exceto null legado de notas) continuam recusados.
- SSRF possui validação DNS/IP/redirects, mas não testes completos de DNS rebinding.
- Limites ZIP ainda usam tamanhos declarados. Range não satisfazível ainda pode retornar 200.
- O teste de volume ignorado não foi executado; desempenho em acervo real não foi medido.
- Paginação por offset pode mudar se o catálogo for alterado entre páginas.
- Imagens antigas não têm coleta automática.

## Próximo passo

Não há pendência na implementação autorizada. Commit e push foram autorizados.
Para publicação de imagem ou deploy manual, confira as instruções do usuário e
valide a construção da imagem antes de prosseguir.
