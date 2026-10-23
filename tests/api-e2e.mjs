import assert from 'node:assert/strict';
import { writeFile, utimes, access } from 'node:fs/promises';

const baseUrl = process.env.ESTANTE_E2E_URL || 'http://127.0.0.1:20001/api';

async function request(path, options = {}) {
  const headers = { ...(options.headers || {}) };
  if (options.body && !(options.body instanceof FormData)) headers['Content-Type'] = 'application/json';
  const response = await fetch(`${baseUrl}${path}`, { ...options, headers });
  if (!response.ok) throw new Error(`${options.method || 'GET'} ${path}: ${response.status} ${await response.text()}`);
  if (response.status === 204) return null;
  return response.json();
}

async function waitForTask() {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const status = await request('/scan');
    if (!status.running) return status;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw new Error('tempo excedido aguardando tarefa de fundo');
}

assert.equal((await request('/health')).status, 'ok');
const settings = await request('/settings');
assert.ok(
  settings.library_root.endsWith('/pdf') || settings.library_root.endsWith('tests/fixtures/e2e') || settings.library_root.endsWith('tests/runtime-data-api-ci/library'),
  `raiz de biblioteca inesperada: ${settings.library_root}`,
);
assert.equal(settings.auth_enabled, false);

const crossOrigin = await fetch(`${baseUrl}/scan`, {
  method: 'POST', headers: { Origin: 'https://pagina-maliciosa.example' },
});
assert.equal(crossOrigin.status, 403);

// Keep this multipart request fully buffered. When FormData streams the body,
// Node 22 may surface EPIPE if the server rejects the filename before the upload
// has finished, hiding the HTTP 400 response that this test needs to verify.
const unsupportedBoundary = '----EstanteLivreUnsupportedUpload';
const unsupportedBody = Buffer.from([
  `--${unsupportedBoundary}`,
  'Content-Disposition: form-data; name="file"; filename="nao-e-livro.exe"',
  'Content-Type: application/octet-stream',
  '',
  'executavel de teste',
  `--${unsupportedBoundary}--`,
  '',
].join('\r\n'));
const unsupportedResponse = await fetch(`${baseUrl}/upload`, {
  method: 'POST',
  headers: {
    'Content-Type': `multipart/form-data; boundary=${unsupportedBoundary}`,
    'Content-Length': String(unsupportedBody.byteLength),
  },
  body: unsupportedBody,
});
assert.equal(unsupportedResponse.status, 400);

await request('/scan', { method: 'POST' });
const firstScan = await waitForTask();
assert.equal(firstScan.phase, 'completed');
assert.equal(firstScan.found, 3);
assert.equal(firstScan.added, 3);

const catalog = await request('/books?q=Sertao&availability=available&sort=title');
assert.equal(catalog.books.length, 1);
assert.ok(catalog.facets.formats.includes('txt'));

// Paginas limitadas, totais filtrados e desempate estavel.
const firstPage = await request('/books?limit=1');
const secondPage = await request('/books?limit=1&offset=1');
assert.equal(firstPage.total, 3);
assert.equal(firstPage.limit, 1);
assert.equal(firstPage.offset, 0);
assert.equal(firstPage.books.length, 1);
assert.notEqual(firstPage.books[0].id, secondPage.books[0].id);
assert.equal((await request('/books?limit=1&offset=3')).books.length, 0);
assert.equal((await request('/books?q=Sertao&limit=1')).total, 1);
for (const query of ['limit=0', 'limit=201', 'offset=-1']) {
  assert.equal((await fetch(`${baseUrl}/books?${query}`)).status, 400);
}

// Busca textual completa via SQLite FTS5 e manutencao de indice
const ftsInitial = await request('/maintenance/fts');
assert.equal(ftsInitial.total_books, 3);
assert.ok(typeof ftsInitial.indexed_books === 'number');
const ftsReindex = await request('/maintenance/fts', { method: 'POST' });
assert.ok(typeof ftsReindex.processed === 'number');
assert.equal(ftsReindex.total_books, 3);
const ftsStatus = await request('/maintenance/fts');
assert.equal(ftsStatus.total_books, 3);
assert.equal(ftsStatus.indexed_books, 3);
assert.equal(ftsStatus.percent, 100);

const ftsSearch = await request('/search?q=catalogacao');
assert.ok(ftsSearch.results.length >= 1);
assert.ok(ftsSearch.results.some(r => r.snippet && r.snippet.includes('catalogacao')));

