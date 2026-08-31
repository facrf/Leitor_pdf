import assert from 'node:assert/strict';

const baseUrl = process.env.ESTANTE_E2E_URL || 'http://127.0.0.1:20002/api';

const health = await fetch(`${baseUrl}/health`);
assert.equal(health.status, 200);

const denied = await fetch(`${baseUrl}/settings`);
assert.equal(denied.status, 401);
assert.match(denied.headers.get('www-authenticate'), /^Basic /);

const invalid = await fetch(`${baseUrl}/settings`, {
  headers: { Authorization: `Basic ${Buffer.from('facrf:senha-errada').toString('base64')}` },
});
assert.equal(invalid.status, 401);

const allowed = await fetch(`${baseUrl}/settings`, {
  headers: { Authorization: `Basic ${Buffer.from('facrf:segredo-e2e').toString('base64')}` },
});
assert.equal(allowed.status, 200);
assert.equal((await allowed.json()).auth_enabled, true);

console.log('Autenticacao E2E aprovada: saude publica e API protegida por HTTP Basic.');
