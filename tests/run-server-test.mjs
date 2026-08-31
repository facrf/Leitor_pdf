import { spawn } from 'node:child_process';
import { rm } from 'node:fs/promises';

const testFile = process.argv[2];
if (!testFile?.startsWith('tests/') || !testFile.endsWith('.mjs')) {
  throw new Error('informe um teste JavaScript dentro de tests/');
}

const authScenario = testFile.endsWith('/auth-e2e.mjs');
const port = authScenario ? 20002 : 20001;
const label = authScenario ? 'auth' : 'api';
const runtimeDirectory = `tests/runtime-data-${label}-ci`;
const baseUrl = `http://127.0.0.1:${port}/api`;
await rm(runtimeDirectory, { recursive: true, force: true });

const server = spawn('cargo', ['run', '--locked'], {
  stdio: 'inherit',
  env: {
    ...process.env,
    APP_BIND: `127.0.0.1:${port}`,
    LIBRARY_ROOT: 'tests/fixtures/e2e',
    DATABASE_PATH: `${runtimeDirectory}/library.db`,
    COVERS_DIR: `${runtimeDirectory}/covers`,
    BRANDING_DIR: `${runtimeDirectory}/branding`,
    BACKUP_DIR: `${runtimeDirectory}/backups`,
    AUTH_USERNAME: authScenario ? 'facrf' : '',
    AUTH_PASSWORD: authScenario ? 'segredo-e2e' : '',
    RUST_LOG: 'estante_livre=info',
  },
});

const serverExit = new Promise(resolve => server.once('exit', (code, signal) => resolve({ code, signal })));

async function waitForServer() {
  for (let attempt = 0; attempt < 240; attempt += 1) {
    try {
      const response = await fetch(`${baseUrl}/health`);
      if (response.ok) return;
    } catch (_) {
      // O servidor ainda esta compilando ou iniciando.
    }
    const exited = await Promise.race([
      serverExit.then(result => ({ ...result, exited: true })),
      new Promise(resolve => setTimeout(() => resolve({ exited: false }), 500)),
    ]);
    if (exited.exited) throw new Error(`servidor encerrou antes do teste (codigo ${exited.code}, sinal ${exited.signal})`);
  }
  throw new Error('tempo excedido aguardando o servidor E2E');
}

async function stopServer() {
  if (server.exitCode !== null || server.signalCode !== null) return;
  server.kill('SIGTERM');
  const stopped = await Promise.race([
    serverExit.then(() => true),
    new Promise(resolve => setTimeout(() => resolve(false), 5_000)),
  ]);
  if (!stopped) server.kill('SIGKILL');
}

let testExitCode = 1;
try {
  await waitForServer();
  const test = spawn(process.execPath, [testFile], {
    stdio: 'inherit',
    env: { ...process.env, ESTANTE_E2E_URL: baseUrl },
  });
  testExitCode = await new Promise(resolve => test.once('exit', code => resolve(code ?? 1)));
} finally {
  await stopServer();
}

process.exitCode = testExitCode;
