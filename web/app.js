const state = {
  catalog: { total: 0, limit: 60, offset: 0, requestId: 0 },
  books: [], facets: { formats: [], authors: [], subjects: [] }, desk: [], suggestions: [],
  settings: null, providers: [], collections: [], bookCollectionIds: [], currentBook: null,
  filters: { q: '', format: '', media_type: 'book', author: '', subject: '', publisher: '', language: '', year: '', min_size: '', max_size: '', progress: '', availability: '', collection_id: '', sort: '', content_only: '' },
  reader: { kind: null, location: {}, percent: 0, items: [] },
  searchTimer: null, scanTimer: null, scanObservedRunning: false,
  carouselIndex: 0, carouselTimer: null,
  progressSaveTimer: null, progressSaveChain: Promise.resolve(),
  sleepTimer: null, sleepTimerEndsAt: 0,
};

const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];

async function api(path, options = {}) {
  const init = { ...options, headers: { ...(options.headers || {}) } };
  if (options.body && !(options.body instanceof FormData)) init.headers['Content-Type'] = 'application/json';
  const response = await fetch(`/api${path}`, init);
  if (!response.ok) {
    let message = `Erro ${response.status}`;
    try { message = (await response.json()).error || message; } catch (_) { /* resposta sem JSON */ }
    throw new Error(message);
  }
  return response.status === 204 ? null : response.json();
}

function escapeHtml(value = '') {
  return String(value).replace(/[&<>'"]/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' }[character]));
}

