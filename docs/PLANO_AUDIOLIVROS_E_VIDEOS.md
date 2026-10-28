# Plano de Adoção: Audiolivros e Cursos em Vídeo em Abas Separadas

Este documento estabelece o plano arquitetural e as etapas de implementação para expandir a **Estante Livre** além de documentos e e-books tradicionais, integrando **Audiolivros** e **Cursos em Vídeo** em abas independentes na interface, preservando integralmente o funcionamento local (*offline-first*), a privacidade e o alto desempenho do acervo.

---

## 1. Visão Geral e Princípios

### 1.1 Objetivos
- Permitir que o usuário organize, ouça audiolivros e assista a cursos em vídeo na mesma plataforma em que lê seus livros e PDFs.
- Separar o acervo em três abas principais no topo da estante:
  1. 📚 **Livros** (`PDF`, `EPUB`, `MOBI`, `AZW`, `AZW3`, `CBZ`, `TXT`, `MD`, `HTML`, `FB2`)
  2. 🎧 **Audiolivros** (`M4B`, `MP3`, `M4A`, `OGG`, `OPUS`, `FLAC`)
  3. 🎬 **Cursos em Vídeo** (`MP4`, `WEBM`, `MKV`, `M4V`)
- Oferecer reprodutores adequados a cada tipo de mídia, com salvamento contínuo de posição e anotações vinculadas ao tempo de reprodução.

### 1.2 Princípios Invioláveis
1. **Offline-first e Privacidade Total**: Nenhum arquivo ou metadado de mídia é compartilhado externamente; reprodução e índices são 100% locais.
2. **Streaming por Demanda (Range Requests)**: Arquivos de áudio e vídeo de grande porte (100 MB a 10 GB) utilizam requisições `HTTP 206 Partial Content` (já nativas no backend), sem carregar arquivos inteiros na memória RAM.
3. **Varredura sem Degradação de I/O**: A varredura de mídias pesadas aproveita o cálculo de hash parcial (primeiros e últimos 64 KiB), mantendo o tempo de indexação desprezível.
4. **Resiliência do SQLite e IndexedDB**: O progresso é armazenado localmente no IndexedDB antes da sincronização e enviado à fila dedicada do SQLite via JSON flexível em `location`.
5. **Backups Seguros**: Arquivos originais de vídeo e áudio continuam preservados na pasta de biblioteca e nunca são inflados dentro de arquivos `.zip` de backup do banco.

---

## 2. Arquitetura Técnica

```
┌─────────────────────────────────────────────────────────────┐
│                    Interface Web (web/)                     │
│  [ 📚 Livros (210) ]   [ 🎧 Audiolivros (15) ]   [ 🎬 Cursos (8) ] │
├──────────────────────────────┬──────────────────────────────┤
│       Estante / Catálogo     │       Players Dedicados      │
│  - Grid / Lista por tipo     │  - Leitor PDF/EPUB/CBZ       │
│  - Filtros contextuais       │  - Audio Player + MediaSess. │
│  - Busca FTS5 e metadados    │  - Video Player + Aulas      │
├──────────────────────────────┴──────────────────────────────┤
│                   Cliente HTTP + IndexedDB                  │
│       Fila local de progresso (`estante-progress`)          │
└──────────────────────────────┬──────────────────────────────┘
                               │ HTTP /api/books (Range 206)
┌──────────────────────────────▼──────────────────────────────┐
│                     Backend Rust (src/)                     │
│  - `api.rs`: Rotas de streaming com Range Requests e MIME   │
│  - `models.rs`: `media_type` em BookSummary e CatalogQuery  │
│  - `scanner.rs`: Varredura de extensões de áudio e vídeo    │
│  - `db.rs`: Filtro por categoria e facets agregadas         │
└─────────────────────────────────────────────────────────────┘
```

### 2.1 Categorização de Mídia
Cada formato de arquivo mapeia para uma categoria fundamental (`media_type`):
- `book`: `pdf`, `epub`, `mobi`, `azw`, `azw3`, `cbz`, `txt`, `md`, `html`, `htm`, `fb2`
- `audiobook`: `m4b`, `mp3`, `m4a`, `ogg`, `opus`, `flac`
- `video`: `mp4`, `webm`, `mkv`, `m4v`

### 2.2 Estrutura do Progresso e Anotações
A flexibilidade do campo `location` (JSON) na tabela `reading_progress` e `notes` permite:
- **Audiolivro**:
  ```json
  {
    "type": "audio",
    "time": 342.5,
    "duration": 3600.0,
    "track": 1
  }
  ```
- **Vídeo / Aula**:
  ```json
  {
    "type": "video",
    "time": 840.0,
    "duration": 1420.0,
    "lesson_id": 4
  }
  ```
- O valor `percent` (0.0 a 100.0) continua sendo o ponto de sincronização comum para barras de progresso na estante e na mesa de leitura.

