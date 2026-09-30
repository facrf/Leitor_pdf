# Arquitetura

## Objetivo

Estante Livre é uma biblioteca digital de usuário único, projetada para manter arquivos, catálogo e hábitos de leitura sob controle do proprietário. A aplicação funciona integralmente sem internet; a rede é um recurso opcional e desligado inicialmente.

## Componentes

```text
web/ (HTML, CSS e JavaScript)
          │
          ▼
src/api.rs ─────── src/db.rs ─────── SQLite
    │                  │
    │                  ├── catálogo e metadados
    │                  ├── progresso e anotações
    │                  └── links de compartilhamento
    │
    ├── src/auth.rs ───── HTTP Basic opcional por ambiente
    ├── src/scanner.rs ─── /pdf (worker sequencial + cache incremental)
    ├── src/readers.rs ─── arquivos EPUB/CBZ/MOBI/texto
    ├── src/metadata.rs ── fontes habilitadas pelo usuário
    ├── src/covers.rs ─── Poppler em baixa prioridade
    ├── src/opds.rs ───── exportação/importação explícita
    ├── src/pending_file.rs ─ limpeza de arquivos incompletos
    └── src/backup.rs ─── ZIP do SQLite e recursos locais
```

O processo Rust serve a interface estática e a API no mesmo endereço. Não há dependência de Node.js, PHP ou serviço externo em produção.

## Persistência

O banco SQLite contém as tabelas abaixo:

| Tabela | Conteúdo |
|---|---|
| `settings` | Pasta, consentimento de rede, perfil da varredura, carrossel e referências da identidade. |
| `books` | Caminho relativo, formato, metadados, disponibilidade e referência de capa. |
| `reading_progress` | Localização JSON e percentual da última leitura de cada livro. |
| `notes` | Texto da anotação e localização JSON associada. |
| `metadata_providers` | Fontes nativas e fontes compatíveis adicionadas pelo usuário. |
| `shares` | Token, livro, criação, expiração e revogação. |
| `collections` | Nome, cor e criação das listas do usuário. |
| `collection_books` | Relação muitos-para-muitos entre coleções e livros. |
| `scan_issues` | Avisos limitados da varredura mais recente para o painel de saúde. |

Os arquivos originais permanecem na pasta da biblioteca. A varredura usa somente caminhos relativos no catálogo e verifica se o arquivo continua dentro da pasta configurada antes de servi-lo.

## Indexação e controle de carga

`POST /scan` registra imediatamente um `ScanStatus` em memória e inicia uma tarefa assíncrona. A leitura do disco ocorre em um único `spawn_blocking`; callbacks curtos atualizam o retrato consultado por `GET /scan`. Uma segunda solicitação não cria concorrência: recebe o estado do trabalho existente.

```text
discovering ──> indexing (processado / total) ──> synchronizing ──> completed
                                      └──────────────────────────> failed
```

Antes da leitura, o scanner carrega do SQLite um índice por caminho com tamanho, data em nanossegundos, título, autor, páginas e assinatura. Entradas idênticas reutilizam esse registro (`unchanged`). Arquivos novos ou alterados recebem uma assinatura SHA-256 do tamanho + 64 KiB iniciais/finais, suficiente para selecionar prováveis duplicatas sem ler os 150 GB completos.

O perfil econômico não abre PDF/EPUB e usa o nome do arquivo. O equilibrado extrai metadados e intercala trabalho com descanso proporcional, visando aproximadamente 65% de ciclo ativo do worker. O completo não adiciona pausas. Ao terminar, todos os resultados são sincronizados em uma transação SQLite; arquivos ausentes passam a indisponíveis. A mesma trava em memória impede que varredura e geração de capas rodem simultaneamente.

O saneamento de metadados reconhece UTF-8, UTF-16 BE/LE e Windows-1252. Textos com substituições Unicode, controles, tamanho excessivo ou alta densidade de símbolos são rejeitados. `metadata_scanned` registra a tentativa de extração PDF/EPUB, permitindo enriquecer arquivos inalterados ao trocar do perfil econômico para completo. `metadata_edited` protege alterações manuais e resultados externos durante o `upsert`; títulos corrompidos continuam reparáveis. Livros legados têm título/autor preservados por precaução.

## Formatos e leitura

| Formato | Estratégia |
|---|---|
| PDF | O arquivo original é servido com suporte a `Range`; o navegador o exibe. A página é informada no controle de leitura. |
| EPUB | O OPF e a sequência de capítulos são lidos do ZIP; capítulos e recursos são servidos sob demanda. |
| CBZ | Imagens do arquivo ZIP são ordenadas naturalmente e exibidas página a página. |
| MOBI/AZW | O texto PalmDOC não criptografado é extraído para um leitor isolado. |
| TXT/MD/HTML/HTM/FB2 | Conteúdo textual servido em leitor isolado. |