function formatBytes(bytes) {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** index).toFixed(index ? 1 : 0)} ${units[index]}`;
}

function toast(message, error = false) {
  const element = document.createElement('div');
  element.className = `toast${error ? ' error' : ''}`;
  element.textContent = message;
  $('#toast-stack').append(element);
  setTimeout(() => element.remove(), 4200);
}

function coverMarkup(book, detail = false) {
  const hue = (book.id * 47) % 360;
  const style = `--cover-a:hsl(${hue} 30% 35%);--cover-b:hsl(${(hue + 40) % 360} 35% 19%)`;
  const image = book.has_cover
    ? `<img src="/api/books/${book.id}/cover?v=${encodeURIComponent(book.updated_at || '')}" alt="Capa de ${escapeHtml(book.title)}">`
    : `<div class="generated-cover" style="${style}"><span>${escapeHtml(book.title)}</span><small>${escapeHtml(book.author || 'Biblioteca local')}</small></div>`;
  return `<div class="book-cover ${book.is_available === false ? 'unavailable' : ''}">${image}${detail ? '' : `<b class="format-badge">${escapeHtml(book.format)}</b>`}</div>`;
}

async function loadBooks(query = state.filters.q, offset = 0) {
  const requestId = ++state.catalog.requestId;
  state.filters.q = query || '';
  try {
    const params = new URLSearchParams();
    Object.entries(state.filters).forEach(([key, value]) => { if (value) params.set(key === 'q' ? 'q' : key, value); });
    params.set('limit', state.catalog.limit);
    params.set('offset', offset);
    const data = await api(`/books?${params}`);
    if (requestId !== state.catalog.requestId) return;
    // Exclusoes/filtros podem tornar a pagina atual vazia.
    if (!data.books.length && data.total && offset > 0) {
      return loadBooks(query, Math.floor((data.total - 1) / data.limit) * data.limit);
    }
    Object.assign(state.catalog, { total: data.total, limit: data.limit, offset: data.offset });
    state.books = data.books;
    state.facets = data.facets;
    renderFilters();
    renderBooks();
  } catch (error) { if (requestId === state.catalog.requestId) toast(error.message, true); }
}

function renderBooks() {
  const grid = $('#book-grid');
  const { total, limit, offset } = state.catalog;
  $('#library-summary').textContent = `${total} ${total === 1 ? 'livro encontrado' : 'livros encontrados'}`;
  $('#catalog-pagination').classList.toggle('hidden', total <= limit);
  $('#catalog-page-label').textContent = total ? `${offset + 1}–${offset + state.books.length} de ${total}` : '0 livros';
  $('#catalog-previous').disabled = offset === 0;
  $('#catalog-next').disabled = offset + limit >= total;
  $('#empty-state').classList.toggle('hidden', state.books.length !== 0);
  grid.classList.toggle('hidden', state.books.length === 0);
  const filtered = Object.values(state.filters).some(Boolean);
  $('#empty-message').textContent = filtered ? 'Nenhum livro corresponde aos filtros escolhidos.' : 'Adicione um livro ou faça uma varredura da pasta configurada.';
  grid.innerHTML = state.books.map(book => {
    const isAudio = book.media_type === 'audiobook' || ['m4b','mp3','m4a','ogg','opus','flac'].includes(book.format);
    const isVideo = book.media_type === 'video' || ['mp4','webm','mkv','m4v'].includes(book.format);
    const progressLabel = !book.progress_percent
      ? 'Ainda não iniciado'
      : isAudio
      ? `${Math.round(book.progress_percent)}% ouvido`
      : isVideo
      ? `${Math.round(book.progress_percent)}% assistido`
      : `${Math.round(book.progress_percent)}% lido`;
    return `
    <article class="book-card" data-book-id="${book.id}" tabindex="0" aria-label="Abrir ${escapeHtml(book.title)}">
      ${coverMarkup(book)}
      <div class="book-info"><h3>${escapeHtml(book.title)}</h3><p>${escapeHtml(book.author || book.filename)}</p>
        ${book.subjects?.length ? `<small class="subject-label">${escapeHtml(book.subjects.slice(0, 2).join(' · '))}</small>` : ''}
        <div class="progress-track"><i style="width:${Math.max(0, Math.min(100, book.progress_percent))}%"></i></div>
        <small class="progress-label">${progressLabel}</small>
      </div>
    </article>`;
  }).join('');
}

function renderFilters() {
  const configurations = [
    ['#filter-format', state.facets.formats, 'Todos os formatos', 'format'],
    ['#filter-author', state.facets.authors, 'Todos os autores', 'author'],
    ['#filter-subject', state.facets.subjects, 'Todos os assuntos', 'subject'],
    ['#filter-publisher', state.facets.publishers || [], 'Todas', 'publisher'],
    ['#filter-language', state.facets.languages || [], 'Todos', 'language'],
    ['#filter-year', state.facets.years || [], 'Todos', 'year'],
  ];
  configurations.forEach(([selector, values, label, key]) => {
    const select = $(selector);
    select.innerHTML = `<option value="">${label}</option>${values.map(value => `<option value="${escapeHtml(value)}">${escapeHtml(value)}</option>`).join('')}`;
    select.value = state.filters[key];
  });
  const collectionSelect = $('#filter-collection');
  collectionSelect.innerHTML = '<option value="">Todas as coleções</option>' + (state.facets.collections || []).map(collection => `<option value="${collection.id}">${escapeHtml(collection.name)} (${collection.book_count})</option>`).join('');
  collectionSelect.value = state.filters.collection_id;
  $('#filter-progress').value = state.filters.progress;
  $('#filter-availability').value = state.filters.availability;
  $('#filter-min-size').value = state.filters.min_size ? Math.round(Number(state.filters.min_size) / 1024 / 1024) : '';
  $('#filter-max-size').value = state.filters.max_size ? Math.round(Number(state.filters.max_size) / 1024 / 1024) : '';
  $('#filter-sort').value = state.filters.sort || 'title';
  $('#filter-search-mode').value = state.filters.content_only ? 'content' : '';
  if (state.facets?.media_counts) {
    const mc = state.facets.media_counts;
    const badgeBooks = $('#badge-books');
    const badgeAudio = $('#badge-audiobooks');
    const badgeVideo = $('#badge-videos');
    if (badgeBooks) badgeBooks.textContent = mc.books || 0;
    if (badgeAudio) badgeAudio.textContent = mc.audiobooks || 0;
    if (badgeVideo) badgeVideo.textContent = mc.videos || 0;
  }
  $$('.media-tab-btn').forEach(btn => {
    const active = btn.dataset.mediaType === (state.filters.media_type || 'book');
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-selected', active ? 'true' : 'false');
  });
  const hasExtraFilters = Object.entries(state.filters).some(([k, v]) => k !== 'media_type' && Boolean(v));
  $('#clear-filters').classList.toggle('hidden', !hasExtraFilters);
}

async function loadReadingDesk() {
  try {
    const data = await api('/reading-desk');
    state.desk = data.books;
    renderReadingDesk();
  } catch (error) { toast(error.message, true); }
}

function renderReadingDesk() {
  const section = $('#reading-desk');
  section.classList.toggle('hidden', state.desk.length === 0);
  $('#reading-desk-list').innerHTML = state.desk.map(book => {
    const isAudio = book.media_type === 'audiobook' || ['m4b','mp3','m4a','ogg','opus','flac'].includes(book.format);
    const isVideo = book.media_type === 'video' || ['mp4','webm','mkv','m4v'].includes(book.format);
    const deskStatus = isAudio
      ? `${Math.round(book.progress_percent)}% OUVIDO`
      : isVideo
      ? `${Math.round(book.progress_percent)}% ASSISTIDO`
      : `${Math.round(book.progress_percent)}% LIDO`;
    return `
    <article class="desk-book" data-book-id="${book.id}" tabindex="0">
      ${coverMarkup(book)}
      <div class="desk-book-copy"><small>${deskStatus}</small><h3>${escapeHtml(book.title)}</h3><p>${escapeHtml(book.author || book.filename)}</p>
        <div class="progress-track"><i style="width:${book.progress_percent}%"></i></div>
      </div>
      <button class="desk-remove" type="button" data-desk-remove="${book.id}" aria-label="Retirar ${escapeHtml(book.title)} da mesa">×</button>
    </article>`;
  }).join('');
}

async function removeFromDesk(id) {
  try {
    const remove = async () => {
      const pending = (await pendingProgress()).find(item => item.bookId === id);
      await api(`/books/${id}/progress`, { method: 'DELETE' });
      if (pending) await progressTransaction((store, done) => {
        const request = store.get(pending.key);
        request.onsuccess = () => {
          if (request.result?.version === pending.version) store.delete(pending.key);
          done();
        };
      });
      if (globalThis.indexedDB) await updateProgressSyncStatus();
    };
    state.progressSaveChain = state.progressSaveChain.then(() => globalThis.navigator?.locks
      ? navigator.locks.request('estante-progress-sync', remove) : remove());
    await state.progressSaveChain;
    toast('Livro retirado da mesa de leitura.');
    await Promise.all([loadReadingDesk(), loadBooks()]);
  } catch (error) { state.progressSaveChain = Promise.resolve(); toast(error.message, true); }
}

async function loadSuggestions() {
  clearInterval(state.carouselTimer);
  state.carouselTimer = null;
  if (!state.settings?.carousel_enabled) {
    $('#suggestions').classList.add('hidden');
    return;
  }
  try {
    const data = await api('/suggestions');
    state.suggestions = data.books;
    state.carouselIndex = 0;
    $('#suggestions').classList.toggle('hidden', state.suggestions.length === 0);
    renderSuggestion();
    if (state.suggestions.length > 1) {
      state.carouselTimer = setInterval(() => moveSuggestion(1), state.settings.carousel_interval_seconds * 1000);
    }
  } catch (error) { toast(error.message, true); }
}

function renderSuggestion() {
  const book = state.suggestions[state.carouselIndex];
  if (!book) return;
  $('#suggestion-stage').innerHTML = `<article class="suggestion-card" data-book-id="${book.id}">
    <div class="suggestion-cover">${coverMarkup(book, true)}</div>
    <div><small class="suggestion-counter">SUGESTÃO ${state.carouselIndex + 1} DE ${state.suggestions.length}</small><h3>${escapeHtml(book.title)}</h3><p class="suggestion-author">${escapeHtml(book.author || 'Autor não informado')}</p><p>${escapeHtml(book.subjects?.slice(0, 3).join(' · ') || `Um título em ${book.format.toUpperCase()} escolhido ao acaso na sua estante.`)}</p><button class="button primary" type="button">Conhecer este livro →</button></div>
  </article>`;
}

function moveSuggestion(step) {
  if (!state.suggestions.length) return;
  state.carouselIndex = (state.carouselIndex + step + state.suggestions.length) % state.suggestions.length;
  renderSuggestion();
}

async function openBook(id) {
  try {
    const [{ book, collections, book_collection_ids: bookCollectionIds }, providerData] = await Promise.all([api(`/books/${id}`), api('/metadata/providers')]);
    state.currentBook = book;
    state.collections = collections;
    state.bookCollectionIds = bookCollectionIds;
    state.providers = providerData.providers;
    $('#book-detail').innerHTML = detailMarkup(book);
    bindBookDetail();
    $('#book-dialog').showModal();
    loadShares();
  } catch (error) { toast(error.message, true); }
}

function detailMarkup(book) {
  const canRead = ['pdf','epub','mobi','azw','azw3','cbz','txt','md','html','htm','fb2'].includes(book.format);
  const isAudio = book.media_type === 'audiobook' || ['m4b','mp3','m4a','ogg','opus','flac'].includes(book.format);
  const isVideo = book.media_type === 'video' || ['mp4','webm','mkv','m4v'].includes(book.format);
  const canPlay = canRead || isAudio || isVideo;
  const actionLabel = isAudio
    ? (book.progress?.percent ? 'Continuar audição' : 'Ouvir audiolivro')
    : isVideo
    ? (book.progress?.percent ? 'Continuar assistindo' : 'Assistir aula / vídeo')
    : (book.progress?.percent ? 'Continuar leitura' : 'Começar a ler');
  const categoryLabel = isAudio ? '🎧 AUDIOLIVRO' : isVideo ? '🎬 CURSO / VÍDEO' : '📚 LIVRO';
  const progressUnit = isAudio ? 'ouvido' : isVideo ? 'assistido' : 'lido';
  const providers = state.providers.filter(item => item.enabled).map(item => `<option value="${item.id}">${escapeHtml(item.name)}</option>`).join('');
  return `<div class="book-detail-layout">
    <div class="detail-cover-area">${coverMarkup(book, true)}</div>
    <div class="detail-body">
      <p class="eyebrow">${categoryLabel} · ${escapeHtml(book.format.toUpperCase())} · ${formatBytes(book.size)}</p>
      <h2>${escapeHtml(book.title)}</h2><p class="detail-author">${escapeHtml(book.author || 'Autor não informado')}</p>
      <p class="detail-description">${escapeHtml(book.description || 'Sem sinopse. Você pode editar os dados manualmente ou buscar metadados em uma fonte habilitada.')}</p>
      <div class="detail-actions">
        <button class="button primary" id="read-book" ${canPlay && book.is_available ? '' : 'disabled'}>▶ ${actionLabel}</button>
        <a class="button ghost" href="/api/books/${book.id}/file?download=true">⇩ Baixar</a>
        <a class="button ghost" href="/api/books/${book.id}/notes/export?format=markdown" download>✎ Notas (.md)</a>
        <button class="button ghost" id="share-book">↗ Compartilhar</button>
        <button class="button ghost" id="edit-book">✎ Editar</button>
      </div>
      <div class="metadata-grid">
        <div><span>Arquivo</span>${escapeHtml(book.filename)}</div><div><span>Progresso</span>${Math.round(book.progress?.percent || 0)}% ${book.progress?.percent ? progressUnit : ''}</div>
        <div><span>Editora</span>${escapeHtml(book.publisher || '—')}</div><div><span>Publicação</span>${escapeHtml(book.published_date || '—')}</div>
        <div><span>ISBN</span>${escapeHtml(book.isbn || '—')}</div><div><span>Idioma</span>${escapeHtml(book.language || '—')}</div>
      </div>
      <section class="detail-section"><h3>Coleções</h3><div class="collection-checks">${state.collections.length ? state.collections.map(collection => `<label><input type="checkbox" data-book-collection="${collection.id}" ${state.bookCollectionIds.includes(collection.id) ? 'checked' : ''}><i style="--collection-color:${escapeHtml(collection.color)}"></i>${escapeHtml(collection.name)}</label>`).join('') : '<span class="detail-description">Crie uma coleção nas configurações.</span>'}</div></section>
      <section class="detail-section" id="edit-section" hidden>${editFormMarkup(book)}</section>
      <section class="detail-section">
        <h3>Completar metadados</h3>
        ${state.settings?.network_metadata_enabled ? `<form class="metadata-search" id="metadata-form"><select name="provider_id">${providers}</select><input name="q" required value="${escapeHtml([book.title, book.author].filter(Boolean).join(' '))}" placeholder="Título, autor ou ISBN"><button class="button ghost" type="submit">Buscar</button></form><div id="metadata-results" class="metadata-results"></div>` : '<p class="detail-description">A busca externa está desativada. Habilite-a nas configurações quando quiser consultar metadados.</p>'}
      </section>
      <section class="detail-section"><div class="section-title"><div><h3>Links de compartilhamento</h3><p>Links são criados somente por você e podem ser revogados.</p></div></div><div id="share-list" class="share-list"></div></section>
    </div>
  </div>`;
}

function editFormMarkup(book) {
  const fields = [
    ['title','Título',book.title,true], ['author','Autor',book.author], ['publisher','Editora',book.publisher],
    ['published_date','Publicação',book.published_date], ['isbn','ISBN',book.isbn], ['language','Idioma',book.language],
  ];
  return `<h3>Editar dados locais</h3><form id="edit-form" class="provider-form">${fields.map(([name,label,value,required]) => `<label class="field"><span>${label}</span><input name="${name}" value="${escapeHtml(value || '')}" ${required ? 'required' : ''}></label>`).join('')}<label class="field wide"><span>Assuntos (separados por vírgula)</span><input name="subjects" value="${escapeHtml((book.subjects || []).join(', '))}"></label><label class="field wide"><span>Descrição</span><textarea name="description" rows="5">${escapeHtml(book.description || '')}</textarea></label><button class="button primary small" type="submit">Salvar</button></form>`;
}

function bindBookDetail() {
  $('#read-book')?.addEventListener('click', () => { $('#book-dialog').close(); openReader(state.currentBook); });
  $('#edit-book')?.addEventListener('click', () => { $('#edit-section').hidden = !$('#edit-section').hidden; });
  $('#edit-form')?.addEventListener('submit', saveManualMetadata);
  $('#metadata-form')?.addEventListener('submit', searchMetadata);
  $('#share-book')?.addEventListener('click', createShare);
  $$('[data-book-collection]').forEach(input => input.onchange = () => toggleBookCollection(Number(input.dataset.bookCollection), input.checked));
}

async function toggleBookCollection(collectionId, enabled) {
  try {
    await api(`/collections/${collectionId}/books/${state.currentBook.id}`, { method: enabled ? 'PUT' : 'DELETE' });
    state.bookCollectionIds = enabled ? [...new Set([...state.bookCollectionIds, collectionId])] : state.bookCollectionIds.filter(id => id !== collectionId);
    toast(enabled ? 'Livro adicionado à coleção.' : 'Livro retirado da coleção.');
    loadBooks();
  } catch (error) { toast(error.message, true); }
}

async function saveManualMetadata(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const payload = Object.fromEntries(form);
  payload.subjects = payload.subjects.split(',').map(value => value.trim()).filter(Boolean);
  for (const key of ['author','publisher','published_date','isbn','language','description']) payload[key] ||= null;
  try {
    const { book } = await api(`/books/${state.currentBook.id}`, { method: 'PATCH', body: JSON.stringify(payload) });
    state.currentBook = book;
    $('#book-detail').innerHTML = detailMarkup(book); bindBookDetail(); loadShares(); loadBooks($('#search').value);
    toast('Metadados locais salvos.');
  } catch (error) { toast(error.message, true); }
}

async function searchMetadata(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  const button = $('button', event.currentTarget); button.disabled = true; button.textContent = 'Buscando…';
  try {
    const data = await api(`/metadata/search?provider_id=${form.get('provider_id')}&q=${encodeURIComponent(form.get('q'))}`);
    const target = $('#metadata-results');
    target.innerHTML = data.results.length ? data.results.map((result, index) => `<article class="metadata-result"><div><strong>${escapeHtml(result.title)}</strong><small>${escapeHtml(result.authors.join(', ') || 'Autor não informado')} · ${escapeHtml(result.published_date || 'sem data')} · ${escapeHtml(result.provider)}</small></div><button class="button ghost small" data-metadata-index="${index}">Usar</button></article>`).join('') : '<p class="detail-description">Nenhum resultado encontrado.</p>';
    $$('[data-metadata-index]', target).forEach(element => element.addEventListener('click', () => applyMetadata(data.results[Number(element.dataset.metadataIndex)])));
  } catch (error) { toast(error.message, true); }
  finally { button.disabled = false; button.textContent = 'Buscar'; }
}

async function applyMetadata(candidate) {
  try {
    const { book } = await api(`/books/${state.currentBook.id}/metadata`, { method: 'PUT', body: JSON.stringify(candidate) });
    state.currentBook = book; $('#book-detail').innerHTML = detailMarkup(book); bindBookDetail(); loadShares(); loadBooks($('#search').value);
    toast('Metadados e capa salvos localmente.');
  } catch (error) { toast(error.message, true); }
}

async function createShare() {
  try {
    const { path } = await api(`/books/${state.currentBook.id}/shares`, { method: 'POST', body: JSON.stringify({ expires_in_hours: 168 }) });
    const url = new URL(path, location.origin).href;
    try { await navigator.clipboard.writeText(url); toast('Link válido por 7 dias copiado.'); }
    catch (_) { prompt('Copie o link de compartilhamento:', url); }
    loadShares();
  } catch (error) { toast(error.message, true); }
}

async function loadShares() {
  if (!state.currentBook) return;
  try {
    const { shares } = await api(`/books/${state.currentBook.id}/shares`);
    const active = shares.filter(item => !item.revoked_at);
    $('#share-list').innerHTML = active.length ? active.map(item => {
      const path = `/api/public/${item.token}/file`;
      return `<div class="share-item"><code>${escapeHtml(path)} · ${item.expires_at ? `expira ${new Date(item.expires_at).toLocaleDateString()}` : 'sem expiração'}</code><button class="button danger small" data-revoke="${item.id}">Revogar</button></div>`;
    }).join('') : '<p class="detail-description">Nenhum link ativo.</p>';
    $$('[data-revoke]').forEach(button => button.addEventListener('click', async () => { await api(`/shares/${button.dataset.revoke}`, { method: 'DELETE' }); toast('Link revogado.'); loadShares(); }));
  } catch (error) { toast(error.message, true); }
}

async function openReader(book) {
  clearInterval(state.carouselTimer);
  state.carouselTimer = null;
  state.currentBook = book;
  let progress = book.progress;
  try {
    const pending = (await pendingProgress()).find(item => item.bookId === book.id);
    if (pending) progress = JSON.parse(pending.body);
  } catch (error) { toast(`Fila local indisponível: ${error.message}`, true); }
  state.reader = { kind: book.format, location: progress?.location || {}, percent: progress?.percent || 0, items: [] };
  if (!state.reader.percent) state.reader.percent = book.page_count ? Math.min(99, 100 / book.page_count) : 0.1;
  $('#reader-title').textContent = book.title;
  $('#reader-download').href = `/api/books/${book.id}/file?download=true`;
  $('#reader').classList.remove('hidden'); document.body.style.overflow = 'hidden';
  await renderReader();
  scheduleProgressSave(true);
  loadNotes();
}

async function renderReader() {
  const book = state.currentBook, reader = state.reader, stage = $('#reader-stage'), nav = $('#reader-navigation');
  stage.innerHTML = ''; nav.innerHTML = '';
  try {
    if (book.format === 'pdf') {
      const page = Math.max(1, Number(reader.location.page || 1));
      reader.location = { type: 'page', page };
      stage.innerHTML = `<iframe title="Leitor PDF" src="/api/books/${book.id}/file#page=${page}"></iframe>`;
      nav.innerHTML = `<button class="icon-button light" data-step="-1">‹</button><label>Pág. <input id="page-input" type="number" min="1" ${book.page_count ? `max="${book.page_count}"` : ''} value="${page}"></label><button class="icon-button light" data-step="1">›</button>`;
      nav.querySelectorAll('[data-step]').forEach(button => button.onclick = () => changePdfPage(Number(button.dataset.step)));
      $('#page-input').onchange = () => changePdfPage(0);
      updateReaderLabel();
    } else if (book.format === 'epub') {
      const { manifest } = await api(`/books/${book.id}/epub`); reader.items = manifest.chapters;
      renderPagedResource('epub', Number(reader.location.chapter || 0));
    } else if (book.format === 'cbz') {
      const { manifest } = await api(`/books/${book.id}/comic`); reader.items = manifest.pages;
      renderPagedResource('comic', Number(reader.location.page_index || 0));
    } else if (['mobi','azw','azw3','txt','md','html','htm','fb2'].includes(book.format)) {
      const endpoint = ['mobi','azw','azw3'].includes(book.format) ? 'mobi' : 'text';
      stage.innerHTML = `<iframe title="Leitor de texto" sandbox src="/api/books/${book.id}/${endpoint}"></iframe>`;
      renderPercentNavigation();
    } else if (['m4b','mp3','m4a','ogg','opus','flac'].includes(book.format)) {
      renderAudioPlayer();
    } else if (['mp4','webm','mkv','m4v'].includes(book.format)) {
      await renderVideoPlayer();
    } else throw new Error(`Leitura de ${book.format} ainda não disponível.`);
  } catch (error) { stage.innerHTML = `<div class="reader-error"><h2>Não foi possível abrir este livro</h2><p>${escapeHtml(error.message)}</p><a class="button light" href="/api/books/${book.id}/file?download=true">Baixar arquivo</a></div>`; }
}

function formatTime(seconds) {
  if (!seconds || isNaN(seconds)) return '00:00';
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = val => String(val).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function renderAudioPlayer() {
  const book = state.currentBook, stage = $('#reader-stage'), nav = $('#reader-navigation');
  const savedTime = Number(state.reader.location.time || 0);
  stage.innerHTML = `
    <div class="media-player-container audio-player-view">
      <div class="media-cover-wrapper">
        ${coverMarkup(book)}
      </div>
      <div class="media-info">
        <h2>${escapeHtml(book.title)}</h2>
        <p>${escapeHtml(book.author || book.filename)}</p>
      </div>
      <audio id="media-audio-element" controls preload="metadata" src="/api/books/${book.id}/file">
        Seu navegador não suporta a reprodução deste áudio.
      </audio>
    </div>`;
  nav.innerHTML = `
    <button class="icon-button light" id="audio-step-back" type="button" title="Voltar 15s">↺ 15s</button>
    <label>Velocidade:
      <select id="audio-speed-select">
        <option value="0.75">0.75x</option>
        <option value="1" selected>1x</option>
        <option value="1.25">1.25x</option>
        <option value="1.5">1.5x</option>
        <option value="2">2x</option>
      </select>
    </label>
    <label>Sono:
      <select id="audio-sleep-timer">
        <option value="0">Desativado</option>
        <option value="15">15 min</option>
        <option value="30">30 min</option>
        <option value="45">45 min</option>
        <option value="60">60 min</option>
      </select>
    </label>
    <span id="audio-sleep-countdown" class="sleep-countdown hidden" aria-live="polite"></span>
    <button class="icon-button light" id="audio-step-fwd" type="button" title="Avançar 30s">↻ 30s</button>
  `;
  const audio = $('#media-audio-element');
  if (audio) {
    if (savedTime > 0) {
      audio.onloadedmetadata = () => {
        audio.currentTime = savedTime;
        updateMediaProgress(audio, 'audio');
      };
    }
    audio.ontimeupdate = () => updateMediaProgress(audio, 'audio');
    audio.onpause = () => scheduleProgressSave(true);
    setupSleepTimer(audio);
    setupMediaSession(audio, book, 'audio');
  }
  $('#audio-speed-select')?.addEventListener('change', e => {
    if (audio) audio.playbackRate = Number(e.target.value);
  });
  $('#audio-step-back')?.addEventListener('click', () => {
    if (audio) audio.currentTime = Math.max(0, audio.currentTime - 15);
  });
  $('#audio-step-fwd')?.addEventListener('click', () => {
    if (audio) audio.currentTime = Math.min(audio.duration || Infinity, audio.currentTime + 30);
  });
  updateReaderLabel();
}

async function renderVideoPlayer() {
  const book = state.currentBook, stage = $('#reader-stage'), nav = $('#reader-navigation');
  const savedTime = Number(state.reader.location.time || 0);

  const [courseRes, subtitlesRes] = await Promise.all([
    api(`/books/${book.id}/course`).catch(() => ({ course: null })),
    api(`/books/${book.id}/subtitles`).catch(() => ({ subtitles: [] })),
  ]);
  const course = courseRes?.course;
  const subtitles = subtitlesRes?.subtitles || [];

  const trackElements = subtitles.map(sub => `
    <track kind="subtitles"
           label="${escapeHtml(sub.label)}"
           srclang="${escapeHtml(sub.srclang)}"
           src="/api/books/${book.id}/subtitles/${encodeURIComponent(sub.filename)}"
           ${sub.is_default ? 'default' : ''}>
  `).join('');

  if (course) {
    stage.innerHTML = `
      <div class="video-theater-layout">
        <div class="video-stage-main">
          <video id="media-video-element" controls preload="metadata" playsinline src="/api/books/${book.id}/file">
            ${trackElements}
            Seu navegador não suporta a reprodução deste vídeo.
          </video>
        </div>
        <aside class="course-sidebar" id="course-sidebar">
          <header class="course-sidebar-header">
            <h3 class="course-title" title="${escapeHtml(course.course_name)}">${escapeHtml(course.course_name)}</h3>
            <div class="course-meta">
              <span>${course.completed_lessons} de ${course.total_lessons} aulas</span>
              <strong>${Math.round(course.course_percent)}%</strong>
            </div>
            <div class="course-progress-bar">
              <div class="course-progress-fill" style="width: ${Math.round(course.course_percent)}%"></div>
            </div>
            <button type="button" class="course-create-collection-btn" id="course-create-collection">
              ＋ Criar Coleção deste Curso
            </button>
          </header>
          <ul class="course-lessons-list" id="course-lessons-list">
            ${course.lessons.map(lesson => `
              <li class="course-lesson-item ${lesson.is_current ? 'active' : ''}" data-lesson-id="${lesson.id}">
                <button type="button" class="lesson-check-btn ${lesson.completed ? 'completed' : ''}" data-toggle-complete="${lesson.id}" title="${lesson.completed ? 'Concluída' : 'Marcar como concluída'}">
                  ${lesson.completed ? '✓' : '○'}
                </button>
                <div class="lesson-details">
                  <span class="lesson-title" title="${escapeHtml(lesson.title)}">${escapeHtml(lesson.title)}</span>
                  <span class="lesson-meta">${Math.round(lesson.percent)}% · ${escapeHtml(lesson.format.toUpperCase())}</span>
                </div>
              </li>
            `).join('')}
          </ul>
        </aside>
      </div>`;
  } else {
    stage.innerHTML = `
      <div class="media-player-container video-player-view">
        <video id="media-video-element" controls preload="metadata" playsinline src="/api/books/${book.id}/file">
          ${trackElements}
          Seu navegador não suporta a reprodução deste vídeo.
        </video>
      </div>`;
  }

  nav.innerHTML = `
    <button class="icon-button light" id="video-step-back" type="button" title="Voltar 10s">↺ 10s</button>
    <label>Velocidade:
      <select id="video-speed-select">
        <option value="0.75">0.75x</option>
        <option value="1" selected>1x</option>
        <option value="1.25">1.25x</option>
        <option value="1.5">1.5x</option>
        <option value="2">2x</option>
      </select>
    </label>
    <button class="icon-button light" id="video-step-fwd" type="button" title="Avançar 10s">↻ 10s</button>
    ${course ? `<button class="button ghost small light" id="video-toggle-sidebar" type="button" title="Alternar painel de aulas">◨ Aulas (${course.completed_lessons}/${course.total_lessons})</button>` : ''}
    <button class="icon-button light" id="video-fullscreen-btn" type="button" title="Tela Cheia (F)">⛶</button>
  `;

  const video = $('#media-video-element');
  if (video) {
    if (savedTime > 0) {
      video.onloadedmetadata = () => {
        video.currentTime = savedTime;
        updateMediaProgress(video, 'video');
      };
    }
    video.ontimeupdate = () => {
      updateMediaProgress(video, 'video');
      if (video.duration && (video.currentTime / video.duration) >= 0.95 && state.reader.percent < 95) {
        state.reader.percent = 100;
        scheduleProgressSave(true);
      }
    };
    video.onended = () => {
      state.reader.percent = 100;
      scheduleProgressSave(true);
      toast('Aula concluída!');
      if (course) {
        const item = $(`[data-lesson-id="${book.id}"] .lesson-check-btn`);
        if (item) {
          item.classList.add('completed');
          item.textContent = '✓';
        }
      }
    };
    video.onpause = () => scheduleProgressSave(true);
    setupMediaSession(video, book, 'video');
  }

  $('#video-speed-select')?.addEventListener('change', e => {
    if (video) video.playbackRate = Number(e.target.value);
  });
  $('#video-step-back')?.addEventListener('click', () => {
    if (video) video.currentTime = Math.max(0, video.currentTime - 10);
  });
  $('#video-step-fwd')?.addEventListener('click', () => {
    if (video) video.currentTime = Math.min(video.duration || Infinity, video.currentTime + 10);
  });
  $('#video-toggle-sidebar')?.addEventListener('click', () => {
    $('#course-sidebar')?.classList.toggle('collapsed');
  });
  $('#video-fullscreen-btn')?.addEventListener('click', () => {
    toggleFullscreen($('#reader-stage'));
  });

  if (course) {
    $$('.course-lesson-item').forEach(item => {
      item.addEventListener('click', e => {
        if (e.target.closest('[data-toggle-complete]')) return;
        switchToLesson(Number(item.dataset.lessonId));
      });
    });

    $$('[data-toggle-complete]').forEach(btn => {
      btn.addEventListener('click', async e => {
        e.stopPropagation();
        const lessonId = Number(btn.dataset.toggleComplete);
        const isComp = btn.classList.contains('completed');
        const newPercent = isComp ? 0 : 100;
        try {
          await api(`/books/${lessonId}/progress`, {
            method: 'PUT',
            body: JSON.stringify({
              percent: newPercent,
              location: { type: 'video', time: 0, duration: 0 },
            }),
          });
          btn.classList.toggle('completed', !isComp);
          btn.textContent = !isComp ? '✓' : '○';
          toast(!isComp ? 'Aula marcada como concluída.' : 'Aula desmarcada.');
        } catch (err) {
          toast(err.message, true);
        }
      });
    });

    $('#course-create-collection')?.addEventListener('click', async () => {
      try {
        const existing = (state.collections || []).find(c => c.name.toLowerCase() === course.course_name.toLowerCase());
        let colId = existing?.id;
        if (!colId) {
          const created = await api('/collections', {
            method: 'POST',
            body: JSON.stringify({ name: course.course_name, color: '#10b981' }),
          });
          colId = created.id;
        }
        for (const lesson of course.lessons) {
          await api(`/collections/${colId}/books/${lesson.id}`, { method: 'PUT' });
        }
        toast(`Coleção "${course.course_name}" criada com ${course.lessons.length} aulas!`);
        await loadSettings();
      } catch (err) {
        toast(`Erro ao criar coleção: ${err.message}`, true);
      }
    });
  }

  updateReaderLabel();
}

async function switchToLesson(lessonId) {
  if (state.currentBook?.id === lessonId) return;
  await persistProgress();
  try {
    const data = await api(`/books/${lessonId}`);
    if (data?.book) {
      state.currentBook = data.book;
      let progress = data.book.progress;
      try {
        const pending = (await pendingProgress()).find(item => item.bookId === data.book.id);
        if (pending) progress = JSON.parse(pending.body);
      } catch (_) {}
      state.reader = { kind: data.book.format, location: progress?.location || {}, percent: progress?.percent || 0, items: [] };
      $('#reader-title').textContent = data.book.title;
      $('#reader-download').href = `/api/books/${data.book.id}/file?download=true`;
      await renderVideoPlayer();
      const video = $('#media-video-element');
      if (video) video.play().catch(() => {});
      loadNotes();
    }
  } catch (error) {
    toast(`Não foi possível alternar a aula: ${error.message}`, true);
  }
}

function toggleFullscreen(element) {
  if (!document.fullscreenElement) {
    (element || document.documentElement).requestFullscreen?.().catch(() => {});
  } else {
    document.exitFullscreen?.().catch(() => {});
  }
}

function setupMediaSession(media, book, type) {
  if (!('mediaSession' in navigator)) return;
  try {
    const artwork = book.has_cover ? [
      { src: `/api/books/${book.id}/cover`, sizes: '512x512', type: 'image/jpeg' },
      { src: `/api/books/${book.id}/cover`, sizes: '256x256', type: 'image/jpeg' },
    ] : [];
    navigator.mediaSession.metadata = new MediaMetadata({
      title: book.title,
      artist: book.author || 'Estante Livre',
      album: type === 'audio' ? 'Audiolivro' : 'Curso em Vídeo',
      artwork,
    });
    navigator.mediaSession.setActionHandler('play', () => { media.play(); });
    navigator.mediaSession.setActionHandler('pause', () => { media.pause(); });
    navigator.mediaSession.setActionHandler('seekbackward', details => {
      const skip = details.seekOffset || (type === 'audio' ? 15 : 10);
      media.currentTime = Math.max(0, media.currentTime - skip);
    });
    navigator.mediaSession.setActionHandler('seekforward', details => {
      const skip = details.seekOffset || (type === 'audio' ? 30 : 10);
      media.currentTime = Math.min(media.duration || Infinity, media.currentTime + skip);
    });
    if ('seekto' in navigator.mediaSession) {
      navigator.mediaSession.setActionHandler('seekto', details => {
        if (details.seekTime != null) media.currentTime = details.seekTime;
      });
    }
  } catch (_) { /* navegadores sem suporte completo a MediaSession */ }
}

function syncMediaSessionPosition(media) {
  if (!('mediaSession' in navigator) || !navigator.mediaSession.setPositionState) return;
  if (!media.duration || isNaN(media.duration)) return;
  try {
    navigator.mediaSession.setPositionState({
      duration: media.duration,
      playbackRate: media.playbackRate || 1,
      position: Math.min(media.currentTime || 0, media.duration),
    });
  } catch (_) {}
}

function setupSleepTimer(audio) {
  const select = $('#audio-sleep-timer');
  const label = $('#audio-sleep-countdown');
  if (!select || !label) return;
  select.onchange = () => {
    clearInterval(state.sleepTimer);
    state.sleepTimer = null;
    const minutes = Number(select.value);
    if (minutes <= 0) {
      label.classList.add('hidden');
      return;
    }
    state.sleepTimerEndsAt = Date.now() + minutes * 60 * 1000;
    label.classList.remove('hidden');
    updateSleepLabel(minutes * 60);
    state.sleepTimer = setInterval(() => {
      const remaining = Math.max(0, Math.round((state.sleepTimerEndsAt - Date.now()) / 1000));
      if (remaining <= 0) {
        clearInterval(state.sleepTimer);
        state.sleepTimer = null;
        label.classList.add('hidden');
        select.value = '0';
        if (audio && !audio.paused) {
          audio.pause();
          toast('Temporizador de sono: áudio pausado.');
        }
      } else {
        updateSleepLabel(remaining);
      }
    }, 1000);
  };
}

function updateSleepLabel(seconds) {
  const label = $('#audio-sleep-countdown');
  if (!label) return;
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  label.textContent = `⏳ ${m}:${s < 10 ? '0' : ''}${s}`;
}

function updateMediaProgress(media, type) {
  if (!media) return;
  const time = media.currentTime || 0;
  const duration = media.duration && !isNaN(media.duration) ? media.duration : 0;
  const percent = duration > 0 ? Math.min(100, Math.max(0, (time / duration) * 100)) : 0;
  state.reader.percent = percent;
  state.reader.location = { type, time, duration };
  updateReaderLabel();
  syncMediaSessionPosition(media);
  scheduleProgressSave();
}

function changePdfPage(step) {
  const input = $('#page-input');
  let page = Math.max(1, Number(input.value || 1) + step);
  if (input.max) page = Math.min(page, Number(input.max));
  input.value = page; state.reader.location = { type: 'page', page };
  state.reader.percent = state.currentBook.page_count ? Math.min(100, page / state.currentBook.page_count * 100) : state.reader.percent;
  $('iframe', $('#reader-stage')).src = `/api/books/${state.currentBook.id}/file#page=${page}`;
  updateReaderLabel(); scheduleProgressSave();
}

