import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

// Executa as funcoes reais sem inicializar a interface ou um servidor Rust.
const calls = [];
let release;
const firstRequest = new Promise(resolve => { release = resolve; });
const context = vm.createContext({
  document: { addEventListener() {} }, clearTimeout, setTimeout,
  FormData: class {},
  fetch: async (url, options) => {
    calls.push({ url, payload: JSON.parse(options.body) });
    if (calls.length === 1) await firstRequest;
    return { ok: true, status: 200, json: async () => ({ saved: true }) };
  },
});
vm.runInContext(await readFile('web/app.js', 'utf8'), context);
vm.runInContext(`
  state.currentBook = { id: 1 };
  state.reader = { location: { page: 2 }, percent: 20 };
  persistProgress();
  state.reader = { location: { page: 3 }, percent: 30 };
  globalThis.lastSave = persistProgress();
  state.currentBook = { id: 2 };
  state.reader = { location: { page: 9 }, percent: 90 };
`, context);
await Promise.resolve();
assert.equal(calls.length, 1, 'segunda gravacao deve aguardar a primeira');
let finished = false;
context.lastSave.then(() => { finished = true; });
await Promise.resolve();
assert.equal(finished, false, 'chamador deve aguardar a fila');
release();
await context.lastSave;
assert.deepEqual(calls, [
  { url: '/api/books/1/progress', payload: { location: { page: 2 }, percent: 20 } },
  { url: '/api/books/1/progress', payload: { location: { page: 3 }, percent: 30 } },
]);
console.log('Progresso aprovado: ordem, espera e captura do livro antes da troca.');