Conteúdo de EPUB, MOBI e HTML é exibido em `iframe` com sandbox e política de conteúdo restritiva. Scripts, conexões externas e formulários são bloqueados. Arquivos com DRM não são removidos nem descriptografados.

## Privacidade e fronteiras de rede

Por padrão, `network_metadata_enabled` é `false`. Com esse valor, a API de pesquisa retorna `403` e nenhuma chamada é feita.

Quando habilitado, somente três fluxos usam rede:

1. A pesquisa de metadados envia o texto digitado pelo usuário à fonte selecionada.
2. Ao aplicar um resultado com capa, o backend baixa a imagem e passa a servi-la localmente.
3. Uma importação OPDS confirmada baixa o feed e os livros selecionáveis nele.

O navegador nunca recebe uma URL remota de capa. Hosts locais e endereços IP privados são recusados para capas e OPDS. Limites de tamanho, contagem e extensão protegem downloads. O serviço também bloqueia requisições mutáveis que tragam um cabeçalho `Origin` diferente do host atual.

## Backup e troca do banco

O backup usa `VACUUM INTO` sob o mutex da conexão, obtendo um arquivo SQLite consistente sem copiar WAL em uso. O ZIP inclui somente banco, capas, identidade e manifesto. Na restauração, a extração ocorre em pasta exclusiva dentro de `BACKUP_DIR`, com `enclosed_name`, lista de prefixos permitidos, até 10.000 entradas e 2 GiB declarados descompactados. O pacote acompanha o worker e limpa sua pasta ao ser descartado.

A restauração adquire acesso exclusivo entre handlers HTTP e recusa execução se houver scan/capas em segundo plano. Valida integridade SQLite, schema reconhecido, referências e dados antes de usar a API transacional de backup do SQLite sobre a conexão existente. Pasta da biblioteca e consentimento de rede da instalação são preservados. As pastas anteriores de assets ficam guardadas até o commit e são repostas em caso de falha. Não há transação única entre SQLite e filesystem nem recuperação automática conjunta após queda do processo.

Downloads públicos de capas/OPDS validam DNS e cada redirecionamento, fixam IPs na conexão e desativam proxy de ambiente. Os limites de corpo são aplicados durante a transferência. `PendingFile` remove arquivos criados pela operação se ela falhar/cancelar; falhas de limpeza são registradas nos logs. Provedores manuais mantêm suporte deliberado a endpoints locais.

## Busca e coleções

`CatalogQuery` concentra filtros opcionais. O SQL é montado com valores parametrizados; somente a expressão `ORDER BY` vem de uma lista fixa. Facetas retornam os valores existentes e coleções com contagem. A associação em `collection_books` usa chaves estrangeiras com `ON DELETE CASCADE`, portanto excluir uma coleção nunca exclui a entrada ou o arquivo do livro.

## Implantação

O `Dockerfile` possui duas etapas: compilação Rust e imagem final Debian com certificados e `poppler-utils`. O `compose.yaml` monta `./pdf` em `/pdf` e `./data` em `/data`, expondo somente a porta 20000. Capas, identidade e backups ficam sob `/data`, todos configuráveis por ambiente.

`AUTH_USERNAME` e `AUTH_PASSWORD` ativam HTTP Basic no middleware da API. Saúde e downloads por token ficam fora dessa barreira deliberadamente. Não há contas, sessões ou múltiplos perfis; para instalações fora do computador local, use VPN ou proxy reverso com TLS.

## Robustez de leitores e catálogo

O catálogo retorna páginas de até 200 livros (60 por padrão), total filtrado e deslocamento.
O banco calcula contagem e página sob o mesmo mutex; empates de ordenação usam o ID.
A interface descarta respostas antigas de busca e oferece navegação entre páginas.
O feed OPDS conserva os filtros no link `next`.

Posições de notas, progresso e restauração usam `models::validate_location`.
Rótulos de posição são escapados na interface, inclusive para dados legados.
Hrefs EPUB são decodificados antes de normalizar o caminho interno, recusando
travessia acima da raiz, escapes inválidos e separadores inseguros.
A CSP dos leitores permite CSS inline/local no sandbox e bloqueia scripts e conexões.

Aplicação de metadados prepara uma capa com nome único e proteção `PendingFile`;
um UPDATE grava os campos e a referência juntos. Imagens antigas são conservadas.
O renderizador PDF usa prazo configurável, mata e aguarda o processo no timeout,
e `kill_on_drop` em cancelamento. Configuração de autenticação incompleta é recusada.