function renderPagedResource(kind, requestedIndex) {
  const items = state.reader.items, max = Math.max(0, items.length - 1), index = Math.min(max, Math.max(0, requestedIndex));
  state.reader.percent = items.length ? (index + 1) / items.length * 100 : 0;
  if (kind === 'epub') {
    const item = items[index]; state.reader.location = { type: 'chapter', chapter: index, href: item.href };
    $('#reader-stage').innerHTML = `<iframe title="Leitor EPUB" sandbox src="/api/books/${state.currentBook.id}/epub/resource/${encodePath(item.href)}"></iframe>`;
  } else {
    const item = items[index]; state.reader.location = { type: 'comic', page_index: index, page: index + 1 };
    $('#reader-stage').innerHTML = `<img class="comic-page" alt="Página ${index + 1}" src="/api/books/${state.currentBook.id}/comic/resource/${encodePath(item)}">`;
  }
  $('#reader-navigation').innerHTML = `<button class="icon-button light" data-reader-step="-1" ${index === 0 ? 'disabled' : ''}>‹</button><span>${index + 1} / ${items.length}</span><button class="icon-button light" data-reader-step="1" ${index === max ? 'disabled' : ''}>›</button>`;
  $$('[data-reader-step]').forEach(button => button.onclick = () => renderPagedResource(kind, index + Number(button.dataset.readerStep)));
  updateReaderLabel(); scheduleProgressSave();
}

