# Checkpoint — 2026-10-28

## Revisão atual — Conclusão das Etapas 3 e 4 (Player de Cursos em Vídeo, Aulas por Pasta, Thumbnails via FFmpeg e Legendas)

Implementação e verificação completa das **Etapas 3 e 4** do plano de adoção (`docs/PLANO_AUDIOLIVROS_E_VIDEOS.md`):
- **Etapa 3**: Player com layout "Modo Curso / Teatro", detecção e agrupamento inteligente de aulas e módulos pelo diretório pai (`course_info`), ordenação natural dos arquivos (`natural_key`), painel retrátil de aulas com checklist visual de conclusão (`percent >= 90.0`), progresso consolidado do curso, criação e amarração de coleção em lote com 1 clique, alternância suave entre aulas e atalhos ergonômicos de teclado (F para tela cheia, M para mudo, Espaço para play/pause, setas para busca).
- **Etapa 4**: Extração de thumbnails para vídeos (`mp4`, `webm`, `mkv`, `m4v`) via `ffmpeg` em baixa prioridade (`nice -n 15`) com escalonamento proporcional e fallback para o primeiro quadro (`00:00:00`), suporte completo a legendas locais em `.vtt` e `.srt` (com conversão on-the-fly para WebVTT e detecção de idiomas pt, en, es, fr, de), integração no `<video>` nativo do navegador via tags `<track>`, atualização do `Dockerfile` incluindo `ffmpeg`, e expansão do painel de diagnóstico de saúde (`/maintenance/health`) com cartões detalhados por tipo de mídia (`E-books e documentos`, `Audiolivros`, `Vídeos e cursos`).

### Mudanças realizadas:

1. **Backend: Agrupamento de Aulas de Cursos e Metadados (`src/models.rs`, `src/db.rs`, `src/readers.rs`, `tests/rust/catalog-regression.rs`)**:
   - `src/models.rs`: Adicionadas estruturas `CourseLesson` e `CourseInfo`. `LibraryHealth` enriquecida com `media_counts: Option<MediaCounts>`.
   - `src/readers.rs`: Função de ordenação alfanumérica natural `natural_key` exposta como `pub(crate)` para reutilização no banco e nas legendas.
   - `src/db.rs`:
     - Implementado método `course_info(book_id)`: identifica a pasta pai, busca todas as aulas do mesmo módulo, aplica ordenação natural (`01`, `02`, `10`), calcula percentual consolidado e status de conclusão (`completed`).
     - Atualizada query `missing_pdf_covers` para incluir formatos de vídeo (`mp4`, `webm`, `mkv`, `m4v`).
     - Atualizado `library_health` para calcular contadores de livros, audiolivros e vídeos.
   - `tests/rust/catalog-regression.rs`: Adicionado teste de regressão `course_info_groups_lessons_and_calculates_progress`.

2. **Backend: Thumbnails de Vídeo via FFmpeg (`src/covers.rs`, `Dockerfile`)**:
   - `src/covers.rs`: Implementada extração de thumbnails com `nice -n 15 ffmpeg -ss 00:00:01 -i <source> -frames:v 1 -update 1 -vf scale=min(900\,iw):-1 -q:v 2` com fallback para `-ss 00:00:00` e limpeza via `PendingFile`.
   - `Dockerfile`: Adicionado pacote `ffmpeg` às dependências da imagem de produção Debian Bookworm.

3. **Backend: Detecção e Conversão de Legendas (`src/subtitles.rs`, `src/api.rs`, `src/main.rs`, `tests/rust/subtitles.rs`)**:
   - Criado módulo `src/subtitles.rs` com `detect_subtitles` (varre arquivos `.vtt` e `.srt` do mesmo vídeo), `load_subtitle_vtt` (leitura limitada a 5 MB e proteção contra path traversal) e `srt_to_vtt` (conversão sob demanda de timestamps e índices SubRip para WebVTT).
   - `src/api.rs`: Rotas `GET /api/books/{id}/course`, `GET /api/books/{id}/subtitles` e `GET /api/books/{id}/subtitles/{filename}`.
   - Teste unitário completo em `tests/rust/subtitles.rs`.