const contentCatalog = await request('/books?q=catalogacao&content_only=true');
assert.ok(contentCatalog.books.length >= 1);

const emptySearch = await request('/search?q=');
assert.equal(emptySearch.total, 0);
assert.equal(emptySearch.results.length, 0);
const missingSearch = await fetch(`${baseUrl}/search`);
assert.equal(missingSearch.status, 400);


// Posicoes aceitas podem ser restauradas; HTML e tipos invalidos sao recusados.
const bookId = catalog.books[0].id;
for (const location of [[], null, { type: 'page', page: '<b>1</b>' }, { page: 1.5 }, { percent: 101 }]) {
  for (const [endpoint, method, payload] of [
    ['notes', 'POST', { content: 'Nota sintetica', location }],
    ['progress', 'PUT', { percent: 37, location }],
  ]) {
    const response = await fetch(`${baseUrl}/books/${bookId}/${endpoint}`, {
      method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload),
    });
    assert.equal(response.status, 400);
  }
}
const { note } = await request(`/books/${bookId}/notes`, {
  method: 'POST', body: JSON.stringify({ content: 'Nota sem posicao explicita' }),
});
assert.deepEqual(note.location, {});

// Criacao de nota com citacao destacada e cor de marca-texto
const highlightNote = await request(`/books/${bookId}/notes`, {
  method: 'POST',
  body: JSON.stringify({
    content: 'Minha analise do trecho',
    location: { type: 'chapter', chapter: 0, quote: 'Trecho citado da obra', color: 'yellow' },
  }),
});
assert.equal(highlightNote.note.content, 'Minha analise do trecho');
assert.equal(highlightNote.note.location.quote, 'Trecho citado da obra');
assert.equal(highlightNote.note.location.color, 'yellow');

// Busca e listagem geral de notas
const allNotes = await request('/notes');
assert.ok(allNotes.notes.length >= 2);
assert.ok(allNotes.notes.some(n => n.book_id === bookId));

const filteredNotes = await request(`/notes?q=analise&color=yellow`);
assert.equal(filteredNotes.notes.length, 1);
assert.equal(filteredNotes.notes[0].location.color, 'yellow');

// Exportacao de notas em Markdown e JSON
const mdExportRes = await fetch(`${baseUrl}/books/${bookId}/notes/export?format=markdown`);
assert.equal(mdExportRes.status, 200);
assert.ok(mdExportRes.headers.get('content-type').includes('text/markdown'));
const mdText = await mdExportRes.text();
assert.ok(mdText.includes('Minha analise do trecho'));
assert.ok(mdText.includes('Trecho citado da obra'));

const jsonExportRes = await fetch(`${baseUrl}/books/${bookId}/notes/export?format=json`);
assert.equal(jsonExportRes.status, 200);
assert.ok(jsonExportRes.headers.get('content-type').includes('application/json'));
const jsonData = await jsonExportRes.json();
assert.ok(jsonData.total_notes >= 2);

const libMdExport = await fetch(`${baseUrl}/notes/export?format=markdown`);
assert.equal(libMdExport.status, 200);
assert.ok((await libMdExport.text()).includes('Anotações da Biblioteca'));

// Falhas antes da gravacao devem preservar todos os metadados.
const original = (await request(`/books/${bookId}`)).book;
const candidate = { provider: 'synthetic', title: 'Nao deve substituir o titulo', authors: [], subjects: [], cover_url: 'http://127.0.0.1/cover.jpg' };
async function applyExpectingFailure(status) {
  const response = await fetch(`${baseUrl}/books/${bookId}/metadata`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(candidate),
  });
  assert.equal(response.status, status);
  assert.deepEqual((await request(`/books/${bookId}`)).book, original);
}
await applyExpectingFailure(403);
await request('/settings', { method: 'PUT', body: JSON.stringify({ ...settings, network_metadata_enabled: true }) });
await applyExpectingFailure(400);
await request('/settings', { method: 'PUT', body: JSON.stringify(settings) });

