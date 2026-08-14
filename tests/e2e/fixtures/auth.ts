import { test as base, expect, type Page, type BrowserContext } from '@playwright/test';

const ADMIN_USER = 'admin';
const ADMIN_PASS = 'TestPassword12345';

async function loginViaApi(port: number): Promise<string> {
  const res = await fetch(`http://127.0.0.1:${port}/api/v1/auth/login`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Origin': `http://127.0.0.1:${port}`,
    },
    body: JSON.stringify({ username: ADMIN_USER, password: ADMIN_PASS }),
  });
  if (!res.ok) throw new Error(`login failed: ${res.status}`);
  const setCookie = res.headers.get('set-cookie');
  if (!setCookie) throw new Error('no session cookie');
  return setCookie.split(';')[0];
}

async function login(page: Page, context: BrowserContext): Promise<void> {
  const baseURL = page.context()._options.baseURL;
  if (!baseURL) throw new Error('no baseURL configured');
  const port = parseInt(new URL(baseURL).port, 10);

  const cookieValue = await loginViaApi(port);
  const [name, value] = cookieValue.split('=');

  await context.addCookies([{
    name,
    value,
    domain: '127.0.0.1',
    path: '/',
    httpOnly: true,
    sameSite: 'Lax',
  }]);

  await page.goto('/');
  await page.waitForLoadState('networkidle');
  await expect(page.locator('aside')).toBeVisible({ timeout: 15000 });
  await page.waitForLoadState('networkidle');
}

export const test = base.extend<{ authedPage: Page }>({
  authedPage: async ({ page, context }, use) => {
    await login(page, context);
    await use(page);
  },
});

export { expect };
