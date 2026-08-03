import { test, expect, type Page } from '@playwright/test';

async function waitForApp(page: Page) {
  await page.goto('/', { waitUntil: 'networkidle', timeout: 30_000 });
  await expect(page.getByText('Deve Sub').first()).toBeVisible({ timeout: 20_000 });
}

async function goToNodes(page: Page) {
  await page.goto('/');
  await waitForApp(page);
  await page.getByText('节点管理').click();
  await expect(page.getByPlaceholder('搜索节点名称…')).toBeVisible({ timeout: 10_000 });
}

test.describe('Gate 1 — 10,000-node virtual list', () => {
  test('virtual list renders and supports scrolling', async ({ page }) => {
    await goToNodes(page);

    const scrollContainer = page.locator('#node-list-scroll');
    await expect(scrollContainer).toBeVisible();

    const items = scrollContainer.locator('div.flex.h-12.items-center');
    const initialCount = await items.count();
    expect(initialCount).toBeGreaterThan(5);

    await scrollContainer.evaluate((el) => el.scrollTo(0, 5000));
    await page.waitForTimeout(300);
    const scrolledCount = await items.count();
    expect(scrolledCount).toBeGreaterThan(5);
  });

  test('pagination shows correct page info', async ({ page }) => {
    await goToNodes(page);

    const pageInfo = page.getByText(/Page \d+ of \d+/);
    await expect(pageInfo).toBeVisible();
    const text = (await pageInfo.textContent()) ?? '';
    expect(text).toMatch(/Page 1 of 200/);
  });
});

test.describe('Gate 2 — multi-select, pagination, filtering', () => {
  test('search filter narrows results', async ({ page }) => {
    await goToNodes(page);

    const search = page.getByPlaceholder('搜索节点名称…');
    await search.fill('Node-0000');
    await page.waitForTimeout(300);

    const pageInfo = page.getByText(/Page \d+ of \d+.*nodes/);
    const text = (await pageInfo.textContent()) ?? '';
    const match = text.match(/(\d+)\s+nodes/);
    expect(match).toBeTruthy();
    expect(parseInt(match![1])).toBeLessThan(100);
  });

  test('protocol filter narrows results', async ({ page }) => {
    await goToNodes(page);

    const select = page.locator('select');
    await select.selectOption('Vless');
    await page.waitForTimeout(300);

    const badges = page.locator('span.inline-flex.items-center.rounded-full', { hasText: 'Vless' });
    const count = await badges.count();
    expect(count).toBeGreaterThan(0);

    const allBadges = page.locator('span.inline-flex.items-center.rounded-full');
    for (let i = 0; i < Math.min(count, 5); i++) {
      const text = (await badges.nth(i).textContent()) ?? '';
      expect(text.trim()).toBe('Vless');
    }
  });

  test('multi-select with checkboxes', async ({ page }) => {
    await goToNodes(page);

    const checkboxes = page.locator('input[type="checkbox"]');
    const itemCheckboxes = checkboxes.nth(1);
    await itemCheckboxes.check();
    await page.waitForTimeout(200);

    const selectedText = page.getByText(/已选 \d+ 项/);
    await expect(selectedText).toBeVisible();

    await itemCheckboxes.uncheck();
    await page.waitForTimeout(200);
    await expect(selectedText).not.toBeVisible();
  });

  test('pagination next/prev buttons', async ({ page }) => {
    await goToNodes(page);

    const nextBtn = page.getByRole('button', { name: 'Next →' });
    await nextBtn.click();
    await page.waitForTimeout(300);

    const pageInfo = page.getByText(/Page \d+ of \d+/);
    const text = (await pageInfo.textContent()) ?? '';
    expect(text).toContain('Page 2');

    const prevBtn = page.getByRole('button', { name: '← Prev' });
    await prevBtn.click();
    await page.waitForTimeout(300);
    const text2 = (await pageInfo.textContent()) ?? '';
    expect(text2).toContain('Page 1');
  });
});
