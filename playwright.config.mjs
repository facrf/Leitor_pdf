import { defineConfig } from '@playwright/test';

const externalBaseUrl = process.env.ESTANTE_BROWSER_URL;
const baseURL = externalBaseUrl || 'http://127.0.0.1:20004';

export default defineConfig({
  testDir: './tests',
  testMatch: 'browser-e2e.spec.mjs',
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['line'], ['html', { open: 'never' }]] : 'line',
  outputDir: 'tests/runtime-data-playwright-results',
  use: {
    baseURL,
    browserName: 'chromium',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  webServer: externalBaseUrl ? undefined : {
    command: 'node tests/start-browser-server.mjs',
    url: `${baseURL}/api/health`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