4. **Interface Web: Modo Curso / Teatro e Diagnóstico (`web/app.js`, `web/styles.css`)**:
   - `web/styles.css`: Estilização do palco de teatro (`.video-theater-layout`, `.video-stage-main`), estilos de legendas (`::cue`), sidebar retrátil de aulas (`.course-sidebar`), barra de progresso do curso (`.course-progress-bar`), botão de coleção (`.course-create-collection-btn`) e itens de aula interativos com checklist (`.lesson-check-btn`).
   - `web/app.js`:
     - `renderVideoPlayer`: Detecção automática de curso e legendas; injeção de `<track>` no elemento `<video>`; carregamento da barra lateral de módulos; alternância rápida de aula via `switchToLesson`; marcação interativa de conclusão (salvando 100% de progresso); conclusão automática ao atingir 95% ou fim do vídeo; botão para criar coleção do curso em lote com um clique.
     - Atalhos de teclado: `F` (tela cheia), `M` (mutar áudio), `Espaço` (play/pause), `ArrowLeft`/`ArrowRight` (buscar 10s).
     - `renderHealth`: Exibição de contadores específicos de e-books, audiolivros e vídeos nos cartões de saúde da biblioteca.

5. **Testes e Documentação (`tests/api-e2e.mjs`, `tests/start-browser-server.mjs`, `README.md`, `docs/api.md`)**:
   - `tests/api-e2e.mjs`: Testes E2E para `/books/{id}/course`, `/books/{id}/subtitles` e `health.media_counts`.
   - `tests/start-browser-server.mjs`: Propagação do `CARGO_TARGET_DIR=target_local`.
   - `README.md` e `docs/api.md`: Documentação das 4 etapas e das novas rotas de curso e legenda.

### Verificações realmente executadas:
- `CARGO_TARGET_DIR=target_local cargo clippy --locked --all-targets -- -D warnings`: 0 warnings, 0 erros.
- `CARGO_TARGET_DIR=target_local cargo test --locked`: 52 testes passaram (0 falhas, 1 teste de volume ignorado).
- `npm run test:ui`: aprovado (contrato visual, IDs únicos e gravação de progresso).
- `npm run test:api`: aprovado (todas as rotas E2E).
- `npm run test:auth`: aprovado (saúde pública e proteção HTTP Basic).
- `bash tests/with-local-tools.sh npm run test:browser`: aprovado (Playwright browser test completo).

### Próximo passo:
- Realizar commit e push das Etapas 3 e 4 no repositório remoto (`origin/main`).

---

# Checkpoint — 2026-10-28

## Revisão anterior — Conclusão da Etapa 2 (Metadados de Áudio, Capas Embutidas, Temporizador de Sono e MediaSession)

Implementação e verificação completa da **Etapa 2**: parser zero-dependency de metadados ID3v2, ID3v1 e MP4/M4B atoms com extração de capas embutidas (`APIC` e `covr`), integração automática na geração de capas e varredura de audiolivros, player aprimorado com Temporizador de Sono (Sleep Timer) com contagem regressiva, integração profunda com `navigator.mediaSession` (metadados, capa, controles da tela de bloqueio e atalhos de seek) e atalhos de teclado dedicados no reprodutor.

### Mudanças realizadas:

1. **Backend: Parsers de Metadados e Capas Embutidas (`src/audio_metadata.rs`, `tests/rust/audio-metadata.rs`)**:
   - Implementado parser bounded seguro para tags ID3v2.3 / ID3v2.4 (`TIT2` título, `TPE1` autor/artista, frame `APIC` para capa embutida JPEG/PNG).
   - Implementado fallback ID3v1 nos últimos 128 bytes do arquivo (`TAG`).
   - Implementado parser para átomos MP4/M4B (`moov.udta.meta.ilst` para `©nam`, `©ART` e `covr`).
   - Bounded I/O: limites rigorosos de leitura (cabeçalhos de até 10 MB para átomos e ID3v2; buffer de capa de até 10 MB), garantindo zero estouro de memória e tratamento resiliente de arquivos truncados.
   - Teste de regressão completo: `tests/rust/audio-metadata.rs` cobrindo ID3v2, ID3v1 e átomos M4B com imagem JPEG sintética.

2. **Backend: Integração no Scanner e Gerador de Capas (`src/scanner.rs`, `src/covers.rs`, `src/db.rs`)**:
   - `src/scanner.rs`: na varredura de `mp3`, `m4a` e `m4b`, extrai título e autor reais embutidos nas tags caso disponíveis, caindo graciosamente para o nome do arquivo se ausentes.
   - `src/covers.rs`: na rota/tarefa de geração de capa (`generate_one`), arquivos `mp3`, `m4a` e `m4b` têm suas capas embutidas extraídas e salvas na pasta de capas como JPEG/PNG, dispensando binários externos para capas de audiolivros.
   - `src/db.rs`: `missing_pdf_covers` agora inclui extensões `mp3`, `m4a` e `m4b` como elegíveis para geração de capa.

