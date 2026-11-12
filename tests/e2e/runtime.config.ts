import { defineConfig } from '@playwright/test';
import { resolve } from 'path';

const output = process.env.DEVE_SUB_RUNTIME_OUTPUT ?? resolve(__dirname, 'test-results/runtime');
export default defineConfig({
  testDir: './runtime', workers: 1, retries: 0, timeout: 120_000,
  outputDir: output, reporter: [['list'], ['json', { outputFile: resolve(output, 'results.json') }]],
  use: { baseURL: process.env.DEVE_SUB_RUNTIME_URL, headless: true, trace: 'off',
    permissions: ['clipboard-read', 'clipboard-write'],
    launchOptions: { executablePath: process.env.DEVE_SUB_BROWSER } },
});