const progressPayload = { location: { type: 'percent', percent: 37 }, percent: 37 };
await request(`/books/${catalog.books[0].id}/progress`, {
  method: 'PUT', body: JSON.stringify(progressPayload),
});
const savedProgress = (await request(`/books/${catalog.books[0].id}`)).book.progress;
assert.deepEqual(savedProgress.location, progressPayload.location);
assert.equal(savedProgress.percent, 37);
const invalidProgress = await fetch(`${baseUrl}/books/${catalog.books[0].id}/progress`, {
  method: 'PUT', headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ location: 'nao-e-objeto', percent: 37 }),
});
assert.equal(invalidProgress.status, 400);

const collectionName = `Validacao E2E ${Date.now()}`;
const { collection } = await request('/collections', {
  method: 'POST', body: JSON.stringify({ name: collectionName, color: '#52758f' }),
});
await request(`/collections/${collection.id}/books/${catalog.books[0].id}`, { method: 'PUT' });
const collected = await request(`/books?collection_id=${collection.id}`);
assert.equal(collected.books.length, 1);

const { health } = await request('/maintenance/health');
assert.equal(health.total, 3);
assert.equal(health.duplicate_groups, 1);
const { groups } = await request('/maintenance/duplicates');
assert.equal(groups.length, 1);
assert.equal(groups[0].books.length, 2);
const protectedBook = groups[0].books[0];
const rejectedDeletion = await fetch(`${baseUrl}/books/${protectedBook.id}/file`, {
  method: 'DELETE',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ filename: 'confirmacao-incorreta.txt' }),
});
assert.equal(rejectedDeletion.status, 400);
assert.equal((await request(`/books/${protectedBook.id}`)).book.is_available, true);

const opdsResponse = await fetch(`${baseUrl}/opds`);
assert.equal(opdsResponse.status, 200);
assert.match(opdsResponse.headers.get('content-type'), /application\/atom\+xml/);
assert.match(await opdsResponse.text(), /<feed[\s>]/);
const pagedOpds = await (await fetch(`${baseUrl}/opds?limit=1&format=txt`)).text();
assert.equal((pagedOpds.match(/<entry>/g) || []).length, 1);
assert.match(pagedOpds, /rel="next"/);
assert.match(pagedOpds, /format=txt/);
assert.match(pagedOpds, /offset=1/);

// Contrato Range: intervalo fora do arquivo nao retransmite o original inteiro.
const invalidRange = await fetch(`${baseUrl}/books/${protectedBook.id}/file`, { headers: { Range: 'bytes=999999-' } });
assert.equal(invalidRange.status, 416);
assert.match(invalidRange.headers.get('content-range'), /^bytes \*\/\d+$/);
assert.equal((await invalidRange.arrayBuffer()).byteLength, 0);
const suffixRange = await fetch(`${baseUrl}/books/${protectedBook.id}/file`, { headers: { Range: 'bytes=-4' } });
assert.equal(suffixRange.status, 206);
assert.equal((await suffixRange.arrayBuffer()).byteLength, 4);

// Exclusao integral confirmada preserva o original em uma lixeira recuperavel.
let trashToken;
try {
  const removed = await request(`/books/${protectedBook.id}/file`, {
    method: 'DELETE', body: JSON.stringify({ filename: protectedBook.filename }),
  });
  trashToken = removed.trash_token;
  assert.equal(removed.recoverable_from_application, true);
  assert.equal((await request(`/books/${protectedBook.id}`)).book.is_available, false);
  assert.ok((await request('/maintenance/trash')).items.some(item => item.token === trashToken));
} finally {
  if (trashToken) await request(`/maintenance/trash/${trashToken}/restore`, { method: 'POST' });
}
assert.equal((await request(`/books/${protectedBook.id}`)).book.is_available, true);
assert.equal((await request('/maintenance/trash')).items.length, 0);
const noDuplicate = catalog.books.find(book => !groups[0].books.some(candidate => candidate.id === book.id));
const refusedUnique = await fetch(`${baseUrl}/books/${noDuplicate.id}/file`, {
  method: 'DELETE', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ filename: noDuplicate.filename }),
});
assert.equal(refusedUnique.status, 400);
const preview = await request('/maintenance/covers/orphans');
assert.equal(preview.retention_days, 7);
const refusedCleanup = await fetch(`${baseUrl}/maintenance/covers/orphans`, {
  method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ filenames: ['../outside.png'] }),
});
assert.equal(refusedCleanup.status, 400);
assert.equal((await request('/maintenance/covers/orphans', { method: 'POST', body: JSON.stringify({ filenames: [] }) })).removed, 0);

