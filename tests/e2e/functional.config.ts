import { defineConfig, devices } from '@playwright/test';
import { randomUUID } from 'crypto';

process.env.DEVE_SUB_FUNCTIONAL_RUN_ID ??= randomUUID();
const run = process.env.DEVE_SUB_FUNCTIONAL_RUN_ID;
if (!/^[a-zA-Z0-9_-]+$/.test(run)) throw new Error('invalid functional run ID');
const output = process.env.DEVE_SUB_FUNCTIONAL_OUTPUT ?? `test-results/functional-${run}`;

export default defineConfig({
  testDir: './functional', fullyParallel: true, workers: 4, retries: 0,
  forbidOnly: !!process.env.CI, timeout: 60_000, globalTimeout: 600_000,
  expect: { timeout: 10_000 },
  outputDir: output,
  reporter: [['list'], ['json', { outputFile: `${output}/results.json` }]],
  use: { headless: true, trace: 'retain-on-failure', screenshot: 'only-on-failure',
    actionTimeout: 10_000, navigationTimeout: 15_000,
    permissions: ['clipboard-read', 'clipboard-write'],
    launchOptions: { executablePath: process.env.DEVE_SUB_BROWSER } },
  projects: [
    { name: 'functional-api', testMatch: /api\.spec\.ts/ },
    { name: 'functional-desktop', testMatch: /ui\.spec\.ts/, use: { ...devices['Desktop Chrome'] } },
    { name: 'functional-mobile', testMatch: /ui\.spec\.ts/, use: { ...devices['Pixel 5'] } },
  ],
});
