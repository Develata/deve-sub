import { test, expect, type Page } from '@playwright/test';

async function waitForApp(page: Page) {
  await page.goto('/', { waitUntil: 'networkidle', timeout: 30_000 });
  await expect(page.getByText('Deve Sub').first()).toBeVisible({ timeout: 20_000 });
}

test.describe('Gate 7 — 30-day traffic chart', () => {
  test('SVG chart renders with paths and axis labels', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const svg = page.locator('svg').first();
    await expect(svg).toBeVisible({ timeout: 10_000 });

    const paths = svg.locator('path');
    await expect(paths).toHaveCount(4, { timeout: 5_000 });

    const textElements = svg.locator('text');
    const textCount = await textElements.count();
    expect(textCount).toBeGreaterThanOrEqual(6);

    const circle = svg.locator('circle');
    await expect(circle).toHaveCount(1);
  });
});

test.describe('Gate 6 — SSE task progress', () => {
  test('progress bar animates from 0 toward 100%', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const progressBar = page.locator('div.h-full.rounded-full.bg-amber-500').first();
    await expect(progressBar).toBeVisible({ timeout: 10_000 });

    const getWidth = async () => {
      const style = (await progressBar.getAttribute('style')) ?? '';
      const m = style.match(/width:\s*([\d.]+)%/);
      return m ? parseFloat(m[1]) : -1;
    };

    const w1 = await getWidth();
    await page.waitForTimeout(800);
    const w2 = await getWidth();

    expect(w1).toBeGreaterThanOrEqual(0);
    expect(w2).toBeGreaterThan(w1);
  });

  test('progress reaches 100%', async ({ page }) => {
    await page.goto('/');
    await waitForApp(page);

    const progressBar = page.locator('div.h-full.rounded-full.bg-amber-500').first();
    await expect(progressBar).toBeVisible({ timeout: 10_000 });

    await expect(async () => {
      const style = (await progressBar.getAttribute('style')) ?? '';
      expect(style).toContain('100%');
    }).toPass({ timeout: 15_000 });
  });
});