3. **Interface Web: Temporizador de Sono, MediaSession e Atalhos (`web/app.js`, `web/styles.css`)**:
   - `web/app.js`:
     - Temporizador de Sono (Sleep Timer) com opções desligado, 15m, 30m, 45m e 60m. Atualização a cada segundo com contagem regressiva visual (`#audio-sleep-countdown`) e pausa suave do áudio ao zerar.
     - Suporte à API `navigator.mediaSession`: publica `MediaMetadata` (título, autor, capa absoluta) e registra handlers para `play`, `pause`, `seekbackward` (15s), `seekforward` (30s), `seekto` e `stop`.
     - Sincronização de estado `setPositionState` com duração e tempo corrente.
     - Atalhos de teclado no estágio de mídia: Barra de Espaço (play/pause), Seta Esquerda (retroceder) e Seta Direita (avançar).
     - Limpeza completa de timers e handlers do MediaSession ao fechar o leitor.
   - `web/styles.css`: Estilização do badge de contagem regressiva do sleep timer (`.sleep-countdown`).

### Verificações realmente executadas:
- `CARGO_TARGET_DIR=target_local cargo clippy --locked --all-targets -- -D warnings`: 0 warnings.
- `CARGO_TARGET_DIR=target_local cargo test --locked`: 47 passaram, 0 falharam, 1 teste de volume ignorado.
- `npm run test:ui`: aprovado (contrato visual, IDs únicos e gravação de progresso).
- `npm run test:api`: aprovado (todas as rotas E2E).
- `npm run test:auth`: aprovado (saúde pública e proteção HTTP Basic).

### Próximo passo:
- Implementar a **Etapa 3**: Player de Cursos em Vídeo, navegação por aulas/módulos baseada na pasta do curso e acompanhamento de aulas concluídas.

---

# Checkpoint — 2026-10-28

## Revisão anterior — Plano de Adoção e Conclusão da Etapa 1 (Audiolivros e Vídeos em Abas Separadas)

Elaboração do plano em etapas (`docs/PLANO_AUDIOLIVROS_E_VIDEOS.md`), commit & push para o repositório remoto, e implementação completa da **Etapa 1**: fundação de mídias, extensão do scanner, streaming inline com Range Requests, filtro de categoria `media_type`, contadores de mídia nas facetas, navegação por abas na interface e reprodutores dedicados para áudio e vídeo com sincronização de progresso.

### Mudanças realizadas:

1. **Documentação Arquitetural e Plano em Etapas (`docs/PLANO_AUDIOLIVROS_E_VIDEOS.md`, `README.md`)**:
   - Definição do escopo das 4 etapas de implementação (Etapa 1: Fundação, Abas e Streaming Inline; Etapa 2: Player Especializado de Audiolivro e MediaSession; Etapa 3: Player de Cursos em Vídeo e Aulas por Módulos; Etapa 4: Thumbnails e Legendas).
   - Matriz de compatibilidade de formatos de áudio (`mp3`, `m4a`, `m4b`, `ogg`, `opus`, `flac`) e vídeo (`mp4`, `webm`, `mkv`, `m4v`).
   - Sincronização documentada e enviada via `git push origin main`.

2. **Backend: Scanner e Suporte a Extensões Multimídia (`src/scanner.rs`)**:
   - Ampliado `SUPPORTED_EXTENSIONS` para incluir extensões de áudio (`m4b`, `mp3`, `m4a`, `ogg`, `opus`, `flac`) e vídeo (`mp4`, `webm`, `mkv`, `m4v`).
   - Criada função `media_type_for_format(format: &str) -> &'static str` para classificar formatos em `book`, `audiobook` e `video`.

3. **Backend: Modelos e Validação (`src/models.rs`, `tests/rust/location-validation.rs`)**:
   - Adicionado campo `media_type: String` em `BookSummary` e `Book`.
   - Adicionado `media_type: Option<String>` em `CatalogQuery`.
   - Adicionada estrutura `MediaCounts { books, audiobooks, videos }` em `CatalogFacets`.
   - Ampliada `validate_location` para validar posições com `time` e `duration` não-negativos e finitos para reprodução multimídia.

