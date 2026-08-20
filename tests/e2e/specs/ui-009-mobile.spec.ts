import { test, expect } from '../fixtures/auth';

test('UI-009 (mobile): can view subscriptions, refresh sources, and copy links', async ({ authedPage: page }) => {
  await expect(page.locator('main')).toBeVisible({ timeout: 10000 });

  const hamburger = page.locator('button[aria-label="Menu"]');
  await expect(hamburger).toBeVisible();
  await hamburger.click();

  await page.locator('aside button').filter({ hasText: /长期订阅|Subscriptions/ }).click();
  await page.waitForLoadState('networkidle');

  const subTable = page.locator('table');
  await expect(subTable).toBeVisible({ timeout: 10000 });
  await expect(page.locator('tr').filter({ hasText: 'test-sub' })).toBeVisible();

  const copyBtn = page.locator('button').filter({ hasText: /复制|Copy/ }).first();
  await expect(copyBtn).toBeVisible();

  await page.screenshot({ path: 'screenshots/ui-009-mobile-subscriptions.png' });

  await hamburger.click();
  await page.locator('aside button').filter({ hasText: /订阅源|Sources/ }).click();
  await page.waitForLoadState('networkidle');

  const sourceTable = page.locator('table');
  await expect(sourceTable).toBeVisible({ timeout: 10000 });
  await expect(page.locator('tr').filter({ hasText: 'test-source' })).toBeVisible();

  const refreshBtn = page.locator('button').filter({ hasText: /刷新|Refresh/ }).first();
  await expect(refreshBtn).toBeVisible();

  await page.screenshot({ path: 'screenshots/ui-009-mobile-sources.png' });
});