function renderPercentNavigation() {
  const value = Math.round(state.reader.percent || 0);
  $('#reader-navigation').innerHTML = `<label>Progresso <input id="percent-input" type="range" min="0" max="100" value="${value}"> <span id="percent-value">${value}%</span></label>`;
  $('#percent-input').oninput = event => { state.reader.percent = Number(event.target.value); state.reader.location = { type: 'percent', percent: state.reader.percent }; $('#percent-value').textContent = `${state.reader.percent}%`; updateReaderLabel(); scheduleProgressSave(); };
  $('#percent-input').onchange = () => scheduleProgressSave(true);
  updateReaderLabel();
}

function updateReaderLabel() {
  const location = state.reader.location;
  if (location.page) {
    $('#reader-location').textContent = `Página ${location.page} · ${Math.round(state.reader.percent)}%`;
  } else if (location.type === 'chapter') {
    $('#reader-location').textContent = `Capítulo ${location.chapter + 1} · ${Math.round(state.reader.percent)}%`;
  } else if (location.type === 'audio' || location.type === 'video') {
    $('#reader-location').textContent = `${formatTime(location.time || 0)} / ${formatTime(location.duration || 0)} · ${Math.round(state.reader.percent)}%`;
  } else {
    $('#reader-location').textContent = `${Math.round(state.reader.percent)}% lido`;
  }
}

