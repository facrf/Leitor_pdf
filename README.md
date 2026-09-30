# Estante Livre

Biblioteca digital privada e offline-first para PDF, EPUB, MOBI/AZW, CBZ e livros em texto. O backend Rust entrega a API e a interface web responsiva em um único container, com indexação incremental, busca avançada, coleções, diagnóstico, backup, OPDS e autenticação opcional.

## Recursos

- Cataloga recursivamente `pdf`, `epub`, `mobi`, `azw`, `azw3`, `cbz`, `txt`, `md`, `html`, `htm` e `fb2`.
- Lê PDF pelo visualizador nativo do navegador; EPUB por capítulos; CBZ por páginas; MOBI/AZW clássico e texto em um leitor isolado.
- Salva página/capítulo/percentual de leitura e anotações no SQLite.
- As mudanças pelos controles do leitor são salvas após 700 ms e ao fechar o leitor. Ao ocultar/sair da aba, há uma tentativa adicional com `keepalive`, sem garantia em caso de encerramento abrupto ou falta de rede. PDF usa a página informada nos controles da aplicação (não acompanha o visualizador nativo); texto/MOBI usa o percentual manual.
- Reúne automaticamente livros iniciados em **Minha mesa de leitura** e permite retirá-los dela sem apagar o arquivo.
- Filtra por formato, autor, assunto, coleção, editora, idioma, ano, tamanho, disponibilidade e progresso; ordena por título, autor, ano, atualização, tamanho ou leitura.
- Organiza livros em coleções coloridas editáveis, sem mover os arquivos no disco.
- Executa a varredura em segundo plano, mostrando etapa, arquivo atual, quantidade e porcentagem indexada.
- Reaproveita metadados e assinatura de arquivos cujo tamanho e data não mudaram, tornando as atualizações seguintes incrementais.
- Oferece perfis econômico, equilibrado e completo para controlar o impacto de CPU e disco.
- Rejeita metadados PDF corrompidos e usa o nome legível do arquivo como título seguro.
- Localiza prováveis duplicatas pela assinatura SHA-256 de amostras do conteúdo e exige a digitação exata do nome antes de apagar um original.
- Exibe um painel de saúde com arquivos ausentes, campos incompletos, capas pendentes, duplicatas e avisos da última varredura.
- Exibe, quando ativado pelo administrador, um carrossel aleatório formado somente por livros locais.
- Permite trocar ou excluir logo, favicon e imagem de abertura, armazenados localmente.
- Gera capas de PDF pela primeira página, sequencialmente e com baixa prioridade (`nice` + Poppler).
- Cria e restaura pacotes ZIP do catálogo, progresso, notas, coleções, capas e identidade; os livros originais não entram no pacote.
- Durante a restauração, os demais handlers da API aguardam sua conclusão. Se já houver varredura ou geração de capas em segundo plano, a restauração é recusada. O banco é restaurado pela API transacional de backup do SQLite, preservando a conexão. As pastas de imagens anteriores são mantidas até o commit do banco e repostas em caso de falha; erros de recuperação são registrados nos logs. Isso não garante recuperação conjunta de banco e imagens após queda do processo ou do sistema.
- Exporta o catálogo em OPDS e importa, mediante confirmação e consentimento de rede, até 100 itens de outro catálogo.
- Protege a API opcionalmente com HTTP Basic definido somente por variáveis de ambiente.
- Permite editar metadados manualmente, pesquisar Open Library/Google Books e salvar a capa localmente.
- Faz upload, download com suporte a `Range` e compartilhamento por token com validade de 7 dias e revogação.
- Originais HTML e outros formatos ativos são entregues como download com isolamento CSP; a leitura HTML continua disponível pelo leitor dedicado. Capas só podem ser servidas de dentro da pasta configurada.
- Não realiza chamadas externas em segundo plano. A rede para metadados começa desativada.

> MOBI/AZW com DRM não é descriptografado. AZW3/KF8 pode usar recursos que o extrator textual inicial ainda não renderiza; o arquivo original sempre permanece disponível para download.

## Instalação com imagem oficial

As imagens oficiais são publicadas pelo GitHub Actions no GitHub Container Registry (GHCR). O Compose não possui `build`: instalações locais apenas baixam uma imagem já publicada.