4. **Backend: Streaming Inline e Banco de Dados (`src/api.rs`, `src/db.rs`, `tests/rust/catalog-regression.rs`)**:
   - `stream_file`: ampliada a lista de MIME types em `inline_safe` para permitir reprodução nativa de áudio (`audio/*`) e vídeo (`video/*`) no navegador com `Content-Disposition: inline` e Range 206.
   - `catalog_page`: adicionado filtro por `media_type` em SQL com amarração de parâmetros e preservação de paginação e desempate estável.
   - `catalog_facets`: cálculo agregado de `media_counts` para alimentar os contadores das abas.
   - Teste de regressão adicionado: `media_types_filter_and_count_correctly`.

5. **Interface Web: Abas, Badges e Reprodutores (`web/index.html`, `web/styles.css`, `web/app.js`)**:
   - `web/index.html`: adicionado componente `<nav class="media-tabs">` com abas para Livros (`#tab-books`), Audiolivros (`#tab-audiobooks`) e Cursos em Vídeo (`#tab-videos`), além de badges para contagens (`#badge-books`, `#badge-audiobooks`, `#badge-videos`). Input de upload atualizado com novos formatos.
   - `web/styles.css`: estilização das abas responsivas (`.media-tabs`, `.media-tab-btn`, `.tab-badge`) e layouts de reprodução de áudio (`.audio-player-view`) e vídeo (`.video-player-view`).
   - `web/app.js`:
     - Gerenciamento de abas ativas e filtro no catálogo (`state.filters.media_type`).
     - Atualização dinâmica dos contadores das abas via facetas.
     - Reprodutores `<audio>` e `<video>` dedicados com controle de velocidade (0.75x a 2x), botões de pular/retroceder tempo (10s/15s/30s), persistência contínua de progresso (`time`, `duration`, `percent`) e pausa automática ao fechar o leitor.
     - Navegação de anotações com suporte a timecodes (`jumpToLocation`).

### Verificações realmente executadas:
- `CARGO_TARGET_DIR=target_local cargo clippy --locked --all-targets -- -D warnings`: 0 warnings.
- `CARGO_TARGET_DIR=target_local cargo test --locked`: 44 passaram, 0 falharam.
- `npm run test:ui`: aprovado (contrato visual, IDs únicos e gravação de progresso).
- `npm run test:api`: aprovado (todas as rotas E2E).
- `npm run test:auth`: aprovado (saúde pública e proteção HTTP Basic).

### Próximo passo:
- Submeter as alterações da Etapa 1 ao repositório remoto (`git push`) e, na sequência, avançar para a Etapa 2 (Player de Audiolivro com MediaSession API e extração de tags ID3/MP4).

---

# Checkpoint — 2026-10-27

## Revisão atual — Auditoria de Código, Resiliência Operacional e Melhorias de UX

Auditoria completa da base de código em busca de melhorias de desempenho, segurança e experiência do usuário, cobrindo o backend Rust, persistência SQLite e a interface web.

### Mudanças realizadas:

1. **Robustez de Processos e Timeout de OCR (`src/readers.rs`, `compose.yaml`, `.env.example`)**:
   - `extract_pdf_ocr_sample`: o parâmetro `timeout_secs` agora é aplicado na invocação de subprocessos (`pdftoppm` e `tesseract`) usando o utilitário de sistema `timeout`. Isso previne que arquivos malformados ou corrompidos travem os processos de OCR indefinidamente.
   - Adicionada variável `OCR_TIMEOUT_SECONDS: ${OCR_TIMEOUT_SECONDS:-15}` em `compose.yaml` e documentada em `.env.example`.

2. **Offloading de Operações Bloqueantes no Executor Tokio (`src/api.rs`)**:
   - `reindex_fts_content`: extração textual (`extract_text_sample_with_options`), que envolve parse de PDF, descompressão EPUB/MOBI e OCR, agora roda via `tokio::task::spawn_blocking`, impedindo a degradação de latência do executor assíncrono.

3. **Integridade de Dados e Consultas SQLite (`src/db.rs`, `src/api.rs`)**:
   - `list_all_notes`: filtro por cor alterado para `json_extract(n.location, '$.color') = ?`, garantindo correspondência exata no AST JSON, imune a variações de espaçamento ou formatação de strings.
   - `mark_available`: atualização de `updated_at` com o timestamp corrente na restauração de arquivos, sincronizando ordenações recentes e invalidações de cache do cliente.
   - `export_all_notes`: rota de exportação define limite de até 50.000 anotações para exportar cadernos inteiros sem truncamento prematuro no valor padrão de 100 notas da listagem paginada.