function scheduleProgressSave(immediate = false) {
  if (!state.currentBook || $('#reader').classList.contains('hidden')) return;
  clearTimeout(state.progressSaveTimer);
  state.progressSaveTimer = setTimeout(persistProgress, immediate ? 0 : 700);
}

// Uma entrada por livro e biblioteca; a confirmacao so remove a versao enviada.
let progressDatabase;
function openProgressDatabase() {
  if (!globalThis.indexedDB) return Promise.resolve(null);
  if (!progressDatabase) progressDatabase = new Promise((resolve, reject) => {
    const request = indexedDB.open('estante-progress', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('pending', { keyPath: 'key' });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => { progressDatabase = null; reject(request.error); };
    request.onblocked = () => { progressDatabase = null; reject(new Error('Armazenamento local bloqueado')); };
  });
  return progressDatabase;
}

function progressScope() { return JSON.stringify([state.settings?.catalog_id || '', state.settings?.library_root || '']); }

/** A transacao le e altera a fila atomicamente, inclusive entre abas. */
async function progressTransaction(operation) {
  const database = await openProgressDatabase();
  if (!database) return null;
  return new Promise((resolve, reject) => {
    const transaction = database.transaction('pending', 'readwrite');
    const store = transaction.objectStore('pending');
    let result;
    operation(store, value => { result = value; });
    transaction.oncomplete = () => resolve(result);
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error || new Error('Fila local cancelada'));
  });
}

async function pendingProgress() {
  const scope = progressScope();
  return (await progressTransaction((store, done) => {
    const request = store.getAll();
    request.onsuccess = () => done(request.result.filter(item => item.scope === scope));
  })) || [];
}

async function sendPendingProgress() {
  const items = await pendingProgress();
  let failed;
  for (const item of items) {
    try {
      await api(`/books/${item.bookId}/progress`, { method: 'PUT', body: item.body, keepalive: true, headers: item.catalogId ? { 'X-Catalog-Id': item.catalogId } : {} });
    } catch (error) { failed = error; continue; }
    await progressTransaction((store, done) => {
      const request = store.get(item.key);
      request.onsuccess = () => {
        if (request.result?.version === item.version) store.delete(item.key);
        done();
      };
    });
  }
  await updateProgressSyncStatus();
  if (failed) throw failed;
}

async function updateProgressSyncStatus() {
  const count = (await pendingProgress()).length;
  for (const selector of ['#progress-sync-status', '#reader-sync-status']) {
    const target = $(selector);
    if (target) target.textContent = count ? `Progresso pendente de ${count} livro(s)` : '';
  }
}

/** Coordena envios entre abas quando Web Locks esta disponivel. */
function syncPendingProgress() {
  if (globalThis.navigator?.locks) return navigator.locks.request('estante-progress-sync', sendPendingProgress);
  return sendPendingProgress();
}

/** Grava localmente antes da rede; captura o livro antes de qualquer espera. */
function persistProgress() {
  clearTimeout(state.progressSaveTimer);
  state.progressSaveTimer = null;
  if (!state.currentBook) return Promise.resolve();
  const bookId = state.currentBook.id;
  const body = JSON.stringify({ location: state.reader.location, percent: state.reader.percent });
  const scope = progressScope();
  const item = { key: JSON.stringify([scope, bookId]), scope, bookId, body, catalogId: state.settings?.catalog_id || '',
    createdAt: Date.now(), version: `${Date.now()}-${Math.random()}` };
  // Inicia a gravacao agora, mesmo quando um envio anterior ainda esta aguardando.
  let storageError;
  const stored = progressTransaction((store, done) => {
    const request = store.get(item.key);
    request.onsuccess = () => {
      if (!request.result || (request.result.createdAt || 0) <= item.createdAt) store.put(item);
      done();
    };
  });
  const durable = stored.catch(error => { storageError = error; });
  state.progressSaveChain = state.progressSaveChain.then(async () => {
    await durable;
    if (globalThis.indexedDB && !storageError) await updateProgressSyncStatus();
    if (!globalThis.indexedDB || storageError) {
      await api(`/books/${bookId}/progress`, { method: 'PUT', body, keepalive: true,
        headers: item.catalogId ? { 'X-Catalog-Id': item.catalogId } : {} });
      if (storageError) toast('Progresso salvo no servidor; a fila local está indisponível.', true);
    } else {
      await syncPendingProgress();
    }
  }).catch(error => {
    const message = storageError || !globalThis.indexedDB
      ? `Progresso não salvo: ${error.message}. Armazenamento local indisponível.`
      : `Progresso pendente: ${error.message}. Nova tentativa ao reconectar.`;
    toast(message, true);
  });
  return state.progressSaveChain;
}

function retryPendingProgress() {
  state.progressSaveChain = state.progressSaveChain.then(syncPendingProgress).catch(() => {});
  return state.progressSaveChain;
}

function flushProgressOnPageExit() {
  if (!state.currentBook || $('#reader').classList.contains('hidden')) return;
  persistProgress();
}

async function loadNotes() {
  if (!state.currentBook) return;
  try {
    const { notes } = await api(`/books/${state.currentBook.id}/notes`);
    state.notes = notes;
    const mdExport = $('#export-notes-md');
    if (mdExport) mdExport.href = `/api/books/${state.currentBook.id}/notes/export?format=markdown`;
    const jsonExport = $('#export-notes-json');
    if (jsonExport) jsonExport.href = `/api/books/${state.currentBook.id}/notes/export?format=json`;
    const toggleLabel = $('#notes-toggle .desktop-label');
    if (toggleLabel) toggleLabel.textContent = notes.length ? `Anotações (${notes.length})` : 'Anotações';
    renderNotesList();
  } catch (error) { toast(error.message, true); }
}

function renderNotesList() {
  const list = $('#notes-list');
  if (!list) return;
  const filter = (state.notesFilter || '').toLowerCase().trim();
  const colorFilter = state.notesColorFilter || 'all';

  const filtered = (state.notes || []).filter(note => {
    const loc = note.location || {};
    const quote = (loc.quote || loc.selected_text || '').toLowerCase();
    const content = (note.content || '').toLowerCase();
    const color = loc.color || '';
    if (colorFilter !== 'all' && color !== colorFilter) return false;
    if (filter && !content.includes(filter) && !quote.includes(filter)) return false;
    return true;
  });

  if (!filtered.length) {
    list.innerHTML = (state.notes || []).length
      ? '<p class="detail-description">Nenhuma anotação corresponde aos filtros aplicados.</p>'
      : '<p class="detail-description">As anotações e destaques feitos durante a leitura aparecerão aqui.</p>';
    return;
  }

  list.innerHTML = filtered.map(note => {
    const loc = note.location || {};
    const quote = loc.quote || loc.selected_text;
    const color = loc.color || '';
    const colorClass = color ? `swatch swatch-${escapeHtml(color)}` : '';
    const colorStyle = color === 'yellow' ? '--quote-color:#eab308;--quote-bg:rgba(254,240,138,0.25)'
      : color === 'green' ? '--quote-color:#22c55e;--quote-bg:rgba(187,247,208,0.25)'
      : color === 'blue' ? '--quote-color:#3b82f6;--quote-bg:rgba(191,219,254,0.25)'
      : '';

    return `<article class="note" data-note-id="${note.id}">
      ${quote ? `<blockquote class="note-quote" style="${colorStyle}">${escapeHtml(quote)}</blockquote>` : ''}
      <p>${escapeHtml(note.content)}</p>
      <footer>
        <button type="button" class="note-location-link" data-jump-note="${note.id}">
          ${escapeHtml(locationLabel(note.location))}
        </button>
        <span>${color ? `<span class="${colorClass}">${escapeHtml(color)}</span> · ` : ''}${new Date(note.updated_at).toLocaleDateString()}</span>
        <button data-note-delete="${note.id}" aria-label="Excluir anotação">Excluir</button>
      </footer>
    </article>`;
  }).join('');

  $$('[data-note-delete]', list).forEach(button => button.onclick = async () => {
    if (!confirm('Excluir esta anotação definitivamente?')) return;
    await api(`/notes/${button.dataset.noteDelete}`, { method: 'DELETE' });
    loadNotes();
  });

  $$('[data-jump-note]', list).forEach(button => button.onclick = () => {
    const note = (state.notes || []).find(n => n.id === Number(button.dataset.jumpNote));
    if (note && note.location) jumpToLocation(note.location);
  });
}

