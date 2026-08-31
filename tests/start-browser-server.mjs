import { rm } from 'node:fs/promises';
import { spawn } from 'node:child_process';

const runtimeDirectory = 'tests/runtime-data-browser';
await rm(runtimeDirectory, { recursive: true, force: true });

const server = spawn('cargo', ['run', '--locked'], {
  stdio: 'inherit',
  env: {
    ...process.env,
    APP_BIND: '127.0.0.1:20004',
    LIBRARY_ROOT: 'tests/fixtures/e2e',
    DATABASE_PATH: `${runtimeDirectory}/library.db`,
    COVERS_DIR: `${runtimeDirectory}/covers`,
    BRANDING_DIR: `${runtimeDirectory}/branding`,
    BACKUP_DIR: `${runtimeDirectory}/backups`,
    RUST_LOG: 'estante_livre=info',
  },
});

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => server.kill(signal));
}

server.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 1);
});
