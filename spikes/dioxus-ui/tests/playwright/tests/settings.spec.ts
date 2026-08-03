import { test, expect, type Page } from '@playwright/test';

async function waitForApp(page: Page) {
  await page.goto('/', { waitUntil: 'networkidle', timeout: 30_000 });
  await expect(page.getByText('Deve Sub').first()).toBeVisible({ timeout: 20_000 });
}

async function goToSettings(page: Page) {
  await page.goto('/');
  await waitForApp(page);
  await page.getByText('设置').click();
  await expect(page.getByRole('heading', { name: '主题' })).toBeVisible({ timeout: 10_000 });
}

test.describe('Gate 4 — Chinese/English i18n switching', () => {
  test('switch to English updates nav labels', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    await expect(page.getByText('仪表盘')).toBeVisible();

    await page.getByRole('button', { name: 'EN' }).click();
    await page.waitForTimeout(200);

    await expect(page.getByText('Dashboard')).toBeVisible();
    await expect(page.getByText('Nodes')).toBeVisible();
    await expect(page.getByText('Settings')).toBeVisible();
  });

  test('switch back to Chinese restores nav labels', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    await page.getByRole('button', { name: 'EN' }).click();
    await page.waitForTimeout(200);
    await expect(page.getByText('Dashboard')).toBeVisible();

    await page.getByRole('button', { name: '中文' }).click();
    await page.waitForTimeout(200);
    await expect(page.getByText('仪表盘')).toBeVisible();
  });

  test('language switch persists on settings page', async ({ page }) => {
    await goToSettings(page);

    await page.getByRole('button', { name: 'EN' }).first().click();
    await page.waitForTimeout(200);
    await expect(page.getByRole('heading', { name: 'Theme' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Language' })).toBeVisible();
  });
});

test.describe('Gate 5 — light/dark/custom theme', () => {
  test('dark theme adds dark class to html', async ({ page }) => {
    await goToSettings(page);

    await page.getByText('深色').click();
    await page.waitForTimeout(200);

    const htmlClass = await page.locator('html').getAttribute('class');
    expect(htmlClass).toContain('dark');
  });

  test('light theme removes dark class', async ({ page }) => {
    await goToSettings(page);

    await page.getByText('深色').click();
    await page.waitForTimeout(200);
    await page.getByText('浅色').click();
    await page.waitForTimeout(200);

    const htmlClass = await page.locator('html').getAttribute('class') ?? '';
    expect(htmlClass).not.toContain('dark');
  });

  test('amber theme adds theme-amber class', async ({ page }) => {
    await goToSettings(page);

    await page.getByText('琥珀').click();
    await page.waitForTimeout(200);

    const htmlClass = await page.locator('html').getAttribute('class') ?? '';
    expect(htmlClass).toContain('theme-amber');
  });

  test('theme persists in localStorage', async ({ page }) => {
    await goToSettings(page);

    await page.getByText('深色').click();
    await page.waitForTimeout(200);

    const stored = await page.evaluate(() => localStorage.getItem('theme'));
    expect(stored).toBe('dark');
  });
});
