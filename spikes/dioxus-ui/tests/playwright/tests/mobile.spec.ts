import { test, expect, type Page } from '@playwright/test';

async function waitForApp(page: Page) {
  await page.goto('/', { waitUntil: 'networkidle', timeout: 30_000 });
  await expect(page.getByRole('heading', { name: 'Deve Sub' })).toBeVisible({ timeout: 20_000 });
}

test.describe('Gate 8 — mobile basic operations', () => {
  test('hamburger menu opens sidebar on mobile viewport', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const hamburger = page.locator('button', { hasText: '☰' });
    await expect(hamburger).toBeVisible();

    const navButtons = page.getByText('节点管理');
    await expect(navButtons).not.toBeVisible();

    await hamburger.click();
    await page.waitForTimeout(200);
    await expect(navButtons).toBeVisible();
  });

  test('navigation works via mobile menu', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const hamburger = page.locator('button', { hasText: '☰' });
    await hamburger.click();
    await page.waitForTimeout(200);

    await page.getByText('代理分组').click();
    await page.waitForTimeout(200);
    await expect(page.getByText('代理分组排序')).toBeVisible();
  });

  test('overlay closes mobile menu on click', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const hamburger = page.locator('button', { hasText: '☰' });
    await hamburger.click();
    await page.waitForTimeout(200);

    const overlay = page.locator('div.fixed.inset-0.bg-black\\/30');
    await expect(overlay).toBeVisible();

    await overlay.click();
    await page.waitForTimeout(200);
    await expect(overlay).not.toBeVisible();
  });

  test('dashboard chart visible on mobile viewport', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const svg = page.locator('svg').first();
    await expect(svg).toBeVisible({ timeout: 10_000 });

    const box = await svg.boundingBox();
    expect(box).toBeTruthy();
    expect(box!.width).toBeLessThanOrEqual(420);
  });
});
