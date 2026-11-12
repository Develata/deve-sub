import { defineConfig, devices } from '@playwright/test';
import { resolve } from 'path';

const output = process.env.DEVE_SUB_NORMAL_OUTPUT ?? resolve(__dirname, 'test-results/normal');
export default defineConfig({
  // Every test owns its process/database. This enables test-level CI sharding;
  // one worker per runner still bounds concurrent servers and password hashing.
  testDir: './normal', fullyParallel: true, workers: 1, retries: 0,
  forbidOnly: !!process.env.CI, timeout: 240_000, globalTimeout: 1_800_000,
  expect: { timeout: 15_000 }, outputDir: output,
  reporter: [['list'], ['json', { outputFile: resolve(output, 'results.json') }]],
  use: {
    ...devices['Desktop Chrome'], headless: true, trace: 'retain-on-failure',
    screenshot: 'only-on-failure', actionTimeout: 15_000, navigationTimeout: 20_000,
    permissions: ['clipboard-read', 'clipboard-write'],
    launchOptions: { executablePath: process.env.DEVE_SUB_BROWSER },
  },
});