4. **Recuperação de Arquivos da Lixeira (`src/maintenance.rs`, `tests/rust/maintenance.rs`)**:
   - `restore_trash`: recria com segurança pastas pai intermediárias (`create_dir_all`) caso tenham sido limpas ou excluídas durante o período em que o livro esteve na lixeira.
   - Teste de regressão adicionado: `trash_restores_when_parent_subfolder_was_removed`.

5. **Ergonomia e Prevenção de Erros na Interface (`web/app.js`)**:
   - Prevenção de duplicatas: o botão de salvar anotação é desabilitado durante o envio à API.
   - Proteção de dados: adicionada confirmação (`confirm`) antes de excluir anotações do caderno.
   - Navegação por teclado no leitor: suporte a setas direcionais (`ArrowLeft` / `ArrowRight`) para avançar e voltar páginas ou capítulos quando o leitor está aberto e sem foco em campos de formulário.
   - Resiliência no leitor CBZ: navegação por anotações com suporte tanto a `page_index` quanto a `page`.

6. **Docker e Publicação no GHCR (`.github/workflows/publish-image.yml`, `compose.yaml`, `Dockerfile`)**:
   - Construída e disponibilizada a imagem Docker localmente com a tag `ghcr.io/facrf/estante-livre:latest`.
   - `compose.yaml`: adicionado bloco `build: { context: ., dockerfile: Dockerfile }`, permitindo que `docker compose up -d` ou `docker compose build` construa a imagem localmente sem falhar caso o pull remoto não encontre a tag no GHCR.
   - `Dockerfile`: adicionada variável padrão `OCR_TIMEOUT_SECONDS=15`.
   - `.github/workflows/publish-image.yml`: corrigidas as referências de versões das GitHub Actions (`actions/checkout@v4`, `actions/setup-node@v4`, `actions/upload-artifact@v4`), que usavam versões inexistentes `@v6` e falhavam na inicialização do runner. Ajustadas as plataformas multi-arch para `linux/amd64,linux/arm64`.

### Verificações realmente executadas:
- `cargo clippy --locked --all-targets -- -D warnings`: 0 warnings.
- `cargo test --locked`: 43 passaram, 0 falharam, 1 teste de volume ignorado.
- `npm run test:ui`: aprovado (contrato visual, IDs únicos e gravação de progresso).
- `npm run test:api`: aprovado (todas as rotas E2E).
- `npm run test:auth`: aprovado (saúde pública e proteção HTTP Basic).
- `npm run test:browser`: aprovado (Playwright E2E browser test completo).

### Limitações:
- O timeout em OCR utiliza o binário `timeout` do coreutils quando disponível no ambiente Linux / container Docker; caso ausente, prossegue sem o invólucro de limite de tempo.

### Próximo passo:
- Apresentar o relatório de melhorias implementadas e verificadas ao usuário.

---

# Checkpoint — 2026-10-23

## Revisão atual — Destaques com Marca-texto, Gestão/Exportação de Anotações e Suporte a OCR

Implementação de destaques com cores de marca-texto no leitor, gestão e exportação de anotações (Markdown/JSON) e suporte a OCR via Tesseract para PDFs escaneados.

### Mudanças realizadas:

1. **Modelos e Validação (`src/models.rs`)**:
   - `validate_location`: validação de `color` (string até 32 caracteres) e `quote` / `selected_text` (string até 4096 caracteres), preservando total compatibilidade com posições legadas e esquemas de restauração.
   - Adicionada estrutura `NoteWithBook` para retorno de notas enriquecidas com título e autor do livro.
   - Adicionadas estruturas `NotesQuery` (busca por texto, filtro por cor e paginação) e `ExportNotesQuery` (formato `markdown` ou `json`).
   - Teste unitário em `tests/rust/location-validation.rs` cobrindo aceitação e rejeição de cores e citações inválidas.

2. **Configuração Operacional (`src/config.rs`)**:
   - Adicionados `ocr_enabled: bool` (padrão `true`, configurável via `OCR_ENABLED`) e `ocr_timeout_seconds: u64` (padrão `15`, delimitado entre 1 e 600 segundos).
   - Atualizados `compose.yaml`, `.env.example` e `Dockerfile`.

