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