function jumpToLocation(loc) {
  if (!loc) return;
  if (loc.page && state.reader.kind === 'pdf') {
    const input = $('#page-input');
    if (input) changePdfPage(Number(loc.page) - Number(input.value || 1));
  } else if (loc.chapter !== undefined && state.reader.kind === 'epub') {
    renderPagedResource('epub', Number(loc.chapter));
  } else if ((loc.page_index !== undefined || loc.page !== undefined) && state.reader.kind === 'cbz') {
    renderPagedResource('comic', loc.page_index !== undefined ? Number(loc.page_index) : Math.max(0, Number(loc.page) - 1));
  } else if (loc.percent !== undefined) {
    const slider = $('#percent-input');
    if (slider) {
      slider.value = Math.round(Number(loc.percent));
      slider.dispatchEvent(new Event('input'));
      slider.dispatchEvent(new Event('change'));
    }
  } else if (loc.time !== undefined) {
    const media = $('#media-audio-element') || $('#media-video-element');
    if (media) media.currentTime = Number(loc.time);
  }
}

function locationLabel(location = {}) {
  location ||= {};
  if (location.type === 'chapter') return `Capítulo ${Number(location.chapter) + 1}`;
  if (location.type === 'page') return `Página ${location.page}`;
  if (location.time !== undefined) return formatTime(location.time);
  return `${Math.round(location.percent || 0)}%`;
}

async function closeReader() {
  clearTimeout(state.progressSaveTimer);
  clearInterval(state.sleepTimer);
  state.sleepTimer = null;
  const media = $('#media-audio-element') || $('#media-video-element');
  if (media) media.pause();
  if ('mediaSession' in navigator) {
    try {
      navigator.mediaSession.metadata = null;
      ['play', 'pause', 'seekbackward', 'seekforward', 'seekto'].forEach(action => {
        navigator.mediaSession.setActionHandler(action, null);
      });
    } catch (_) {}
  }
  await persistProgress();
  $('#reader').classList.add('hidden'); $('#notes-panel').classList.remove('open'); document.body.style.overflow = '';
  await Promise.all([loadBooks($('#search').value), loadReadingDesk(), loadSuggestions()]);
}

async function loadSettings() {
  try {
    state.settings = await api('/settings');
    const form = $('#settings-form');
    form.elements.library_root.value = state.settings.library_root;
    form.elements.network_metadata_enabled.checked = state.settings.network_metadata_enabled;
    form.elements.scan_profile.value = state.settings.scan_profile;
    form.elements.carousel_enabled.checked = state.settings.carousel_enabled;
    form.elements.carousel_interval_seconds.value = state.settings.carousel_interval_seconds;
    form.elements.auto_cover_enabled.checked = state.settings.auto_cover_enabled;
    $('#network-status').textContent = state.settings.network_metadata_enabled ? 'Consultas externas somente sob seu comando' : 'Nenhuma consulta externa permitida';
    applyBranding();
    const [data, collectionData] = await Promise.all([api('/metadata/providers'), api('/collections')]);
    state.providers = data.providers; state.collections = collectionData.collections;
    renderProviders(); renderCollections();
    $('#auth-status-detail').textContent = state.settings.auth_enabled ? 'Ativa por AUTH_USERNAME e AUTH_PASSWORD. Use HTTPS fora da máquina local.' : 'Desativada. Defina AUTH_USERNAME e AUTH_PASSWORD no ambiente para proteger a API.';
    $('#auth-status-title').textContent = state.settings.auth_enabled ? 'API protegida' : 'API sem autenticação';
    await loadSuggestions();
  } catch (error) { toast(error.message, true); }
}

function renderCollections() {
  $('#collection-list').innerHTML = state.collections.map(collection => `<form class="collection-admin-row" data-collection-id="${collection.id}"><i style="--collection-color:${escapeHtml(collection.color)}"></i><input name="name" value="${escapeHtml(collection.name)}" maxlength="80" required><input name="color" type="color" value="${escapeHtml(collection.color)}" aria-label="Cor"><small>${collection.book_count} livros</small><button class="button ghost small" type="submit">Salvar</button><button class="button danger small" type="button" data-collection-delete>Excluir</button></form>`).join('');
  $$('.collection-admin-row').forEach(row => {
    row.onsubmit = event => saveCollection(event, row);
    $('[data-collection-delete]', row).onclick = () => deleteCollection(row);
  });
}

async function saveCollection(event, row) {
  event.preventDefault();
  const payload = Object.fromEntries(new FormData(row));
  try { await api(`/collections/${row.dataset.collectionId}`, { method: 'PUT', body: JSON.stringify(payload) }); await loadSettings(); await loadBooks(); toast('Coleção atualizada.'); }
  catch (error) { toast(error.message, true); }
}

async function deleteCollection(row) {
  const name = row.elements.name.value;
  if (!confirm(`Excluir a coleção “${name}”? Os livros não serão apagados.`)) return;
  try { await api(`/collections/${row.dataset.collectionId}`, { method: 'DELETE' }); await loadSettings(); await loadBooks(); toast('Coleção excluída.'); }
  catch (error) { toast(error.message, true); }
}

async function loadMaintenance() {
  try {
    const [{ health }, { groups }, { backups }, { items }, fts] = await Promise.all([
      api('/maintenance/health'), api('/maintenance/duplicates'), api('/maintenance/backups'), api('/maintenance/trash'), api('/maintenance/fts'),
    ]);
    renderHealth(health);
    renderDuplicates(groups);
    renderBackups(backups);
    renderTrash(items);
    renderFtsStatus(fts);
  } catch (error) { toast(`Manutenção: ${error.message}`, true); }
}

function renderFtsStatus(fts) {
  if (!fts) return;
  const percent = Math.round(fts.percent);
  $('#fts-indexed-summary').textContent = `${percent}%`;
  $('#fts-status-message').textContent = `${fts.indexed_books} de ${fts.total_books} livro(s) com conteúdo indexado (${percent}%).`;
}

async function reindexFts() {
  const button = $('#reindex-fts-btn');
  button.disabled = true;
  $('#fts-status-message').textContent = 'Indexando conteúdo textual…';
  try {
    let result;
    do {
      result = await api('/maintenance/fts', { method: 'POST' });
      renderFtsStatus(result);
      if (result.processed === 0) break;
    } while (result.indexed_books < result.total_books);
    toast('Indexação FTS concluída.');
  } catch (error) {
    toast(`Falha na indexação FTS: ${error.message}`, true);
  } finally {
    button.disabled = false;
    await loadMaintenance();
  }
}

function renderHealth(health) {
  const cards = [
    ['Livros no catálogo', health.total, 'total'],
    ['Disponíveis', health.available, 'good'],
  ];
  if (health.media_counts) {
    cards.push(['E-books e documentos', health.media_counts.books, 'good']);
    cards.push(['Audiolivros', health.media_counts.audiobooks, 'good']);
    cards.push(['Vídeos e cursos', health.media_counts.videos, 'good']);
  }
  cards.push(
    ['Arquivos ausentes', health.unavailable, health.unavailable ? 'warn' : 'good'],
    ['Sem autor', health.without_author, health.without_author ? 'warn' : 'good'],
    ['Sem assunto', health.without_subject, health.without_subject ? 'warn' : 'good'],
    ['Sem capa', health.without_cover, health.without_cover ? 'warn' : 'good'],
    ['Sem idioma', health.without_language, health.without_language ? 'warn' : 'good'],
    ['Grupos duplicados', health.duplicate_groups, health.duplicate_groups ? 'warn' : 'good'],
  );
  const issues = health.scan_issues || [];
  $('#health-grid').innerHTML = cards.map(([label, value, tone]) => `<article class="health-card ${tone}"><strong>${value}</strong><span>${label}</span></article>`).join('')
    + (issues.length ? `<details class="scan-issues"><summary>${issues.length} aviso(s) da última varredura</summary>${issues.map(issue => `<p><strong>${escapeHtml(issue.path)}</strong><small>${escapeHtml(issue.message)}</small></p>`).join('')}</details>` : '');
}

function renderDuplicates(groups) {
  $('#duplicate-count').textContent = groups.length;
  $('#duplicate-list').innerHTML = groups.length ? groups.map((group, index) => `<article class="duplicate-group">
    <header><strong>Grupo ${index + 1}</strong><span>${group.books.length} arquivos · ${formatBytes(group.size)}</span></header>
    <small class="fingerprint" title="${escapeHtml(group.fingerprint)}">Assinatura ${escapeHtml(group.fingerprint.slice(0, 16))}…</small>
    ${group.books.map((book, bookIndex) => `<div class="duplicate-book"><div><strong>${escapeHtml(book.title)}</strong><small>${escapeHtml(book.relative_path)}</small></div>${bookIndex === 0 ? '<span class="keep-label">MANTER</span>' : `<button class="button danger small" type="button" data-delete-book="${book.id}" data-filename="${escapeHtml(book.filename)}">Excluir arquivo</button>`}</div>`).join('')}
  </article>`).join('') : '<p class="detail-description">Nenhuma provável duplicata encontrada. A comparação usa tamanho e assinatura parcial do conteúdo.</p>';
  $$('[data-delete-book]', $('#duplicate-list')).forEach(button => button.onclick = () => deleteDuplicateFile(button));
}

async function deleteDuplicateFile(button) {
  const filename = button.dataset.filename;
  const confirmation = prompt(`O conteúdo completo será comparado com outra cópia. O arquivo irá para a lixeira e poderá ser restaurado.\n\nPara confirmar, digite exatamente:\n${filename}`);
  if (confirmation !== filename) {
    if (confirmation !== null) toast('Nome diferente: exclusão cancelada.', true);
    return;
  }
  try {
    await api(`/books/${button.dataset.deleteBook}/file`, { method: 'DELETE', body: JSON.stringify({ filename }) });
    toast(`Arquivo ${filename} movido para a lixeira.`);
    await Promise.all([loadMaintenance(), loadBooks()]);
  } catch (error) { toast(error.message, true); }
}

function renderTrash(items) {
  $('#trash-list').innerHTML = items.length ? items.map(item => `<article class="backup-row"><div><strong>${escapeHtml(item.filename)}</strong><small>${escapeHtml(item.relative_path)}</small></div><button class="button ghost small" type="button" data-trash-restore="${escapeHtml(item.token)}">Restaurar</button></article>`).join('') : '<p class="detail-description">A lixeira está vazia.</p>';
  $$('[data-trash-restore]').forEach(button => button.onclick = async () => {
    try {
      const result = await api(`/maintenance/trash/${button.dataset.trashRestore}/restore`, { method: 'POST' });
      toast(result.rescan_needed ? 'Arquivo restaurado. Clique em Atualizar para catalogá-lo.' : 'Arquivo restaurado.');
      await Promise.all([loadMaintenance(), loadBooks()]);
    } catch (error) { toast(error.message, true); }
  });
}