if (process.env.ESTANTE_E2E_RUNTIME_DIRECTORY) {
  const coversRoot = `${process.env.ESTANTE_E2E_RUNTIME_DIRECTORY}/covers`;
  await writeFile(`${coversRoot}/old-orphan.png`, 'synthetic');
  await writeFile(`${coversRoot}/recent.png`, 'synthetic');
  const old = new Date(Date.now() - 8 * 86400 * 1000);
  await utimes(`${coversRoot}/old-orphan.png`, old, old);
  const eligible = await request('/maintenance/covers/orphans');
  assert.deepEqual(eligible.items.map(item => item.filename), ['old-orphan.png']);
  await request('/maintenance/covers/orphans', { method: 'POST', body: JSON.stringify({ filenames: ['old-orphan.png'] }) });
  await assert.rejects(access(`${coversRoot}/old-orphan.png`));
  await access(`${coversRoot}/recent.png`);
}

const { backup } = await request('/maintenance/backups', { method: 'POST' });
const archiveResponse = await fetch(`${baseUrl}/maintenance/backups/${encodeURIComponent(backup.filename)}`);
assert.equal(archiveResponse.status, 200);
const archive = await archiveResponse.arrayBuffer();
assert.deepEqual([...new Uint8Array(archive.slice(0, 2))], [0x50, 0x4b]);
const restoreForm = new FormData();
restoreForm.append('backup', new Blob([archive], { type: 'application/zip' }), backup.filename);
const restored = await request('/maintenance/restore', { method: 'POST', body: restoreForm });
assert.equal(restored.restored, true);
assert.equal(restored.original_books_unchanged, true);
assert.notEqual((await request('/settings')).catalog_id, settings.catalog_id);
const staleProgress = await fetch(`${baseUrl}/books/${bookId}/progress`, {
  method: 'PUT', headers: { 'Content-Type': 'application/json', 'X-Catalog-Id': settings.catalog_id },
  body: JSON.stringify({ location: {}, percent: 88 }),
});
assert.equal(staleProgress.status, 400);
const restoredNotes = (await request(`/books/${bookId}/notes`)).notes;
assert.ok(restoredNotes.some(n => JSON.stringify(n.location) === '{}'));
assert.ok(restoredNotes.some(n => n.location.quote === 'Trecho citado da obra' && n.location.color === 'yellow'));

const corruptRestoreForm = new FormData();
corruptRestoreForm.append('backup', new Blob(['isto nao e um arquivo ZIP']), 'corrompido.zip');
const corruptRestore = await fetch(`${baseUrl}/maintenance/restore`, {
  method: 'POST', body: corruptRestoreForm,
});
assert.equal(corruptRestore.status, 400);
assert.match((await corruptRestore.json()).error, /ZIP invalido/);
assert.equal((await request('/books')).books.length, 3);

await request('/scan', { method: 'POST' });
const incrementalScan = await waitForTask();
assert.equal(incrementalScan.found, 3);
assert.equal(incrementalScan.added, 0);
assert.equal(incrementalScan.updated, 0);
assert.equal(incrementalScan.unchanged, 3);

await request('/maintenance/covers', { method: 'POST' });
const covers = await waitForTask();
assert.equal(covers.phase, 'completed');
assert.equal(covers.profile, 'low_priority');

// Mesmo tamanho e amostras, miolo distinto: DELETE precisa recusar o falso positivo.
const data = new Uint8Array(192 * 1024).fill(65);
for (const [index, filename] of ['Sample_A.txt', 'Sample_B.txt'].entries()) {
  data[96 * 1024] = 65 + index;
  const form = new FormData(); form.append('file', new Blob([data]), filename);
  await request('/upload', { method: 'POST', body: form });
}
await request('/scan', { method: 'POST' });
await waitForTask();
const sampleBooks = (await request('/books?q=Sample')).books;
assert.equal(sampleBooks.length, 2);
const sampleDeletion = await fetch(`${baseUrl}/books/${sampleBooks[0].id}/file`, {
  method: 'DELETE', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ filename: sampleBooks[0].filename }),
});
assert.equal(sampleDeletion.status, 400);
assert.match((await sampleDeletion.json()).error, /comparacao integral/);
assert.equal((await request(`/books/${sampleBooks[0].id}`)).book.is_available, true);

console.log('API E2E aprovada: seguranca, falhas, scan incremental, busca, colecoes, duplicatas, saude, OPDS, backup/restauracao e capas.');
