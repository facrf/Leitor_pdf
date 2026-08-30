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
    ├── src/scanner.rs ─── /pdf
    ├── src/readers.rs ─── arquivos EPUB/CBZ/MOBI/texto
    └── src/metadata.rs ── fontes habilitadas pelo usuário
```

O processo Rust serve a interface estática e a API no mesmo endereço. Não há dependência de Node.js, PHP ou serviço externo em produção.

## Persistência

O banco SQLite contém as tabelas abaixo:

| Tabela | Conteúdo |
|---|---|
| `settings` | Pasta da biblioteca e consentimento de rede. |
| `books` | Caminho relativo, formato, metadados, disponibilidade e referência de capa. |
| `reading_progress` | Localização JSON e percentual da última leitura de cada livro. |
| `notes` | Texto da anotação e localização JSON associada. |
| `metadata_providers` | Fontes nativas e fontes compatíveis adicionadas pelo usuário. |
| `shares` | Token, livro, criação, expiração e revogação. |

Os arquivos originais permanecem na pasta da biblioteca. A varredura usa somente caminhos relativos no catálogo e verifica se o arquivo continua dentro da pasta configurada antes de servi-lo.

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

Quando habilitado, somente dois fluxos usam rede:

1. A pesquisa de metadados envia o texto digitado pelo usuário à fonte selecionada.
2. Ao aplicar um resultado com capa, o backend baixa a imagem e passa a servi-la localmente.

O navegador nunca recebe uma URL remota de capa. Hosts locais e endereços IP privados são recusados para o download de capas. O serviço também bloqueia requisições mutáveis que tragam um cabeçalho `Origin` diferente do host atual.

## Implantação

O `Dockerfile` possui duas etapas: compilação Rust e imagem final Debian mínima. O `compose.yaml` monta `./pdf` em `/pdf` e `./data` em `/data`, expondo somente a porta 20000.

Para instalações fora do computador local, use uma VPN privada ou proxy reverso com TLS e autenticação. A aplicação não implementa contas de usuário nesta versão.

