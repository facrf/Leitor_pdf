import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const [html, css, javascript] = await Promise.all([
  readFile('web/index.html', 'utf8'),
  readFile('web/styles.css', 'utf8'),
  readFile('web/app.js', 'utf8'),
]);

const requiredIds = [
  'scan-progress', 'book-grid', 'suggestions', 'reading-desk',
  'filter-collection', 'filter-publisher', 'filter-language', 'filter-year',
  'collection-form', 'health-grid', 'duplicate-list', 'backup-list',
  'restore-input', 'generate-covers', 'opds-import-form',
];
for (const id of requiredIds) {
  assert.match(html, new RegExp(`id=["']${id}["']`), `controle #${id} ausente`);
  assert.match(javascript, new RegExp(`#${id}`), `controle #${id} sem integracao JavaScript`);
}

const ids = [...html.matchAll(/\sid=["']([^"']+)["']/g)].map(match => match[1]);
assert.equal(new Set(ids).size, ids.length, 'IDs HTML devem ser unicos');
assert.match(html, /Content-Security-Policy/);
assert.match(html, /Desenvolvido por FACRF\./);
assert.match(css, /@media \(max-width: 800px\)/);
assert.match(css, /@media \(max-width: 520px\)/);
assert.match(css, /prefers-reduced-motion/);
assert.match(javascript, /status\.unchanged/);
assert.match(javascript, /\/maintenance\/backups/);
assert.match(javascript, /\/opds\/import/);

console.log(`Contrato visual aprovado: ${requiredIds.length} controles, IDs unicos e layouts responsivos.`);
