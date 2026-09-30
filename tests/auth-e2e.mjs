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

// Configuracao incompleta deve abortar antes de abrir a porta ou criar dados.
const { spawn } = await import('node:child_process');
for (const credentials of [
  { AUTH_USERNAME: 'synthetic-user', AUTH_PASSWORD: '' },
  { AUTH_USERNAME: '', AUTH_PASSWORD: 'synthetic-password' },
]) {
  const child = spawn(`${process.env.CARGO_TARGET_DIR || 'target'}/debug/estante-livre`, [], {
    env: { ...process.env, ...credentials, APP_BIND: '127.0.0.1:20006',
      LIBRARY_ROOT: 'tests/fixtures/e2e', DATABASE_PATH: 'tests/runtime-data-auth-invalid/library.db',
      COVERS_DIR: 'tests/runtime-data-auth-invalid/covers',
      BRANDING_DIR: 'tests/runtime-data-auth-invalid/branding', BACKUP_DIR: 'tests/runtime-data-auth-invalid/backups' },
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let stderr = '';
  child.stderr.on('data', chunk => { stderr += chunk; });
  const timeout = setTimeout(() => child.kill('SIGKILL'), 5000);
  try {
    const result = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => resolve({ code, signal }));
    });
    assert.equal(result.signal, null);
    assert.notEqual(result.code, 0);
    assert.match(stderr, /AUTH_USERNAME e AUTH_PASSWORD/);
  } finally { clearTimeout(timeout); }
}
console.log('Configuracao parcial de autenticacao recusada antes da inicializacao.');