---

## 3. Etapas de Implementação

### Etapa 1: Fundação de Mídia, Abas e Streaming Inline
- **Objetivo**: Habilitar a varredura das extensões de áudio e vídeo, permitir streaming inline sem forçar download, incluir o filtro por tipo de mídia na API e apresentar as 3 abas na interface.
- **Entregáveis**:
  1. [`src/scanner.rs`](file:///home/facrf/projetos/Leitor_pdf/src/scanner.rs): inclusão das extensões de áudio e vídeo em `SUPPORTED_EXTENSIONS`.
  2. [`src/api.rs`](file:///home/facrf/projetos/Leitor_pdf/src/api.rs): ampliação de `inline_safe` em `stream_file` para tipos `audio/*` e `video/*`.
  3. [`src/models.rs`](file:///home/facrf/projetos/Leitor_pdf/src/models.rs) e [`src/db.rs`](file:///home/facrf/projetos/Leitor_pdf/src/db.rs): filtro `media_type` em `CatalogQuery` e agregação nas facetas do catálogo.
  4. [`web/index.html`](file:///home/facrf/projetos/Leitor_pdf/web/index.html) e [`web/styles.css`](file:///home/facrf/projetos/Leitor_pdf/web/styles.css): seletor visual de abas (Livros, Audiolivros, Cursos em Vídeo) com contadores dinâmicos.
  5. [`web/app.js`](file:///home/facrf/projetos/Leitor_pdf/web/app.js): integração da aba ativa com as chamadas de API e reprodução básica via `<audio>` e `<video>` no estágio do leitor.
  6. Testes unitários e de integração validando os contratos de catálogo e streaming.

### Etapa 2: Player Avançado de Audiolivros e MediaSession
- **Objetivo**: Proporcionar uma experiência ergonômica de audição comparável a aplicativos dedicados de audiolivros.
- **Entregáveis**:
  1. Reprodutor de áudio com interface elegante: barra de progresso (scrubber), tempo atual / restante, botões de retroceder 15s e avançar 30s.
  2. Seletor de velocidade de reprodução rápida (0.75x, 1x, 1.25x, 1.5x, 2x).
  3. Integração com `navigator.mediaSession`: exibição de capa, título e autor nos controles da tela de bloqueio do celular ou atalhos do sistema operacional.
  4. Extração de tags ID3/MP4 (autor, narrador, duração e capa embutida).

### Etapa 3: Player de Cursos em Vídeo e Navegação por Aulas
- **Objetivo**: Suporte completo a cursos estruturados em múltiplos vídeos e módulos.
- **Entregáveis**:
  1. Player com layout "Modo Curso / Teatro": vídeo principal amplo e painel retrátil de aulas.
  2. Agrupamento inteligente por pasta: identificar diretórios de cursos para agrupar múltiplos arquivos `.mp4` em uma única coleção com lista de capítulos/aulas.
  3. Checklist visual de aulas assistidas e progresso consolidado do curso.
  4. Atalhos de teclado ergonômicos (Espaço para play/pause, setas direcionais para busca no vídeo, F para tela cheia).

### Etapa 4: Metadados Ricos, Thumbnails e Recursos Complementares
- **Objetivo**: Polimento visual, facilidades operacionais e diagnóstico da estante.
- **Entregáveis**:
  1. Geração de thumbnails automáticos para vídeos via `ffmpeg` (processo em baixa prioridade com limite de tempo, similar ao `pdftoppm`).
  2. Suporte a legendas locais em formato WebVTT (`.vtt`) ou SubRip (`.srt`).
  3. Atualização do painel de diagnóstico e saúde da biblioteca com contadores específicos de mídias e formatos.

---

## 4. Matriz de Compatibilidade de Formatos Web

| Formato | Tipo | Compatibilidade Nativa Navegador | Estratégia de Suporte |
| :--- | :--- | :--- | :--- |
| **MP3** | Áudio | 100% (Chrome, Firefox, Safari, Edge) | Reprodução nativa direta |
| **M4A / M4B** | Áudio | 100% (AAC) | Reprodução nativa direta com capítulos |
| **OGG / OPUS** | Áudio | 100% | Reprodução nativa direta |
| **FLAC** | Áudio | 98% (Navegadores modernos) | Reprodução nativa direta |
| **MP4 (H.264+AAC)** | Vídeo | 100% | Reprodução nativa direta com Range 206 |
| **WebM (VP8/VP9)** | Vídeo | 100% | Reprodução nativa direta |
| **MKV** | Vídeo | Parcial (depende dos codecs internos) | Suporte direto para streams H.264/WebM compatíveis |

---

## 5. Próximos Passos Imediatos
1. Submeter e sincronizar o plano de documentação com o repositório remoto (`git push`).
2. Executar a **Etapa 1**, disponibilizando o suporte básico a áudio e vídeo com a interface de abas na estante.
