const state = {
  books: [], facets: { formats: [], authors: [], subjects: [] }, desk: [], suggestions: [],
  settings: null, providers: [], collections: [], bookCollectionIds: [], currentBook: null,
  filters: { q: '', format: '', author: '', subject: '', publisher: '', language: '', year: '', min_size: '', max_size: '', progress: '', availability: '', collection_id: '', sort: '' },
  reader: { kind: null, location: {}, percent: 0, items: [] },
  searchTimer: null, scanTimer: null, scanObservedRunning: false,
  carouselIndex: 0, carouselTimer: null,
  progressSaveTimer: null, progressSaveChain: Promise.resolve(),
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

async function loadBooks(query = state.filters.q) {
  state.filters.q = query || '';
  try {
    const params = new URLSearchParams();
    Object.entries(state.filters).forEach(([key, value]) => { if (value) params.set(key === 'q' ? 'q' : key, value); });
    const data = await api(`/books?${params}`);
    state.books = data.books;
    state.facets = data.facets;
    renderFilters();
    renderBooks();
  } catch (error) { toast(error.message, true); }
}

function renderBooks() {
  const grid = $('#book-grid');
  $('#library-summary').textContent = `${state.books.length} ${state.books.length === 1 ? 'livro encontrado' : 'livros encontrados'}`;
  $('#empty-state').classList.toggle('hidden', state.books.length !== 0);
  grid.classList.toggle('hidden', state.books.length === 0);
  const filtered = Object.values(state.filters).some(Boolean);
  $('#empty-message').textContent = filtered ? 'Nenhum livro corresponde aos filtros escolhidos.' : 'Adicione um livro ou faça uma varredura da pasta configurada.';
  grid.innerHTML = state.books.map(book => `
    <article class="book-card" data-book-id="${book.id}" tabindex="0" aria-label="Abrir ${escapeHtml(book.title)}">
      ${coverMarkup(book)}
      <div class="book-info"><h3>${escapeHtml(book.title)}</h3><p>${escapeHtml(book.author || book.filename)}</p>
        ${book.subjects?.length ? `<small class="subject-label">${escapeHtml(book.subjects.slice(0, 2).join(' · '))}</small>` : ''}
        <div class="progress-track"><i style="width:${Math.max(0, Math.min(100, book.progress_percent))}%"></i></div>
        <small class="progress-label">${book.progress_percent ? `${Math.round(book.progress_percent)}% lido` : 'Ainda não iniciado'}</small>
      </div>
    </article>`).join('');
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
  $('#clear-filters').classList.toggle('hidden', !Object.values(state.filters).some(Boolean));
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
  $('#reading-desk-list').innerHTML = state.desk.map(book => `
    <article class="desk-book" data-book-id="${book.id}" tabindex="0">
      ${coverMarkup(book)}
      <div class="desk-book-copy"><small>${Math.round(book.progress_percent)}% LIDO</small><h3>${escapeHtml(book.title)}</h3><p>${escapeHtml(book.author || book.filename)}</p>
        <div class="progress-track"><i style="width:${book.progress_percent}%"></i></div>
      </div>
      <button class="desk-remove" type="button" data-desk-remove="${book.id}" aria-label="Retirar ${escapeHtml(book.title)} da mesa">×</button>
    </article>`).join('');
}

async function removeFromDesk(id) {
  try {
    await api(`/books/${id}/progress`, { method: 'DELETE' });
    toast('Livro retirado da mesa de leitura.');
    await Promise.all([loadReadingDesk(), loadBooks()]);
  } catch (error) { toast(error.message, true); }
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
  const providers = state.providers.filter(item => item.enabled).map(item => `<option value="${item.id}">${escapeHtml(item.name)}</option>`).join('');
  return `<div class="book-detail-layout">
    <div class="detail-cover-area">${coverMarkup(book, true)}</div>
    <div class="detail-body">
      <p class="eyebrow">${escapeHtml(book.format.toUpperCase())} · ${formatBytes(book.size)}</p>
      <h2>${escapeHtml(book.title)}</h2><p class="detail-author">${escapeHtml(book.author || 'Autor não informado')}</p>
      <p class="detail-description">${escapeHtml(book.description || 'Sem sinopse. Você pode editar os dados manualmente ou buscar metadados em uma fonte habilitada.')}</p>
      <div class="detail-actions">
        <button class="button primary" id="read-book" ${canRead && book.is_available ? '' : 'disabled'}>▶ ${book.progress?.percent ? 'Continuar leitura' : 'Começar a ler'}</button>
        <a class="button ghost" href="/api/books/${book.id}/file?download=true">⇩ Baixar</a>
        <button class="button ghost" id="share-book">↗ Compartilhar</button>
        <button class="button ghost" id="edit-book">✎ Editar</button>
      </div>
      <div class="metadata-grid">
        <div><span>Arquivo</span>${escapeHtml(book.filename)}</div><div><span>Progresso</span>${Math.round(book.progress?.percent || 0)}%</div>
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
  state.currentBook = book;
  state.reader = { kind: book.format, location: book.progress?.location || {}, percent: book.progress?.percent || 0, items: [] };
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
    } else throw new Error(`Leitura de ${book.format} ainda não disponível.`);
  } catch (error) { stage.innerHTML = `<div class="reader-error"><h2>Não foi possível abrir este livro</h2><p>${escapeHtml(error.message)}</p><a class="button light" href="/api/books/${book.id}/file?download=true">Baixar arquivo</a></div>`; }
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
  $('#reader-location').textContent = location.page ? `Página ${location.page} · ${Math.round(state.reader.percent)}%` : location.type === 'chapter' ? `Capítulo ${location.chapter + 1} · ${Math.round(state.reader.percent)}%` : `${Math.round(state.reader.percent)}% lido`;
}

function scheduleProgressSave(immediate = false) {
  if (!state.currentBook || $('#reader').classList.contains('hidden')) return;
  clearTimeout(state.progressSaveTimer);
  state.progressSaveTimer = setTimeout(persistProgress, immediate ? 0 : 700);
}

/** Captura livro/posicao agora e serializa as gravacoes, inclusive ao fechar. */
function persistProgress() {
  clearTimeout(state.progressSaveTimer);
  state.progressSaveTimer = null;
  if (!state.currentBook) return Promise.resolve();
  const bookId = state.currentBook.id;
  const body = JSON.stringify({ location: state.reader.location, percent: state.reader.percent });
  state.progressSaveChain = state.progressSaveChain.then(() =>
    api(`/books/${bookId}/progress`, { method: 'PUT', body, keepalive: true })
  ).catch(error => { toast(`Progresso não salvo: ${error.message}`, true); });
  return state.progressSaveChain;
}

function flushProgressOnPageExit() {
  if (!state.currentBook || $('#reader').classList.contains('hidden')) return;
  persistProgress();
}

async function loadNotes() {
  try {
    const { notes } = await api(`/books/${state.currentBook.id}/notes`);
    $('#notes-list').innerHTML = notes.length ? notes.map(note => `<article class="note"><p>${escapeHtml(note.content)}</p><footer><span>${locationLabel(note.location)} · ${new Date(note.updated_at).toLocaleDateString()}</span><button data-note-delete="${note.id}">Excluir</button></footer></article>`).join('') : '<p class="detail-description">As anotações feitas durante a leitura aparecerão aqui.</p>';
    $$('[data-note-delete]').forEach(button => button.onclick = async () => { await api(`/notes/${button.dataset.noteDelete}`, { method: 'DELETE' }); loadNotes(); });
  } catch (error) { toast(error.message, true); }
}

function locationLabel(location = {}) {
  if (location.type === 'chapter') return `Capítulo ${Number(location.chapter) + 1}`;
  if (location.type === 'page') return `Página ${location.page}`;
  return `${Math.round(location.percent || 0)}%`;
}

async function closeReader() {
  clearTimeout(state.progressSaveTimer);
  await persistProgress();
  $('#reader').classList.add('hidden'); $('#notes-panel').classList.remove('open'); document.body.style.overflow = '';
  await Promise.all([loadBooks($('#search').value), loadReadingDesk()]);
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
    const [{ health }, { groups }, { backups }] = await Promise.all([
      api('/maintenance/health'), api('/maintenance/duplicates'), api('/maintenance/backups'),
    ]);
    renderHealth(health);
    renderDuplicates(groups);
    renderBackups(backups);
  } catch (error) { toast(`Manutenção: ${error.message}`, true); }
}

function renderHealth(health) {
  const cards = [
    ['Livros no catálogo', health.total, 'total'],
    ['Disponíveis', health.available, 'good'],
    ['Arquivos ausentes', health.unavailable, health.unavailable ? 'warn' : 'good'],
    ['Sem autor', health.without_author, health.without_author ? 'warn' : 'good'],
    ['Sem assunto', health.without_subject, health.without_subject ? 'warn' : 'good'],
    ['Sem capa', health.without_cover, health.without_cover ? 'warn' : 'good'],
    ['Sem idioma', health.without_language, health.without_language ? 'warn' : 'good'],
    ['Grupos duplicados', health.duplicate_groups, health.duplicate_groups ? 'warn' : 'good'],
  ];
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
  const confirmation = prompt(`Esta operação apaga o arquivo original e não pode ser desfeita pelo aplicativo.\n\nPara confirmar, digite exatamente:\n${filename}`);
  if (confirmation !== filename) {
    if (confirmation !== null) toast('Nome diferente: exclusão cancelada.', true);
    return;
  }
  try {
    await api(`/books/${button.dataset.deleteBook}/file`, { method: 'DELETE', body: JSON.stringify({ filename }) });
    toast(`Arquivo ${filename} excluído. Não é recuperável pelo aplicativo.`);
    await Promise.all([loadMaintenance(), loadBooks()]);
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
  pollScanStatus();
  $('#search').addEventListener('input', event => { clearTimeout(state.searchTimer); state.searchTimer = setTimeout(() => loadBooks(event.target.value), 220); });
  document.addEventListener('keydown', event => { if (event.key === '/' && !['INPUT','TEXTAREA'].includes(document.activeElement.tagName)) { event.preventDefault(); $('#search').focus(); } if (event.key === 'Escape' && !$('#reader').classList.contains('hidden')) closeReader(); });
  $('#book-grid').addEventListener('click', event => { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#book-grid').addEventListener('keydown', event => { if (['Enter',' '].includes(event.key)) { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); } });
  $('#reading-desk-list').addEventListener('click', event => { const remove = event.target.closest('[data-desk-remove]'); if (remove) { event.stopPropagation(); removeFromDesk(Number(remove.dataset.deskRemove)); return; } const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#reading-desk-list').addEventListener('keydown', event => { if (['Enter',' '].includes(event.key) && !event.target.closest('button')) { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); } });
  $('#suggestion-stage').addEventListener('click', event => { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#suggestion-prev').onclick = () => moveSuggestion(-1); $('#suggestion-next').onclick = () => moveSuggestion(1);
  [['#filter-format','format'], ['#filter-author','author'], ['#filter-subject','subject'], ['#filter-collection','collection_id'], ['#filter-publisher','publisher'], ['#filter-language','language'], ['#filter-year','year'], ['#filter-progress','progress'], ['#filter-availability','availability']].forEach(([selector, key]) => { $(selector).onchange = event => { state.filters[key] = event.target.value; loadBooks(); }; });
  $('#filter-sort').onchange = event => { state.filters.sort = event.target.value === 'title' ? '' : event.target.value; loadBooks(); };
  [['#filter-min-size','min_size'], ['#filter-max-size','max_size']].forEach(([selector, key]) => { $(selector).onchange = event => { state.filters[key] = event.target.value ? String(Math.round(Number(event.target.value) * 1024 * 1024)) : ''; loadBooks(); }; });
  $('#clear-filters').onclick = () => { state.filters = { q: '', format: '', author: '', subject: '', publisher: '', language: '', year: '', min_size: '', max_size: '', progress: '', availability: '', collection_id: '', sort: '' }; $('#search').value = ''; loadBooks(''); };
  $$('.view-switch button').forEach(button => button.onclick = () => { $$('.view-switch button').forEach(item => item.classList.remove('active')); button.classList.add('active'); $('#book-grid').classList.toggle('list', button.dataset.view === 'list'); });
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
  $('#generate-covers').onclick = generateCovers; $('#opds-import-form').onsubmit = importOpds;
  $('#reader-close').onclick = closeReader; $('#notes-toggle').onclick = () => $('#notes-panel').classList.toggle('open'); $('#notes-close').onclick = () => $('#notes-panel').classList.remove('open');
  $('#note-form').onsubmit = async event => { event.preventDefault(); const form = event.currentTarget; const content = new FormData(form).get('content'); try { await api(`/books/${state.currentBook.id}/notes`, { method: 'POST', body: JSON.stringify({ content, location: state.reader.location }) }); form.reset(); loadNotes(); toast('Anotação salva nesta posição.'); } catch (error) { toast(error.message, true); } };
  window.addEventListener('pagehide', flushProgressOnPageExit);
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') flushProgressOnPageExit(); });
});
