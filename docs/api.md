# Referência da API

Todas as rotas abaixo usam o prefixo `/api` e retornam JSON, exceto arquivos e recursos de leitura. Erros seguem o formato:

```json
{ "error": "mensagem descritiva" }
```

Rotas mutáveis com o cabeçalho `Origin` exigem que a origem corresponda ao host da requisição. Isso protege instalações locais contra alterações disparadas por outros sites.

## Saúde e configuração

| Método e rota | Finalidade |
|---|---|
| `GET /health` | Estado do serviço. |
| `GET /settings` | Pasta configurada, estado de rede e formatos aceitos. |
| `PUT /settings` | Atualiza `library_root` e `network_metadata_enabled`. |
| `POST /scan` | Varre a biblioteca e sincroniza o catálogo. |

Exemplo de atualização:

```json
{
  "library_root": "/pdf",
  "network_metadata_enabled": false
}
```

## Livros e arquivos

| Método e rota | Finalidade |
|---|---|
| `POST /upload` | Envia `multipart/form-data` com o campo `file`; limite de 1 GiB. |
| `GET /books?q=...` | Lista e filtra por título, autor ou nome do arquivo. |
| `GET /books/{id}` | Retorna dados completos e progresso do livro. |
| `PATCH /books/{id}` | Atualiza metadados locais. |
| `GET /books/{id}/file` | Abre o arquivo original. Aceita `?download=true` e `Range`. |
| `GET /books/{id}/cover` | Retorna a capa armazenada localmente. |

O corpo de `PATCH /books/{id}` aceita `title`, `author`, `description`, `publisher`, `published_date`, `isbn`, `language` e `subjects` (vetor de textos).

## Leitura, progresso e notas

| Método e rota | Finalidade |
|---|---|
| `PUT /books/{id}/progress` | Salva localização JSON e percentual de 0 a 100. |
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

`GET /metadata/search` requer que a busca externa esteja habilitada; caso contrário retorna `403`. Tipos suportados de fonte são `open_library` e `google_books`, apontando para APIs compatíveis.

## Compartilhamento

| Método e rota | Finalidade |
|---|---|
| `GET /books/{id}/shares` | Lista links já criados. |
| `POST /books/{id}/shares` | Cria token de compartilhamento. |
| `DELETE /shares/{id}` | Revoga um token. |
| `GET /public/{token}/file` | Baixa o livro vinculado ao token ativo. |

O corpo de criação aceita opcionalmente `expires_in_hours`, limitado a 1–8760 horas. O token só dá acesso ao arquivo associado, não ao catálogo ou às anotações.
