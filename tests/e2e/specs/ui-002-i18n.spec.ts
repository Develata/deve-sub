import { test, expect } from '../fixtures/auth';

test('UI-002: Chinese/English language switch is instant and persists after reload', async ({ authedPage: page }) => {
  const zhBtn = page.locator('button').filter({ hasText: '中文' }).first();
  const enBtn = page.locator('button').filter({ hasText: 'EN' }).first();

  await zhBtn.click();
  await expect(page.locator('aside')).toContainText('仪表盘');
  await expect(page.locator('aside')).toContainText('节点管理');

  await enBtn.click();
  await expect(page.locator('aside')).toContainText('Dashboard');
  await expect(page.locator('aside')).toContainText('Nodes');

  await page.reload();
  await page.waitForLoadState('networkidle');
  await expect(page.locator('aside')).toBeVisible({ timeout: 10000 });
  await expect(page.locator('aside')).toContainText('Dashboard');

  await zhBtn.click();
  await page.reload();
  await page.waitForLoadState('networkidle');
  await expect(page.locator('aside')).toBeVisible({ timeout: 10000 });
  await expect(page.locator('aside')).toContainText('仪表盘');
});
