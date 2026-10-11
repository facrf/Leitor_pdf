# Referência da API

Todas as rotas abaixo usam o prefixo `/api` e retornam JSON, exceto arquivos e recursos de leitura. Erros seguem o formato:

```json
{ "error": "mensagem descritiva" }
```

Rotas mutáveis com o cabeçalho `Origin` exigem que a origem corresponda ao host da requisição. Isso protege instalações locais contra alterações disparadas por outros sites.

Quando `AUTH_USERNAME` e `AUTH_PASSWORD` estão definidos, todas as rotas exigem HTTP Basic, exceto `GET /health` e `GET /public/{token}/file`. Em acesso remoto, use HTTPS.

## Saúde e configuração

| Método e rota | Finalidade |
|---|---|
| `GET /health` | Estado do serviço. |
| `GET /settings` | Pasta, rede, perfil de varredura, carrossel, identidade e formatos aceitos. |
| `PUT /settings` | Atualiza as preferências administrativas. |
| `POST /scan` | Inicia a varredura em segundo plano; uma execução simultânea é reaproveitada. |
| `GET /scan` | Retorna etapa, perfil, totais, percentual, arquivo atual, avisos e erro. |
| `GET /settings/branding/{kind}` | Serve `logo`, `favicon` ou `hero` local. |
| `POST /settings/branding/{kind}` | Salva imagem multipart de até 5 MiB. |
| `DELETE /settings/branding/{kind}` | Exclui a imagem personalizada. |

Exemplo de atualização:

```json
{
  "library_root": "/pdf",
  "network_metadata_enabled": false,
  "scan_profile": "economical",
  "auto_cover_enabled": false,
  "carousel_enabled": true,
  "carousel_interval_seconds": 12
}
```

`scan_profile` aceita `economical`, `balanced` ou `complete`. O intervalo do carrossel aceita 5–300 segundos. As imagens de identidade aceitam PNG, JPEG, WebP, GIF ou ICO detectados pelo conteúdo; SVG é recusado para reduzir a superfície de conteúdo ativo.

Exemplo de estado durante a indexação:

```json
{
  "running": true,
  "phase": "indexing",
  "profile": "economical",
  "total": 2287,
  "processed": 947,
  "percent": 41.407,
  "found": 946,
  "added": 0,
  "updated": 0,
  "unchanged": 945,
  "skipped": 1,
  "current_file": "livro.pdf"
}
```

As fases são `idle`, `discovering`, `indexing`, `synchronizing`, `covers`, `completed` e `failed`. Arquivos com mesmo caminho, tamanho e data de modificação são retornados em `unchanged` e não têm os metadados reextraídos.

## Livros e arquivos

| Método e rota | Finalidade |
|---|---|
| `POST /upload` | Envia `multipart/form-data` com o campo `file`; limite de 1 GiB. |
| `GET /books?...` | Pesquisa, filtra e ordena o catálogo; retorna `books`, `facets`, `total`, `limit` e `offset`. |
| `GET /reading-desk` | Lista livros com progresso maior que 0 e menor que 100. |
| `GET /suggestions` | Retorna até 10 livros disponíveis em ordem aleatória. |
| `GET /books/{id}` | Retorna dados completos e progresso do livro. |
| `PATCH /books/{id}` | Atualiza metadados locais. |
| `GET /books/{id}/file` | Abre o arquivo original. Aceita `?download=true` e `Range`. |
| `DELETE /books/{id}/file` | Exclui o original após confirmação nominal e marca o item indisponível. |
| `GET /books/{id}/cover` | Retorna a capa armazenada localmente. |

O corpo de `PATCH /books/{id}` aceita `title`, `author`, `description`, `publisher`, `published_date`, `isbn`, `language` e `subjects` (vetor de textos).

Parâmetros de `GET /books`:

| Parâmetro | Valores |
|---|---|
| `q` | Texto em título, autor, arquivo, descrição, editora, ISBN ou assuntos |
| `format`, `author`, `subject`, `publisher`, `language`, `year` | Correspondência da faceta |
| `min_size`, `max_size` | Bytes, inclusivos |
| `progress` | `unread`, `reading` ou `finished` |
| `availability` | `available` ou `missing` |
| `collection_id` | ID numérico da coleção |
| `limit`, `offset` | Tamanho de página (1–200, padrão 60) e deslocamento inteiro sem sinal (padrão 0) |
| `sort` | `title`, `author`, `year`, `recent`, `size` ou `progress` |

Exclusão exige JSON `{ "filename": "nome-exato.pdf" }`. O servidor compara com a entrada catalogada, resolve novamente o caminho dentro da raiz e não oferece recuperação interna.

## Coleções

| Método e rota | Finalidade |
|---|---|
| `GET /collections` | Lista coleções e quantidade de livros. |
| `POST /collections` | Cria com `name` e `color` hexadecimal. |
| `PUT /collections/{id}` | Renomeia e altera a cor. |
| `DELETE /collections/{id}` | Exclui a coleção, sem excluir livros. |
| `PUT /collections/{id}/books/{book_id}` | Adiciona um livro. |
| `DELETE /collections/{id}/books/{book_id}` | Remove um livro. |

`GET /books/{id}` também retorna `collections` e `book_collection_ids`.

## Leitura, progresso e notas

Ao salvar progresso, `location` deve ser um objeto JSON de até 8 KiB serializado e `percent` deve ser finito, entre 0 e 100. Entradas inválidas recebem HTTP 400.

