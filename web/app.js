const state = {
  books: [], settings: null, providers: [], currentBook: null,
  reader: { kind: null, location: {}, percent: 0, items: [] }, searchTimer: null,
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

async function loadBooks(query = '') {
  try {
    const data = await api(`/books?q=${encodeURIComponent(query)}`);
    state.books = data.books;
    renderBooks();
  } catch (error) { toast(error.message, true); }
}

function renderBooks() {
  const grid = $('#book-grid');
  $('#library-summary').textContent = `${state.books.length} ${state.books.length === 1 ? 'livro encontrado' : 'livros encontrados'}`;
  $('#empty-state').classList.toggle('hidden', state.books.length !== 0);
  grid.classList.toggle('hidden', state.books.length === 0);
  grid.innerHTML = state.books.map(book => `
    <article class="book-card" data-book-id="${book.id}" tabindex="0" aria-label="Abrir ${escapeHtml(book.title)}">
      ${coverMarkup(book)}
      <div class="book-info"><h3>${escapeHtml(book.title)}</h3><p>${escapeHtml(book.author || book.filename)}</p>
        <div class="progress-track"><i style="width:${Math.max(0, Math.min(100, book.progress_percent))}%"></i></div>
        <small class="progress-label">${book.progress_percent ? `${Math.round(book.progress_percent)}% lido` : 'Ainda não iniciado'}</small>
      </div>
    </article>`).join('');
}

async function openBook(id) {
  try {
    const [{ book }, providerData] = await Promise.all([api(`/books/${id}`), api('/metadata/providers')]);
    state.currentBook = book;
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
  $('#reader-title').textContent = book.title;
  $('#reader-download').href = `/api/books/${book.id}/file?download=true`;
  $('#reader').classList.remove('hidden'); document.body.style.overflow = 'hidden';
  await renderReader(); loadNotes();
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
  updateReaderLabel(); persistProgress();
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
  updateReaderLabel(); persistProgress();
}

function renderPercentNavigation() {
  const value = Math.round(state.reader.percent || 0);
  $('#reader-navigation').innerHTML = `<label>Progresso <input id="percent-input" type="range" min="0" max="100" value="${value}"> <span id="percent-value">${value}%</span></label>`;
  $('#percent-input').oninput = event => { state.reader.percent = Number(event.target.value); state.reader.location = { type: 'percent', percent: state.reader.percent }; $('#percent-value').textContent = `${state.reader.percent}%`; updateReaderLabel(); };
  $('#percent-input').onchange = persistProgress;
  updateReaderLabel();
}

function updateReaderLabel() {
  const location = state.reader.location;
  $('#reader-location').textContent = location.page ? `Página ${location.page} · ${Math.round(state.reader.percent)}%` : location.type === 'chapter' ? `Capítulo ${location.chapter + 1} · ${Math.round(state.reader.percent)}%` : `${Math.round(state.reader.percent)}% lido`;
}

async function persistProgress() {
  try { await api(`/books/${state.currentBook.id}/progress`, { method: 'PUT', body: JSON.stringify({ location: state.reader.location, percent: state.reader.percent }) }); }
  catch (error) { toast(`Progresso não salvo: ${error.message}`, true); }
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

function closeReader() {
  persistProgress(); $('#reader').classList.add('hidden'); $('#notes-panel').classList.remove('open'); document.body.style.overflow = ''; loadBooks($('#search').value);
}

async function loadSettings() {
  try {
    state.settings = await api('/settings');
    const form = $('#settings-form'); form.elements.library_root.value = state.settings.library_root;
    form.elements.network_metadata_enabled.checked = state.settings.network_metadata_enabled;
    $('#network-status').textContent = state.settings.network_metadata_enabled ? 'Consultas externas somente sob seu comando' : 'Nenhuma consulta externa permitida';
    const data = await api('/metadata/providers'); state.providers = data.providers; renderProviders();
  } catch (error) { toast(error.message, true); }
}

function renderProviders() {
  $('#provider-list').innerHTML = state.providers.map(provider => `<div class="provider"><div><strong>${escapeHtml(provider.name)}</strong><small>${escapeHtml(provider.kind)} · ${escapeHtml(provider.base_url)}</small></div>${provider.builtin ? '<small>NATIVA</small>' : `<button class="button danger small" data-provider-delete="${provider.id}">Remover</button>`}</div>`).join('');
  $$('[data-provider-delete]').forEach(button => button.onclick = async () => { try { await api(`/metadata/providers/${button.dataset.providerDelete}`, { method: 'DELETE' }); await loadSettings(); } catch (error) { toast(error.message, true); } });
}

async function scanLibrary(button = $('#scan-button')) {
  const previous = button.innerHTML; button.disabled = true; button.textContent = 'Varrendo…';
  try { const report = await api('/scan', { method: 'POST' }); toast(`${report.found} livros encontrados; ${report.added} novos.`); await loadBooks($('#search').value); }
  catch (error) { toast(error.message, true); }
  finally { button.disabled = false; button.innerHTML = previous; }
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
  await Promise.all([loadSettings(), loadBooks()]);
  $('#search').addEventListener('input', event => { clearTimeout(state.searchTimer); state.searchTimer = setTimeout(() => loadBooks(event.target.value), 220); });
  document.addEventListener('keydown', event => { if (event.key === '/' && !['INPUT','TEXTAREA'].includes(document.activeElement.tagName)) { event.preventDefault(); $('#search').focus(); } if (event.key === 'Escape' && !$('#reader').classList.contains('hidden')) closeReader(); });
  $('#book-grid').addEventListener('click', event => { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); });
  $('#book-grid').addEventListener('keydown', event => { if (['Enter',' '].includes(event.key)) { const card = event.target.closest('[data-book-id]'); if (card) openBook(Number(card.dataset.bookId)); } });
  $$('.view-switch button').forEach(button => button.onclick = () => { $$('.view-switch button').forEach(item => item.classList.remove('active')); button.classList.add('active'); $('#book-grid').classList.toggle('list', button.dataset.view === 'list'); });
  $('#scan-button').onclick = () => scanLibrary(); $('#empty-scan').onclick = event => scanLibrary(event.currentTarget);
  $('#upload-button').onclick = $('#empty-upload').onclick = () => $('#upload-input').click(); $('#upload-input').onchange = event => upload(event.target.files[0]);
  $('#settings-button').onclick = async () => { await loadSettings(); $('#settings-dialog').showModal(); };
  $$('[data-close]').forEach(button => button.onclick = () => document.getElementById(button.dataset.close).close());
  $('#settings-form').onsubmit = async event => { event.preventDefault(); const form = event.currentTarget; try { await api('/settings', { method: 'PUT', body: JSON.stringify({ library_root: form.elements.library_root.value, network_metadata_enabled: form.elements.network_metadata_enabled.checked }) }); await loadSettings(); toast('Configurações salvas. Faça uma varredura para atualizar a estante.'); } catch (error) { toast(error.message, true); } };
  $('#show-provider-form').onclick = () => $('#provider-form').classList.toggle('hidden');
  $('#provider-form').onsubmit = async event => { event.preventDefault(); const payload = Object.fromEntries(new FormData(event.currentTarget)); payload.enabled = true; try { await api('/metadata/providers', { method: 'POST', body: JSON.stringify(payload) }); event.currentTarget.reset(); event.currentTarget.classList.add('hidden'); await loadSettings(); toast('Fonte adicionada.'); } catch (error) { toast(error.message, true); } };
  $('#reader-close').onclick = closeReader; $('#notes-toggle').onclick = () => $('#notes-panel').classList.toggle('open'); $('#notes-close').onclick = () => $('#notes-panel').classList.remove('open');
  $('#note-form').onsubmit = async event => { event.preventDefault(); const content = new FormData(event.currentTarget).get('content'); try { await api(`/books/${state.currentBook.id}/notes`, { method: 'POST', body: JSON.stringify({ content, location: state.reader.location }) }); event.currentTarget.reset(); loadNotes(); toast('Anotação salva nesta posição.'); } catch (error) { toast(error.message, true); } };
});
