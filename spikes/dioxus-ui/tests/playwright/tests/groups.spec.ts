import { test, expect, type Page } from '@playwright/test';

async function waitForApp(page: Page) {
  await page.goto('/', { waitUntil: 'networkidle', timeout: 30_000 });
  await expect(page.getByText('Deve Sub').first()).toBeVisible({ timeout: 20_000 });
}

async function goToGroups(page: Page) {
  await page.goto('/');
  await waitForApp(page);
  await page.getByText('代理分组').click();
  await expect(page.getByText('代理分组排序')).toBeVisible({ timeout: 10_000 });
}

test.describe('Gate 3 — 500-item drag-and-drop sorting', () => {
  test('renders 500 group items', async ({ page }) => {
    await goToGroups(page);

    const items = page.locator('div[draggable="true"]');
    await expect(items.first()).toBeVisible();
    const count = await items.count();
    expect(count).toBeGreaterThan(10);

    const lastItem = page.getByText('Group-500');
    await expect(lastItem).toBeVisible({ timeout: 5_000 });
  });

  test('drag-and-drop reorders items', async ({ page }) => {
    await goToGroups(page);

    const items = page.locator('div[draggable="true"]');
    const firstItem = items.first();
    const secondItem = items.nth(1);

    const firstName = (await firstItem.textContent()) ?? '';
    const secondName = (await secondItem.textContent()) ?? '';
    expect(firstName).toContain('Group-1');
    expect(secondName).toContain('Group-2');

    await firstItem.dragTo(secondItem);
    await page.waitForTimeout(300);

    const newFirstName = (await items.first().textContent()) ?? '';
    expect(newFirstName).toContain('Group-2');
  });
});