| Método e rota | Finalidade |
|---|---|
| `PUT /books/{id}/progress` | Salva localização JSON e percentual de 0 a 100. |
| `DELETE /books/{id}/progress` | Remove o progresso e, portanto, o livro da mesa de leitura. |
| `GET /books/{id}/notes` | Lista anotações. |
| `POST /books/{id}/notes` | Cria anotação ligada à localização. |
| `DELETE /notes/{id}` | Exclui uma anotação. |
| `GET /books/{id}/epub` | Manifesto de capítulos EPUB. |
| `GET /books/{id}/epub/resource/{caminho}` | Recurso interno de EPUB. |
| `GET /books/{id}/comic` | Manifesto de páginas CBZ. |
| `GET /books/{id}/comic/resource/{caminho}` | Imagem interna de CBZ. |
| `GET /books/{id}/text` | Leitor para texto/HTML/FB2. |
| `GET /books/{id}/mobi` | Leitor para MOBI/AZW sem DRM. |

Exemplo de progresso por página:

```json
{
  "location": { "type": "page", "page": 42 },
  "percent": 37.5
}
```

Exemplo de anotação:

```json
{
  "location": { "type": "chapter", "chapter": 3 },
  "content": "Retomar a discussão deste conceito."
}
```

## Metadados

| Método e rota | Finalidade |
|---|---|
| `GET /metadata/providers` | Lista fontes configuradas. |
| `POST /metadata/providers` | Adiciona fonte compatível. |
| `DELETE /metadata/providers/{id}` | Remove uma fonte não nativa. |
| `GET /metadata/search?provider_id=...&q=...` | Pesquisa uma fonte habilitada. |
| `PUT /books/{id}/metadata` | Aplica candidato e baixa a capa local, se houver. |

`GET /metadata/search` requer que a busca externa esteja habilitada; caso contrário retorna `403`. Tipos suportados de fonte são `open_library` e `google_books`, apontando para APIs compatíveis. O endereço base (`base_url`) de provedores adicionados via `POST /metadata/providers` é validado contra SSRF e deve ser uma URL pública HTTP/HTTPS válida.

## Compartilhamento

| Método e rota | Finalidade |
|---|---|
| `GET /books/{id}/shares` | Lista links já criados. |
| `POST /books/{id}/shares` | Cria token de compartilhamento. |
| `DELETE /shares/{id}` | Revoga um token. |
| `GET /public/{token}/file` | Baixa o livro vinculado ao token ativo. |

O corpo de criação aceita opcionalmente `expires_in_hours`, limitado a 1–8760 horas. O token só dá acesso ao arquivo associado, não ao catálogo ou às anotações.

## Saúde, duplicatas e capas

| Método e rota | Finalidade |
|---|---|
| `GET /maintenance/health` | Totais, ausentes, metadados/capas pendentes, duplicatas e até 100 avisos. |
| `GET /maintenance/duplicates` | Grupos de arquivos com mesmo tamanho e assinatura parcial SHA-256. |
| `POST /maintenance/covers` | Inicia geração sequencial de capas PDF; o progresso usa `GET /scan`. |

A assinatura usa tamanho + primeiros/últimos 64 KiB para evitar a leitura integral do acervo; os grupos são candidatos prováveis. `POST /maintenance/covers` reaproveita a trava da varredura: apenas uma tarefa pesada roda por vez.

## Backup e restauração

| Método e rota | Finalidade |
|---|---|
| `GET /maintenance/backups` | Lista pacotes locais. |
| `POST /maintenance/backups` | Cria snapshot ZIP consistente. |
| `GET /maintenance/backups/{filename}` | Baixa o pacote. |
| `DELETE /maintenance/backups/{filename}` | Exclui o pacote nomeado. |
| `POST /maintenance/restore` | Restaura o primeiro campo multipart enviado. |

A restauração aceita no máximo 2 GiB, bloqueia Zip Slip e entradas inesperadas, valida o SQLite e cria um backup preventivo. O retorno informa `safety_backup` e `original_books_unchanged`. Os originais nunca entram no pacote. ZIPs inválidos, inseguros ou sem `library.db` recebem `400` e não alteram o catálogo; erros internos de disco ou banco continuam sendo `500`.

## OPDS

| Método e rota | Finalidade |
|---|---|
| `GET /opds` | Catálogo Atom/OPDS; aceita os filtros de livros. |
| `POST /opds/import` | Baixa itens de um feed informado em `{ "url": "https://..." }`. |

A importação exige `network_metadata_enabled=true`, feed público HTTP/HTTPS de até 5 MiB, no máximo 100 entradas e 1 GiB por arquivo. A resposta `report` contém `imported`, `skipped` e `warnings`; depois é necessário iniciar a varredura.

## Validações e páginas

`GET /books` retorna o total de livros que correspondem aos filtros, independentemente
da página. A ordenação inclui o ID como desempate. Exemplo: `/books?limit=60&offset=60`.
`GET /opds` aceita a mesma paginação e inclui um link Atom `rel="next"` com os filtros atuais.
Paginação por deslocamento pode mudar se o catálogo for alterado entre requisições.

`location` deve ser um objeto JSON de até 8 KiB. `page` é um inteiro positivo;
`page_index` e `chapter` são inteiros não negativos, até o maior inteiro seguro do JavaScript.
`percent`, quando presente, fica entre 0 e 100. `type` e `href`, quando presentes, são textos.
Em notas, omitir `location` equivale a `{}`; `null`, vetores e campos numéricos inválidos são recusados.
A restauração aceita notas antigas com posição `null`, normalizando-as para `{}`.

Aplicar metadados com capa valida consentimento e baixa a imagem antes de alterar o catálogo.
Metadados e referência da capa são gravados juntos. Uma falha anterior à gravação preserva
os valores existentes e limpa o novo arquivo incompleto.

A inicialização exige ambas as credenciais HTTP Basic ou ambas vazias.
`COVER_TIMEOUT_SECONDS` aceita 1–600 segundos (padrão 60); ao exceder esse prazo,
a geração registra um aviso e continua com o próximo PDF.
