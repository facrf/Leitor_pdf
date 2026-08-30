# Estante Livre

Biblioteca digital privada e offline-first para PDF, EPUB, MOBI/AZW, CBZ e livros em texto. O backend Rust entrega a API e a interface web responsiva em um único container, com catálogo, progresso, anotações, metadados locais, download e links de compartilhamento revogáveis.

## O que esta primeira versão faz

- Cataloga recursivamente `pdf`, `epub`, `mobi`, `azw`, `azw3`, `cbz`, `txt`, `md`, `html`, `htm` e `fb2`.
- Lê PDF pelo visualizador nativo do navegador; EPUB por capítulos; CBZ por páginas; MOBI/AZW clássico e texto em um leitor isolado.
- Salva página/capítulo/percentual de leitura e anotações no SQLite.
- Permite editar metadados manualmente, pesquisar Open Library/Google Books e salvar a capa localmente.
- Faz upload, download com suporte a `Range` e compartilhamento por token com validade de 7 dias e revogação.
- Não realiza chamadas externas em segundo plano. A rede para metadados começa desativada.

> MOBI/AZW com DRM não é descriptografado. AZW3/KF8 pode usar recursos que o extrator textual inicial ainda não renderiza; o arquivo original sempre permanece disponível para download.

## Início rápido com Docker

Crie as pastas montadas e suba o serviço:

```bash
mkdir -p pdf data
docker compose up --build -d
```

Acesse [http://localhost:20000](http://localhost:20000), abra as configurações e confirme `/pdf`. Coloque arquivos em `./pdf`, use **Adicionar** na interface ou clique em **Atualizar** para varrer a pasta.

Para acompanhar os logs:

```bash
docker compose logs -f estante-livre
```

## Privacidade e funcionamento offline

Todo o catálogo, metadados salvos, progresso e anotações ficam em `./data/library.db`; capas baixadas ficam em `./data/covers`. A leitura, busca local, upload e download não dependem da internet.

A busca externa tem duas travas deliberadas:

1. **Permitir busca externa de metadados** deve ser ativado nas configurações.
2. Uma consulta só ocorre quando o usuário escolhe uma fonte e clica em **Buscar**.

Somente o texto digitado na consulta é enviado ao provedor selecionado. O conteúdo do livro, a biblioteca, as notas e o histórico de leitura nunca são enviados. Capas são obtidas pelo backend durante a aplicação explícita de um resultado e depois servidas localmente; o navegador não carrega imagens de terceiros.

Para uma instalação completamente isolada, mantenha a opção desativada e, se desejado, negue acesso de saída ao container. Open Library e Google Books aparecem como fontes nativas, mas não são contatados automaticamente. Uma chave opcional do Google deve ser fornecida apenas pela variável `GOOGLE_BOOKS_API_KEY`; ela não é salva no banco.

## Configuração

| Variável | Padrão no container | Finalidade |
|---|---:|---|
| `APP_BIND` | `0.0.0.0:20000` | Endereço e porta HTTP |
| `LIBRARY_ROOT` | `/pdf` | Pasta inicial dos livros |
| `DATABASE_PATH` | `/data/library.db` | Arquivo SQLite |
| `COVERS_DIR` | `/data/covers` | Cache local de capas |
| `GOOGLE_BOOKS_API_KEY` | vazio | Chave opcional, somente em memória |
| `RUST_LOG` | `estante_livre=info,tower_http=info` | Nível de logs |

A pasta também pode ser alterada pela interface. Em Docker, ela precisa existir **dentro do container**, portanto monte a pasta do host antes. O valor selecionado é persistido no SQLite.

## Fontes de metadados

As integrações nativas são Open Library e Google Books. Em **Configurações → Fontes de metadados** é possível registrar outro endpoint que implemente o formato JSON de uma dessas APIs. Isso viabiliza proxies próprios ou serviços compatíveis, inclusive uma fonte local.

Amazon e Goodreads não oferecem uma API pública geral e estável para este uso; por isso não são consultados por scraping. Eles podem ser integrados no futuro por um adaptador autorizado ou por um proxy compatível configurado pelo proprietário.

## Arquitetura e dados

```
Navegador ── HTTP ──> Rust/Axum ──> SQLite (/data/library.db)
                         │
                         ├── Biblioteca montada (/pdf)
                         ├── Capas locais (/data/covers)
                         └── Metadados externos (somente sob ação explícita)
```

O servidor entrega tanto a API (`/api`) quanto a interface em `web/`. O SQLite é a única fonte de verdade para catálogo, configurações, progresso, anotações, fontes de metadados e compartilhamentos; os livros originais nunca são copiados para o banco.

| Componente | Responsabilidade |
|---|---|
| `src/scanner.rs` | Varredura da pasta, identificação de formatos e metadados iniciais. |
| `src/readers.rs` | Leitura de EPUB/CBZ, extração de MOBI PalmDOC e conteúdo textual isolado. |
| `src/metadata.rs` | Adaptadores Open Library/Google Books e download local de capas. |
| `src/db.rs` | Esquema e operações SQLite. |
| `src/api.rs` | Rotas HTTP, streaming, uploads, progresso, notas e compartilhamentos. |
| `web/` | Interface responsiva sem dependências ou chamadas a CDNs. |

Consulte também a [documentação de arquitetura](docs/architecture.md) e a [referência detalhada da API](docs/api.md).

## Uso da interface

1. Monte ou escolha a pasta de livros; se nada for alterado, o padrão é `/pdf`.
2. Use **Atualizar** para catalogar os arquivos existentes, ou **Adicionar** para enviar um livro à pasta configurada.
3. Abra um livro para ler, baixar, editar informações ou criar links compartilháveis.
4. Durante a leitura, a posição é salva ao navegar e uma anotação é vinculada à posição atual.
5. Quando precisar de metadados, habilite a busca externa nas configurações, escolha a fonte e confirme a busca.

Os links de compartilhamento criados pela interface expiram em sete dias. A API permite escolher outro prazo de validade.

## API principal

- `POST /api/scan`: sincroniza a pasta com o catálogo.
- `POST /api/upload`: salva um arquivo compatível na raiz da biblioteca.
- `GET /api/books`: lista e pesquisa livros.
- `GET /api/books/:id/file?download=true`: baixa o original.
- `PUT /api/books/:id/progress`: salva a posição.
- `GET|POST /api/books/:id/notes`: consulta/cria notas.
- `GET /api/metadata/search`: consulta explícita a uma fonte habilitada.
- `POST /api/books/:id/shares`: cria um link temporário.

## Desenvolvimento

É necessário o toolchain Rust estável atual:

```bash
cargo fmt --check
cargo test
cargo run
```

Sem Docker, ajuste `LIBRARY_ROOT` e mantenha `DATABASE_PATH`/`COVERS_DIR` em caminhos relativos ao projeto, como mostra o [`.env.example`](.env.example).

## Limites de segurança da primeira versão

O serviço é uma biblioteca pessoal de usuário único e não possui login. Não publique a porta 20000 diretamente na internet. Para acesso remoto, use uma VPN privada ou um proxy reverso com autenticação e TLS. Os links de compartilhamento dão acesso somente ao arquivo associado ao token, até sua expiração ou revogação.
