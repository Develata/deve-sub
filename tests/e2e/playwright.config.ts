import { defineConfig, devices } from '@playwright/test';

const FRESH_PORT = 18080;
const SEEDED_PORT = 18081;

export default defineConfig({
  testDir: './specs',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  reporter: [['list'], ['html', { open: 'never' }]],
  timeout: 60_000,
  expect: { timeout: 15_000 },
  globalSetup: './global-setup.ts',
  globalTeardown: './global-teardown.ts',
  use: {
    trace: 'on-first-retry',
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
  ],
});
