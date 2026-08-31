import assert from 'node:assert/strict';

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
  settings.library_root.endsWith('/pdf') || settings.library_root.endsWith('tests/fixtures/e2e'),
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

console.log('API E2E aprovada: seguranca, falhas, scan incremental, busca, colecoes, duplicatas, saude, OPDS, backup/restauracao e capas.');
