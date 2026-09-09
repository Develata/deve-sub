import { defineConfig, devices } from '@playwright/test';
import { FRESH_PORT, SEEDED_PORT, RUN_ID } from './runtime-config';

export default defineConfig({
  testDir: './specs',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: [['list'], ['html', { open: 'never', outputFolder: `playwright-report/${RUN_ID}` }]],
  outputDir: `test-results/${RUN_ID}`,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  globalSetup: './global-setup.ts',
  use: {
    trace: 'retain-on-failure',
    headless: true,
    screenshot: 'only-on-failure',
    permissions: ['clipboard-read', 'clipboard-write'],
  },
  projects: [
    {
      name: 'ui-001-setup',
      testMatch: /ui-001/,
      use: {
        baseURL: `http://127.0.0.1:${FRESH_PORT}`,
        ...devices['Desktop Chrome'],
      },
    },
    {
      name: 'ui-authenticated',
      testMatch: /ui-00[2-9]|ui-010/,
      use: {
        baseURL: `http://127.0.0.1:${SEEDED_PORT}`,
        ...devices['Desktop Chrome'],
      },
    },
    {
      name: 'ui-009-mobile',
      testMatch: /ui-009/,
      use: {
        baseURL: `http://127.0.0.1:${SEEDED_PORT}`,
        ...devices['Pixel 5'],
      },
    },
    {
      // DS-AUD-049: real browser auth through the login form, not API cookie
      // injection. Uses the seeded server so an admin already exists.
      name: 'real-auth',
      testMatch: /real-auth/,
      use: {
        baseURL: `http://127.0.0.1:${SEEDED_PORT}`,
        ...devices['Desktop Chrome'],
      },
    },
  ],
});
