import { rm, cp } from 'node:fs/promises';
import { spawn } from 'node:child_process';

const runtimeDirectory = 'tests/runtime-data-browser';
await rm(runtimeDirectory, { recursive: true, force: true });
await cp('tests/fixtures/e2e', `${runtimeDirectory}/library`, { recursive: true });

const server = spawn('cargo', ['run', '--locked'], {
  stdio: 'inherit',
  env: {
    ...process.env,
    CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR || 'target_local',
    APP_BIND: '127.0.0.1:20004',
    LIBRARY_ROOT: `${runtimeDirectory}/library`,
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
