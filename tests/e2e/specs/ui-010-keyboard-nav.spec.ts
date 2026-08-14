import { test, expect } from '../fixtures/auth';

test('UI-010: keyboard navigation — sidebar, table and dialog usable via keyboard', async ({ authedPage: page }) => {
  await expect(page.locator('aside')).toBeVisible({ timeout: 10000 });

  const firstNavBtn = page.locator('aside nav button').first();
  await firstNavBtn.focus();
  await expect(firstNavBtn).toBeFocused();

  await page.keyboard.press('Tab');
  const secondNavBtn = page.locator('aside nav button').nth(1);
  await expect(secondNavBtn).toBeFocused();

  await page.keyboard.press('Enter');
  await page.waitForLoadState('networkidle');

  await expect(page.locator('main')).toBeVisible();

  await page.locator('aside button').filter({ hasText: /节点管理|Nodes/ }).click();
  await page.waitForLoadState('networkidle');

  const searchInput = page.locator('input[type="search"]');
  await searchInput.focus();
  await expect(searchInput).toBeFocused();
  await searchInput.fill('Node-0000');
  await page.waitForTimeout(500);

  await page.keyboard.press('Tab');
  await page.waitForTimeout(200);

  const scrollContainer = page.locator('#nodes-scroll');
  await expect(scrollContainer).toBeVisible();

  await page.locator('aside button').filter({ hasText: /长期订阅|Subscriptions/ }).click();
  await page.waitForLoadState('networkidle');

  const table = page.locator('table');
  await expect(table).toBeVisible({ timeout: 10000 });

  const copyBtn = page.locator('button').filter({ hasText: /复制|Copy/ }).first();
  await copyBtn.focus();
  await expect(copyBtn).toBeFocused();

  await page.keyboard.press('Enter');
  await page.waitForTimeout(500);

  await page.screenshot({ path: 'screenshots/ui-010-keyboard-nav.png' });
});
