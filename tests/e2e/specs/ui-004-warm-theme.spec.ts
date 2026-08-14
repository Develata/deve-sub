import { test, expect } from '../fixtures/auth';

test('UI-004: Minimal Warm theme — layout and design tokens correct', async ({ authedPage: page }) => {
  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');

  await page.locator('button').filter({ hasText: /Minimal Warm/ }).first().click();

  const html = page.locator('html');
  await expect(html).toHaveClass(/theme-warm/);

  await page.locator('aside button').filter({ hasText: /仪表盘|Dashboard/ }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/ui-004-warm-dashboard.png' });

  await page.locator('aside button').filter({ hasText: /节点管理|Nodes/ }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/ui-004-warm-nodes.png' });

  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/ui-004-warm-settings.png' });

  const warmToken = await page.evaluate(() => {
    const el = document.documentElement;
    return el.classList.contains('theme-warm');
  });
  expect(warmToken).toBe(true);
});