Antes do primeiro uso, troque `facrf` pelo proprietário do repositório GitHub, caso necessário, em `ESTANTE_IMAGE` ou no [compose.yaml](compose.yaml). Crie as pastas montadas e suba o serviço:

```bash
mkdir -p pdf data
ESTANTE_IMAGE=ghcr.io/facrf/estante-livre:latest docker compose up -d
```

Acesse [http://localhost:20000](http://localhost:20000), abra as configurações e confirme `/pdf`. Coloque arquivos em `./pdf`, use **Adicionar** na interface ou clique em **Atualizar** para varrer a pasta.

Para acompanhar os logs:

```bash
docker compose logs -f estante-livre
```

### Portainer

No Portainer, abra **Stacks → Add stack → Web editor**, defina `BOOKS_PATH` como a pasta absoluta onde ficam os livros no host e cole o YAML abaixo. Troque a imagem se o proprietário do pacote no GHCR for diferente de `facrf`.

```yaml
services:
  estante-livre:
    image: ${ESTANTE_IMAGE:-ghcr.io/facrf/estante-livre:latest}
    container_name: estante-livre
    restart: unless-stopped
    ports:
      - "20000:20000"
    environment:
      APP_BIND: 0.0.0.0:20000
      LIBRARY_ROOT: /pdf
      DATABASE_PATH: /data/library.db
      COVERS_DIR: /data/covers
      BRANDING_DIR: /data/branding
      BACKUP_DIR: /data/backups
      COVER_TIMEOUT_SECONDS: ${COVER_TIMEOUT_SECONDS:-60}
      AUTH_USERNAME: ${AUTH_USERNAME:-}
      AUTH_PASSWORD: ${AUTH_PASSWORD:-}
      GOOGLE_BOOKS_API_KEY: ${GOOGLE_BOOKS_API_KEY:-}
      RUST_LOG: estante_livre=info,tower_http=info
    volumes:
      - estante_data:/data
      - type: bind
        source: ${BOOKS_PATH}
        target: /pdf

volumes:
  estante_data:
```

No formulário de variáveis do Stack, use por exemplo:

| Variável | Exemplo | Obrigatória |
|---|---|---|
| `BOOKS_PATH` | `/srv/estante-livre/pdf` | Sim |
| `ESTANTE_IMAGE` | `ghcr.io/facrf/estante-livre:latest` | Não |
| `GOOGLE_BOOKS_API_KEY` | chave opcional | Não |
| `AUTH_USERNAME` / `AUTH_PASSWORD` | credenciais da API | Não |

O processo no container usa o usuário `estante` (UID/GID `10001`). A pasta apontada por `BOOKS_PATH` precisa ser legível; para permitir upload pela interface, ela também precisa permitir escrita para esse usuário. Se o pacote GHCR for privado, cadastre no Portainer um *registry credential* com token de leitura de pacotes antes de criar o Stack.

### Publicação pelo GitHub Actions

O workflow [publish-image.yml](.github/workflows/publish-image.yml) é o único fluxo de publicação da imagem oficial. Ele usa `GITHUB_TOKEN`, portanto não há token de registro salvo no repositório, e publica manifestos para:

- `linux/amd64`
- `linux/arm64`
- `linux/arm/v7`

Essas são plataformas de contêiner Linux. Um binário portátil nativo para Windows (`win64`) é uma entrega futura separada e não faz parte de uma imagem OCI Linux.

Um push em `main` atualiza a tag `latest`; uma tag Git como `v1.0.0` publica também as tags de versão. Como o remoto atual pode não ser o GitHub, espelhe ou envie o repositório para o GitHub e habilite em **Settings → Actions → General** a permissão de leitura e escrita para workflows. Em seguida, torne o pacote público ou forneça credenciais de leitura aos servidores Portainer.

## Privacidade e funcionamento offline

Todo o catálogo, metadados salvos, progresso, configurações, coleções e anotações ficam em `./data/library.db`; capas ficam em `./data/covers`, a identidade visual em `./data/branding` e os backups em `./data/backups`. A leitura, busca local, upload, sugestões, capas de PDF e download não dependem da internet.

A busca externa tem duas travas deliberadas:

1. **Permitir busca externa de metadados** deve ser ativado nas configurações.
2. Uma consulta só ocorre quando o usuário escolhe uma fonte e clica em **Buscar**.

Somente o texto digitado na consulta é enviado ao provedor selecionado. O conteúdo do livro, a biblioteca, as notas e o histórico de leitura nunca são enviados. Capas são obtidas pelo backend durante a aplicação explícita de um resultado e depois servidas localmente; o navegador não carrega imagens de terceiros.

Para uma instalação completamente isolada, mantenha a opção desativada e, se desejado, negue acesso de saída ao container. Open Library e Google Books aparecem como fontes nativas, mas não são contatados automaticamente. Uma chave opcional do Google deve ser fornecida apenas pela variável `GOOGLE_BOOKS_API_KEY`; ela não é salva no banco.

Downloads de capas e importações OPDS aceitam somente destinos públicos: os IPs do DNS e cada redirecionamento são validados e fixados na conexão, sem proxy de ambiente. Provedores de metadados configurados pelo administrador continuam podendo usar endpoints locais. Capas e feeds têm limites durante a transferência (10 MiB e 5 MiB); uploads e livros OPDS incompletos são removidos em falhas ou cancelamento, com aviso nos logs se a limpeza falhar.

## Configuração

| Variável | Padrão no container | Finalidade |
|---|---:|---|
| `APP_BIND` | `0.0.0.0:20000` | Endereço e porta HTTP |
| `LIBRARY_ROOT` | `/pdf` | Pasta inicial dos livros |
| `DATABASE_PATH` | `/data/library.db` | Arquivo SQLite |
| `COVERS_DIR` | `/data/covers` | Cache local de capas |
| `BRANDING_DIR` | `/data/branding` | Logo, favicon e imagem de abertura locais |
| `BACKUP_DIR` | `/data/backups` | Pacotes de backup gerados localmente |
| `COVER_TIMEOUT_SECONDS` | `60` | Prazo por PDF para gerar capa (1 a 600 segundos) |
| `AUTH_USERNAME` | vazio | Usuário HTTP Basic; exige também a senha |
| `AUTH_PASSWORD` | vazio | Senha HTTP Basic; nunca é persistida no SQLite |
| `GOOGLE_BOOKS_API_KEY` | vazio | Chave opcional, somente em memória |
| `RUST_LOG` | `estante_livre=info,tower_http=info` | Nível de logs |

A pasta também pode ser alterada pela interface. Em Docker, ela precisa existir **dentro do container**, portanto monte a pasta do host antes. O valor selecionado e as preferências de varredura/carrossel são persistidos no SQLite.

## Varredura de acervos grandes

Ao clicar em **Atualizar**, `POST /api/scan` inicia um trabalho em segundo plano e a faixa superior consulta `GET /api/scan` até o fim. Depois da descoberta inicial, o percentual é calculado por `arquivos processados / arquivos compatíveis encontrados`. A sincronização final com o SQLite ocorre em uma transação curta. Tamanho e data de modificação com precisão de nanossegundos identificam arquivos inalterados; título, autor, páginas e assinatura já conhecidos são reutilizados sem reabrir PDF/EPUB.

Para um acervo como 150 GB em aproximadamente 2.287 livros, comece pelo perfil **Econômico**:

| Perfil | Estratégia | Uso indicado |
|---|---|---|
| Econômico | Não abre o conteúdo; cataloga pelo nome do arquivo, usa um worker e pequenas pausas. | Primeira carga e discos lentos/NAS. |
| Equilibrado | Extrai título, autor e páginas, com ciclo de trabalho alvo de aproximadamente 65%. | Enriquecimento posterior sem monopolizar um núcleo. |
| Completo | Extrai metadados sem pausas artificiais. | Servidor ocioso e varreduras menores. |

Os percentuais são limites cooperativos do worker de indexação, não cotas rígidas do sistema operacional. Para isolamento absoluto, também é possível limitar o container a metade de um núcleo, por exemplo com `docker update --cpus 0.50 estante-livre`. A varredura é sempre sequencial e nunca cria um worker por livro.

Metadados PDF são decodificados como UTF-8, UTF-16 ou Windows-1252 e passam por uma validação de legibilidade. Valores com caracteres de substituição, controles ou aparência binária são descartados. Na varredura seguinte, títulos antigos reconhecidos como corrompidos são reparados com o nome do arquivo; títulos válidos ou editados manualmente são preservados.

## Fontes de metadados

A indexação registra se já tentou extrair metadados de PDF/EPUB. Mudar do perfil econômico para equilibrado/completo extrai os dados pendentes, mesmo sem alteração do arquivo. Edições manuais e resultados externos aplicados ficam protegidos. Catálogos anteriores a esse controle preservam seus títulos/autores existentes, pois não há histórico para distinguir edições manuais. Arquivos inválidos não são reabertos repetidamente enquanto tamanho/data permanecerem iguais.

As integrações nativas são Open Library e Google Books. Em **Configurações → Fontes de metadados** é possível registrar outro endpoint que implemente o formato JSON de uma dessas APIs. Isso viabiliza proxies próprios ou serviços compatíveis, inclusive uma fonte local.

Amazon e Goodreads não oferecem uma API pública geral e estável para este uso; por isso não são consultados por scraping. Eles podem ser integrados no futuro por um adaptador autorizado ou por um proxy compatível configurado pelo proprietário.

## Arquitetura e dados

```
Navegador ── HTTP ──> Rust/Axum ──> SQLite (/data/library.db)
                         │
                         ├── Biblioteca montada (/pdf)
                         ├── Capas locais (/data/covers)
                         ├── Identidade local (/data/branding)
                         └── Metadados externos (somente sob ação explícita)
```

O servidor entrega tanto a API (`/api`) quanto a interface em `web/`. O SQLite é a única fonte de verdade para catálogo, configurações, progresso, anotações, fontes de metadados e compartilhamentos; os livros originais nunca são copiados para o banco.

| Componente | Responsabilidade |
|---|---|
| `src/scanner.rs` | Descoberta, progresso, perfis de carga, identificação de formatos e metadados seguros. |
| `src/readers.rs` | Leitura de EPUB/CBZ, extração de MOBI PalmDOC e conteúdo textual isolado. |
| `src/metadata.rs` | Adaptadores Open Library/Google Books e download local de capas. |
| `src/db.rs` | Esquema e operações SQLite. |
| `src/api.rs` | Rotas HTTP, streaming, uploads, progresso, notas e compartilhamentos. |
| `web/` | Interface responsiva sem dependências ou chamadas a CDNs. |

Consulte também a [documentação de arquitetura](docs/architecture.md), a [referência detalhada da API](docs/api.md) e o guia de [HTTPS com Caddy ou Traefik](docs/https.md).

## Uso da interface

1. Monte ou escolha a pasta de livros; se nada for alterado, o padrão é `/pdf`.
2. Em configurações, escolha o perfil de varredura e, se desejar, ative capas automáticas/carrossel e personalize logo/favicon/abertura.
3. Use **Atualizar** para catalogar os arquivos existentes; acompanhe a quantidade e porcentagem na faixa de progresso.
4. Use os filtros principais e a busca avançada; abra a ficha para editar dados e associar coleções.
5. Abra um livro para ler, baixar, editar informações ou criar links compartilháveis. Ao iniciar, ele entra na mesa de leitura.
6. Durante a leitura, a posição é salva ao navegar e uma anotação é vinculada à posição atual.
7. Quando precisar de metadados, habilite a busca externa nas configurações, escolha a fonte e confirme a busca.
8. Em **Saúde e manutenção**, revise pendências, duplicatas, backups, capas e OPDS.

Os links de compartilhamento criados pela interface expiram em sete dias. A API permite escolher outro prazo de validade.

## API principal

- `POST /api/scan`: inicia a varredura em segundo plano.
- `GET /api/scan`: informa etapa, quantidades, percentual, avisos e resultado.
- `POST /api/upload`: salva um arquivo compatível na raiz da biblioteca.
- `GET /api/books`: lista, pesquisa e filtra livros; também retorna facetas disponíveis.
- `GET /api/reading-desk`: lista leituras iniciadas.
- `GET /api/suggestions`: sorteia sugestões locais.
- `GET /api/books/:id/file?download=true`: baixa o original.
- `PUT /api/books/:id/progress`: salva a posição.
- `DELETE /api/books/:id/progress`: retira o livro da mesa de leitura.
- `GET|POST /api/books/:id/notes`: consulta/cria notas.
- `GET /api/metadata/search`: consulta explícita a uma fonte habilitada.
- `POST /api/books/:id/shares`: cria um link temporário.
- `GET|POST /api/collections`: lista/cria coleções.
- `GET /api/maintenance/health`: diagnostica o acervo.
- `GET /api/maintenance/duplicates`: agrupa prováveis duplicatas.
- `GET|POST /api/maintenance/backups`: lista/cria backups.
- `POST /api/maintenance/restore`: restaura um pacote com backup preventivo.
- `POST /api/maintenance/covers`: inicia capas PDF em segundo plano.
- `GET /api/opds` e `POST /api/opds/import`: exporta/importa OPDS.

## Desenvolvimento

Quando ferramentas locais estiverem disponíveis em `.cache/`, use `bash tests/with-local-tools.sh cargo test --locked` e `bash tests/with-local-tools.sh npm run test:browser` (também `test:ui`, `test:api` e `test:auth`). O wrapper configura os caches e artefatos dentro do projeto, sem alterar o perfil do shell. A pasta `.cache/` é ignorada pelo Git; o wrapper não instala ferramentas.

Backups restaurados precisam ter o schema reconhecido, referências válidas, caminhos relativos seguros e dados de leitura válidos. A restauração preserva a pasta da biblioteca e a permissão de rede atuais; não ativa consultas externas por configuração trazida de outro servidor. Os diretórios de extração são limpos ao finalizar, falhar ou cancelar a operação (sem garantia em caso de encerramento forçado do processo).

É necessário o toolchain Rust estável atual:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
npm ci
npm run test:ui
npm run test:api
npm run test:auth
npx playwright install chromium
npm run test:browser
cargo run
```

O teste de volume com exatamente 2.287 arquivos é deliberadamente explícito: `cargo test indexes_2287_files -- --ignored`. Veja o [guia de operação e recuperação](docs/operations.md).

O GitHub Actions repete essas verificações, valida o Compose e constrói a imagem antes de publicar no GHCR. Em falhas do navegador, relatório, screenshots, vídeo e trace do Playwright ficam disponíveis como artefato do workflow por sete dias.

Sem Docker, ajuste `LIBRARY_ROOT` e mantenha `DATABASE_PATH`/`COVERS_DIR` em caminhos relativos ao projeto, como mostra o [`.env.example`](.env.example).

## Segurança de acesso

O serviço continua sendo uma biblioteca de usuário único. Quando `AUTH_USERNAME` e `AUTH_PASSWORD` estão ambos preenchidos, todas as rotas da API exigem HTTP Basic, exceto saúde e links públicos por token. Definir apenas uma credencial impede a inicialização; o usuário não pode conter dois-pontos. A interface aciona o diálogo nativo de credenciais do navegador. HTTP Basic não cifra a senha: fora da máquina local, use VPN ou proxy reverso com HTTPS e nunca publique a porta 20000 diretamente.

Excluir uma duplicata remove o arquivo original da pasta e não há lixeira interna; a interface exige repetir exatamente o nome. Backups administrativos não incluem os livros, portanto mantenha também uma cópia separada de `/pdf`.

Para acesso remoto, não exponha diretamente `20000`. Use VPN ou o exemplo documentado de [proxy reverso com HTTPS](docs/https.md).

## Licença e autoria

Copyright © 2026 **FACRF** — [www.fabianocesar.com](https://www.fabianocesar.com).

Este projeto é software livre, distribuído sob a **GNU General Public License versão 3 ou posterior** (`GPL-3.0-or-later`). Consulte [LICENSE](LICENSE). A identificação “Desenvolvido por FACRF.” aparece discretamente no rodapé da biblioteca e das configurações e direciona ao site do autor.

### Revisão de robustez e paginação

O catálogo e o OPDS usam `limit` (padrão 60, máximo 200) e `offset` (padrão 0).
A interface oferece páginas anterior/próxima, conserva os filtros e mostra o total;
o OPDS inclui `rel="next"`. Clientes da API devem percorrer as páginas para obter todo o acervo.
Anotações e progresso validam a mesma estrutura de posição usada na restauração.
Referências EPUB codificadas são decodificadas antes da busca no ZIP. Leitores isolados
permitem CSS local/inline, mantendo scripts e conexões bloqueados.
Aplicar metadados prepara a capa antes de gravar título e referência da imagem juntos;
falhas de validação, consentimento ou download preservam os dados anteriores.
Capas novas têm nomes únicos; capas antigas são conservadas para evitar interferência
com downloads e backups em andamento. Não há coleta automática dessas imagens antigas.
Cada renderização PDF tem prazo configurável e encerra o processo ao excedê-lo.
