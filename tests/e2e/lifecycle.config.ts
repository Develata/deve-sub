import { defineConfig } from '@playwright/test';
import { randomUUID } from 'crypto';

process.env.DEVE_SUB_LIFECYCLE_RUN_ID ??= randomUUID();
const runId = process.env.DEVE_SUB_LIFECYCLE_RUN_ID;
if (!/^[a-zA-Z0-9_-]+$/.test(runId)) throw new Error('invalid lifecycle run identifier');

export default defineConfig({
  testDir: './infrastructure',
  outputDir: `test-results/lifecycle-${runId}`,
  workers: 1,
  retries: 0,
  forbidOnly: !!process.env.CI,
  timeout: 10_000,
  reporter: 'list',
});