async function previewCoverCleanup() {
  try {
    const { items, retention_days: days } = await api('/maintenance/covers/orphans');
    $('#cover-cleanup-preview').textContent = items.length
      ? `${items.length} capa(s), ${formatBytes(items.reduce((sum, item) => sum + item.size, 0))}. Sem referência e com pelo menos ${days} dias. Arquivos: ${items.map(item => item.filename).join(', ')}`
      : 'Nenhuma capa antiga sem referência.';
    const button = $('#clean-orphan-covers');
    button.classList.toggle('hidden', !items.length);
    button.onclick = async () => {
      if (!confirm(`Remover definitivamente ${items.length} capa(s) listadas na prévia?`)) return;
      try {
        const { removed } = await api('/maintenance/covers/orphans', { method: 'POST', body: JSON.stringify({ filenames: items.map(item => item.filename) }) });
        toast(`${removed} capa(s) removida(s).`);
        await previewCoverCleanup();
      } catch (error) { toast(error.message, true); }
    };
  } catch (error) { toast(error.message, true); }
}

function renderBackups(backups) {
  $('#backup-list').innerHTML = backups.length ? backups.map(backup => `<article class="backup-row"><div><strong>${escapeHtml(backup.filename)}</strong><small>${formatBytes(backup.size)} · ${new Date(backup.modified_at).toLocaleString()}</small></div><a class="button ghost small" href="/api/maintenance/backups/${encodeURIComponent(backup.filename)}">Baixar</a><button class="button danger small" type="button" data-backup-delete="${escapeHtml(backup.filename)}">Excluir</button></article>`).join('') : '<p class="detail-description">Nenhum backup criado ainda.</p>';
  $$('[data-backup-delete]', $('#backup-list')).forEach(button => button.onclick = () => deleteBackup(button.dataset.backupDelete));
}

async function createBackup() {
  const button = $('#create-backup');
  button.disabled = true; button.textContent = 'Criando…';
  try { await api('/maintenance/backups', { method: 'POST' }); await loadMaintenance(); toast('Backup consistente criado. Os livros originais não foram duplicados.'); }
  catch (error) { toast(error.message, true); }
  finally { button.disabled = false; button.textContent = 'Criar backup agora'; }
}

async function deleteBackup(filename) {
  if (!confirm(`Excluir o backup “${filename}”?`)) return;
  try { await api(`/maintenance/backups/${encodeURIComponent(filename)}`, { method: 'DELETE' }); await loadMaintenance(); toast('Backup excluído.'); }
  catch (error) { toast(error.message, true); }
}

async function restoreBackup(file) {
  if (!file) return;
  const confirmation = prompt('A restauração substituirá catálogo, progresso e configurações. Um backup de segurança será criado antes.\n\nDigite RESTAURAR para continuar:');
  if (confirmation !== 'RESTAURAR') { $('#restore-input').value = ''; return; }
  const form = new FormData(); form.append('backup', file);
  try {
    const data = await api('/maintenance/restore', { method: 'POST', body: form });
    toast(`Restauração concluída. Backup de segurança: ${data.safety_backup.filename}`);
    await Promise.all([loadSettings(), loadBooks(), loadReadingDesk(), loadMaintenance()]);
  } catch (error) { toast(error.message, true); }
  $('#restore-input').value = '';
}

async function generateCovers() {
  try {
    const status = await api('/maintenance/covers', { method: 'POST' });
    state.scanObservedRunning = true; renderScanStatus(status); scheduleScanPoll();
    $('#settings-dialog').close();
    toast('Geração sequencial de capas iniciada com prioridade baixa.');
  } catch (error) { toast(error.message, true); }
}

async function importOpds(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const url = new FormData(form).get('url');
  if (!confirm(`Importar e baixar os livros anunciados por este catálogo?\n${url}`)) return;
  const button = $('button', form); button.disabled = true; button.textContent = 'Importando…';
  try {
    const { report } = await api('/opds/import', { method: 'POST', body: JSON.stringify({ url }) });
    toast(`${report.imported} livro(s) importado(s); ${report.skipped} ignorado(s). Iniciando a catalogação.`);
    form.reset(); await scanLibrary(); $('#settings-dialog').close();
  } catch (error) { toast(error.message, true); }
  finally { button.disabled = false; button.textContent = 'Importar livros do OPDS'; }
}

function applyBranding() {
  const branding = state.settings?.branding || {};
  const logo = $('#brand-logo'), fallback = $('#brand-fallback');
  if (branding.logo) {
    logo.src = branding.logo; logo.classList.remove('hidden'); fallback.classList.add('hidden');
    logo.onerror = () => { logo.classList.add('hidden'); fallback.classList.remove('hidden'); };
  } else { logo.removeAttribute('src'); logo.classList.add('hidden'); fallback.classList.remove('hidden'); }
  $('#dynamic-favicon').href = branding.favicon || 'data:,';
  const hero = $('#hero');
  hero.classList.toggle('has-custom-image', Boolean(branding.hero));
  hero.style.backgroundImage = branding.hero ? `linear-gradient(90deg, rgba(22,25,21,.92), rgba(22,25,21,.3)), url("${branding.hero}")` : '';
  $$('.asset-control').forEach(control => {
    const url = branding[control.dataset.brandingKind];
    const image = $('img', control), placeholder = $('.asset-preview span', control), remove = $('[data-branding-remove]', control);
    if (url) { image.src = url; image.hidden = false; placeholder.hidden = true; }
    else { image.removeAttribute('src'); image.hidden = true; placeholder.hidden = false; }
    remove.disabled = !url;
  });
}

function renderProviders() {
  $('#provider-list').innerHTML = state.providers.map(provider => `<div class="provider"><div><strong>${escapeHtml(provider.name)}</strong><small>${escapeHtml(provider.kind)} · ${escapeHtml(provider.base_url)}</small></div>${provider.builtin ? '<small>NATIVA</small>' : `<button class="button danger small" data-provider-delete="${provider.id}">Remover</button>`}</div>`).join('');
  $$('[data-provider-delete]').forEach(button => button.onclick = async () => { try { await api(`/metadata/providers/${button.dataset.providerDelete}`, { method: 'DELETE' }); await loadSettings(); } catch (error) { toast(error.message, true); } });
}

async function uploadBranding(control, file) {
  if (!file) return;
  const kind = control.dataset.brandingKind, form = new FormData();
  form.append('image', file);
  try {
    await api(`/settings/branding/${kind}`, { method: 'POST', body: form });
    await loadSettings();
    toast('Identidade visual atualizada.');
  } catch (error) { toast(error.message, true); }
  $('input[type=file]', control).value = '';
}

async function removeBranding(control) {
  const kind = control.dataset.brandingKind;
  if (!confirm('Excluir esta imagem personalizada?')) return;
  try {
    await api(`/settings/branding/${kind}`, { method: 'DELETE' });
    await loadSettings();
    toast('Imagem personalizada excluída.');
  } catch (error) { toast(error.message, true); }
}

async function scanLibrary() {
  try {
    const status = await api('/scan', { method: 'POST' });
    state.scanObservedRunning = true;
    renderScanStatus(status);
    scheduleScanPoll();
  } catch (error) { toast(error.message, true); }
}

function renderScanStatus(status) {
  const panel = $('#scan-progress'), button = $('#scan-button');
  const labels = {
    restoring: 'Restaurando backup…',
    discovering: 'Localizando arquivos compatíveis…', indexing: 'Indexando sua biblioteca',
    synchronizing: 'Organizando a estante…', covers: 'Gerando capas locais…', completed: status.profile === 'low_priority' ? 'Capas concluídas' : 'Varredura concluída', failed: 'A tarefa encontrou um problema',
  };
  if (status.phase === 'idle') { panel.classList.add('hidden'); button.disabled = false; return; }
  panel.classList.remove('hidden');
  const percent = Math.max(0, Math.min(100, Number(status.percent || 0)));
  $('#scan-progress-title').textContent = labels[status.phase] || 'Varredura em andamento';
  $('#scan-progress-percent').textContent = `${Math.round(percent)}%`;
  $('#scan-progress-bar').style.width = `${percent}%`;
  const profileNames = { economical: 'econômico', balanced: 'equilibrado', complete: 'completo' };
  $('#scan-progress-detail').textContent = status.phase === 'discovering'
    ? `Perfil ${profileNames[status.profile] || status.profile} · contando os livros`
    : status.phase === 'failed'
      ? (status.error || 'Não foi possível concluir')
      : status.phase === 'covers'
        ? `${status.processed} de ${status.total} capas · prioridade baixa${status.current_file ? ` · ${status.current_file}` : ''}`
        : `${status.processed} de ${status.total} arquivos · ${status.found} livros indexados${status.unchanged ? ` · ${status.unchanged} sem releitura` : ''}${status.current_file ? ` · ${status.current_file}` : ''}`;
  button.disabled = status.running;
  button.innerHTML = status.running ? `<span aria-hidden="true">↻</span><span class="desktop-label">${Math.round(percent)}%</span>` : '<span aria-hidden="true">↻</span><span class="desktop-label">Atualizar</span>';
}

function scheduleScanPoll() {
  clearTimeout(state.scanTimer);
  state.scanTimer = setTimeout(pollScanStatus, 700);
}

async function pollScanStatus() {
  try {
    const status = await api('/scan');
    renderScanStatus(status);
    if (status.running) {
      state.scanObservedRunning = true;
      scheduleScanPoll();
      return;
    }
    if (state.scanObservedRunning) {
      state.scanObservedRunning = false;
      if (status.phase === 'completed') {
        if (status.profile === 'low_priority') toast(`${status.processed} capa(s) processada(s) em prioridade baixa.`);
        else toast(`${status.found} livros: ${status.added} novos, ${status.updated} atualizados, ${status.unchanged} sem releitura e ${status.skipped} ignorados.`);
        await Promise.all([loadBooks(), loadReadingDesk(), loadSuggestions(), loadMaintenance()]);
      } else if (status.phase === 'failed') toast(status.error || 'A varredura falhou.', true);
      setTimeout(() => { if (!$('#scan-button').disabled) $('#scan-progress').classList.add('hidden'); }, 4500);
    }
  } catch (error) { toast(`Não foi possível acompanhar a varredura: ${error.message}`, true); }
}