3. **OCR para PDFs Escaneados (`src/readers.rs`)**:
   - Implementadas `is_tesseract_available()`, `extract_pdf_ocr_sample()` e `extract_text_sample_with_options()`.
   - Fallback gracioso: quando `lopdf` não encontra camada textual em PDFs (escaneados ou imagens), o sistema verifica a presença do executável `tesseract`; se disponível, rasteriza até 2 páginas temporárias com `pdftoppm` e extrai o texto com OCR sem travar o processo.
   - Sem dependência obrigatória no host: caso `tesseract` não esteja instalado, a função retorna `None` silenciosamente sem panics ou erros.
   - Atualizado `Dockerfile` para incluir `tesseract-ocr`, `tesseract-ocr-por` e `tesseract-ocr-eng` no container Debian final.

4. **Banco de Dados SQLite (`src/db.rs`)**:
   - Implementado método assíncrono `list_all_notes(query: &NotesQuery) -> AppResult<Vec<NoteWithBook>>` com busca parcial em conteúdo e citações, filtro por cor e paginação.

5. **API HTTP e Exportação (`src/api.rs`)**:
   - Rota `GET /api/notes`: listagem geral ou filtrada de anotações de todos os livros ou de um livro específico.
   - Rota `GET /api/notes/export?format=markdown|json`: exportação de todas as anotações do acervo com cabeçalho `Content-Disposition` RFC 6266.
   - Rota `GET /api/books/{id}/notes/export?format=markdown|json`: exportação das notas e destaques de um livro específico.
   - Rota `GET /api/settings`: informa `ocr_available` e `ocr_enabled`.
   - `reindex_fts_content`: passa os parâmetros de OCR configurados para a extração do FTS5.

6. **Interface Web (`web/index.html`, `web/styles.css`, `web/app.js`)**:
   - `web/index.html`:
     - Cabeçalho do caderno com botões de exportação direta em Markdown (`.md`) e JSON (`.json`).
     - Barra de ferramentas com busca rápida em tempo real e botões de filtro por cor (Todas, Amarelo, Verde, Azul).
     - Formulário de anotações com campo de citação/trecho destacado (`#note-quote`) e seletores de cor de marca-texto.
   - `web/styles.css`:
     - Estilos visuais para citações destacadas (`.note-quote`) com cores pastel suaves (amarelo, verde, azul).
     - Marcadores de cor (`.swatch`), filtros clicáveis e links de localização navegáveis.
   - `web/app.js`:
     - `loadNotes()` e `renderNotesList()` com suporte a filtro instantâneo por texto e cor.
     - `jumpToLocation()`: clicar na posição de uma nota navega diretamente para a página (PDF/CBZ) ou capítulo (EPUB) correspondente.
     - Botão de exportação de notas integrado também aos detalhes do livro (`#book-detail`).

7. **Testes e Documentação**:
   - `tests/rust/location-validation.rs`: testes para campos de destaque e cor.
   - `tests/api-e2e.mjs`: testes de ponta a ponta para criação de notas com destaques, listagem com filtros e exportação nos formatos Markdown e JSON.
   - `tests/browser-e2e.spec.mjs`: teste com Playwright exercitando criação de nota com citação, marca-texto amarelo e filtro no leitor.
   - `README.md` e `docs/api.md` atualizados com as novas rotas e recursos.

### Verificações realmente executadas:
- `cargo clippy --locked --all-targets -- -D warnings`: passou sem nenhum aviso (0 warnings).
- `cargo test --locked`: 42 passaram, 0 falharam, 1 teste de volume ignorado.
- `npm run test:ui`: aprovado (contrato visual, IDs únicos e gravação de progresso).
- `npm run test:api`: aprovado (todas as rotas E2E incluindo novas rotas de notas e exportação).
- `npm run test:auth`: aprovado (saúde pública e proteção HTTP Basic).
- `npm run test:browser`: aprovado (Playwright E2E browser test completo incluindo notas e destaques).

### Limitações:
- O OCR processa até 2 páginas por PDF para manter a indexação ágil e não sobrecarregar CPU/disco.
- Destaques em PDF baseiam-se na página e na citação anotada (o visualizador PDF nativo do navegador controla a camada gráfica do documento).

### Próximo passo:
- Apresentar o resultado ao usuário e verificar se deseja commit/push.

---

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