async function upload(file) {
  if (!file) return;
  const form = new FormData(); form.append('file', file);
  toast(`Enviando ${file.name}…`);
  try { await api('/upload', { method: 'POST', body: form }); await scanLibrary(); toast('Livro adicionado à pasta da biblioteca.'); }
  catch (error) { toast(error.message, true); }
  $('#upload-input').value = '';
}

function encodePath(path) { return path.split('/').map(encodeURIComponent).join('/'); }

document.addEventListener('DOMContentLoaded', async () => {
  await Promise.all([loadSettings(), loadBooks(), loadReadingDesk()]);
  retryPendingProgress();
  window.addEventListener('online', retryPendingProgress);
  setInterval(retryPendingProgress, 15000);
  pollScanStatus();
  $('#search').addEventListener('input', event => { clearTimeout(state.searchTimer); state.searchTimer = setTimeout(() => loadBooks(event.target.value), 220); });
  document.addEventListener('keydown', event => {
    if (event.key === '/' && !['INPUT','TEXTAREA'].includes(document.activeElement.tagName)) {
      event.preventDefault();
      $('#search').focus();
    }
    if (event.key === 'Escape' && !$('#reader').classList.contains('hidden')) {
      closeReader();
    }
    if (!['INPUT','TEXTAREA','SELECT'].includes(document.activeElement?.tagName) && !$('#reader').classList.contains('hidden')) {
      const audio = $('#media-audio-element');
      const video = $('#media-video-element');
      if (event.key === ' ' || event.code === 'Space') {
        if (audio || video) {
          event.preventDefault();
          const media = audio || video;
          if (media.paused) media.play();
          else media.pause();
        }
      } else if (event.key === 'ArrowLeft') {
        event.preventDefault();
        if (audio) audio.currentTime = Math.max(0, audio.currentTime - 15);
        else if (video) video.currentTime = Math.max(0, video.currentTime - 10);
        else if (state.reader.kind === 'pdf') changePdfPage(-1);
        else if (state.reader.kind === 'epub') renderPagedResource('epub', Number(state.reader.location.chapter || 0) - 1);
        else if (state.reader.kind === 'cbz') renderPagedResource('comic', Number(state.reader.location.page_index || 0) - 1);
      } else if (event.key === 'ArrowRight') {
        event.preventDefault();
        if (audio) audio.currentTime = Math.min(audio.duration || Infinity, audio.currentTime + 30);
        else if (video) video.currentTime = Math.min(video.duration || Infinity, video.currentTime + 10);
        else if (state.reader.kind === 'pdf') changePdfPage(1);
        else if (state.reader.kind === 'epub') renderPagedResource('epub', Number(state.reader.location.chapter || 0) + 1);
        else if (state.reader.kind === 'cbz') renderPagedResource('comic', Number(state.reader.location.page_index || 0) + 1);
      } else if (event.key === 'f' || event.key === 'F') {
        if (video) {
          event.preventDefault();
          toggleFullscreen($('#reader-stage'));
        }
      } else if (event.key === 'm' || event.key === 'M') {
        const media = audio || video;
        if (media) {
          event.preventDefault();
          media.muted = !media.muted;
        }
      }
    }
  });
  $('#book-grid').addEventListener('click', event => { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#book-grid').addEventListener('keydown', event => { if (['Enter',' '].includes(event.key)) { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); } });
  $('#reading-desk-list').addEventListener('click', event => { const remove = event.target.closest('[data-desk-remove]'); if (remove) { event.stopPropagation(); removeFromDesk(Number(remove.dataset.deskRemove)); return; } const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#reading-desk-list').addEventListener('keydown', event => { if (['Enter',' '].includes(event.key) && !event.target.closest('button')) { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); } });
  $('#suggestion-stage').addEventListener('click', event => { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#suggestion-prev').onclick = () => moveSuggestion(-1); $('#suggestion-next').onclick = () => moveSuggestion(1);
  [['#filter-format','format'], ['#filter-author','author'], ['#filter-subject','subject'], ['#filter-collection','collection_id'], ['#filter-publisher','publisher'], ['#filter-language','language'], ['#filter-year','year'], ['#filter-progress','progress'], ['#filter-availability','availability']].forEach(([selector, key]) => { $(selector).onchange = event => { state.filters[key] = event.target.value; loadBooks(); }; });
  $('#filter-sort').onchange = event => { state.filters.sort = event.target.value === 'title' ? '' : event.target.value; loadBooks(); };
  $('#filter-search-mode').onchange = event => { state.filters.content_only = event.target.value === 'content' ? 'true' : ''; loadBooks(); };
  [['#filter-min-size','min_size'], ['#filter-max-size','max_size']].forEach(([selector, key]) => { $(selector).onchange = event => { state.filters[key] = event.target.value ? String(Math.round(Number(event.target.value) * 1024 * 1024)) : ''; loadBooks(); }; });
  $('#clear-filters').onclick = () => { state.filters = { q: '', format: '', media_type: state.filters.media_type, author: '', subject: '', publisher: '', language: '', year: '', min_size: '', max_size: '', progress: '', availability: '', collection_id: '', sort: '', content_only: '' }; $('#search').value = ''; loadBooks(''); };
  $$('.media-tab-btn').forEach(btn => {
    btn.onclick = () => {
      $$('.media-tab-btn').forEach(b => {
        b.classList.remove('active');
        b.setAttribute('aria-selected', 'false');
      });
      btn.classList.add('active');
      btn.setAttribute('aria-selected', 'true');
      state.filters.media_type = btn.dataset.mediaType || 'book';
      state.filters.format = '';
      $('#filter-format').value = '';
      state.catalog.offset = 0;
      loadBooks(state.filters.q, 0);
    };
  });
  $$('.view-switch button').forEach(button => button.onclick = () => { $$('.view-switch button').forEach(item => item.classList.remove('active')); button.classList.add('active'); $('#book-grid').classList.toggle('list', button.dataset.view === 'list'); });
  $('#catalog-previous').onclick = () => loadBooks(state.filters.q, Math.max(0, state.catalog.offset - state.catalog.limit));
  $('#catalog-next').onclick = () => loadBooks(state.filters.q, state.catalog.offset + state.catalog.limit);
  $('#scan-button').onclick = scanLibrary; $('#empty-scan').onclick = scanLibrary;
  $('#upload-button').onclick = $('#empty-upload').onclick = () => $('#upload-input').click(); $('#upload-input').onchange = event => upload(event.target.files[0]);
  $('#settings-button').onclick = async () => { $('#settings-dialog').showModal(); await Promise.all([loadSettings(), loadMaintenance()]); };
  $$('[data-close]').forEach(button => button.onclick = () => document.getElementById(button.dataset.close).close());
  $('#settings-form').onsubmit = async event => { event.preventDefault(); const form = event.currentTarget; try { await api('/settings', { method: 'PUT', body: JSON.stringify({ library_root: form.elements.library_root.value, network_metadata_enabled: form.elements.network_metadata_enabled.checked, scan_profile: form.elements.scan_profile.value, auto_cover_enabled: form.elements.auto_cover_enabled.checked, carousel_enabled: form.elements.carousel_enabled.checked, carousel_interval_seconds: Number(form.elements.carousel_interval_seconds.value) }) }); await loadSettings(); toast('Configurações salvas.'); } catch (error) { toast(error.message, true); } };
  $$('.asset-control').forEach(control => { $('input[type=file]', control).onchange = event => uploadBranding(control, event.target.files[0]); $('[data-branding-remove]', control).onclick = () => removeBranding(control); });
  $('#show-provider-form').onclick = () => $('#provider-form').classList.toggle('hidden');
  $('#provider-form').onsubmit = async event => { event.preventDefault(); const form = event.currentTarget; const payload = Object.fromEntries(new FormData(form)); payload.enabled = true; try { await api('/metadata/providers', { method: 'POST', body: JSON.stringify(payload) }); form.reset(); form.classList.add('hidden'); await loadSettings(); toast('Fonte adicionada.'); } catch (error) { toast(error.message, true); } };
  $('#collection-form').onsubmit = async event => { event.preventDefault(); const form = event.currentTarget; const payload = Object.fromEntries(new FormData(form)); try { await api('/collections', { method: 'POST', body: JSON.stringify(payload) }); form.reset(); form.elements.color.value = '#6e816a'; await Promise.all([loadSettings(), loadBooks()]); toast('Coleção criada.'); } catch (error) { toast(error.message, true); } };
  $('#refresh-health').onclick = loadMaintenance; $('#create-backup').onclick = createBackup;
  $('#restore-input').onchange = event => restoreBackup(event.target.files[0]);
  $('#preview-orphan-covers').onclick = previewCoverCleanup;
  $('#reindex-fts-btn').onclick = reindexFts;
  $('#generate-covers').onclick = generateCovers; $('#opds-import-form').onsubmit = importOpds;
  $('#reader-close').onclick = closeReader; $('#notes-toggle').onclick = () => $('#notes-panel').classList.toggle('open'); $('#notes-close').onclick = () => $('#notes-panel').classList.remove('open');
  $('#notes-filter')?.addEventListener('input', event => {
    state.notesFilter = event.target.value;
    renderNotesList();
  });
  $$('[data-filter-color]').forEach(button => {
    button.onclick = () => {
      $$('[data-filter-color]').forEach(b => b.classList.remove('active'));
      button.classList.add('active');
      state.notesColorFilter = button.dataset.filterColor;
      renderNotesList();
    };
  });
  $('#note-form').onsubmit = async event => {
    event.preventDefault();
    const form = event.currentTarget;
    const submitBtn = $('button[type="submit"]', form);
    if (submitBtn) submitBtn.disabled = true;
    const data = new FormData(form);
    const content = data.get('content');
    const quote = (data.get('quote') || '').trim();
    const color = (data.get('color') || '').trim();
    const location = { ...(state.reader.location || {}) };
    if (quote) location.quote = quote;
    if (color) location.color = color;
    try {
      await api(`/books/${state.currentBook.id}/notes`, {
        method: 'POST',
        body: JSON.stringify({ content, location })
      });
      form.reset();
      if (form.elements.color && form.elements.color[0]) form.elements.color[0].checked = true;
      await loadNotes();
      toast('Anotação salva nesta posição.');
    } catch (error) { toast(error.message, true); }
    finally {
      if (submitBtn) submitBtn.disabled = false;
    }
  };
  window.addEventListener('pagehide', flushProgressOnPageExit);
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') flushProgressOnPageExit(); else retryPendingProgress(); });
});
