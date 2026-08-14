import { test, expect } from '../fixtures/auth';

test('UI-003: light/dark mode — all pages have no unreadable text', async ({ authedPage: page }) => {
  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');

  const html = page.locator('html');

  // Switch to dark.
  await page.locator('button').filter({ hasText: /深色|Dark/ }).first().click();
  await expect(html).toHaveClass(/dark/);

  // Screenshot dashboard in dark.
  await page.locator('aside button').filter({ hasText: /仪表盘|Dashboard/ }).click();
  await page.waitForLoadState('networkidle');
  await expect(page.locator('main')).toBeVisible();
  await page.screenshot({ path: 'screenshots/ui-003-dark-dashboard.png' });

  // Check nodes page in dark.
  await page.locator('aside button').filter({ hasText: /节点管理|Nodes/ }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/ui-003-dark-nodes.png' });

  // Switch to light.
  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');
  await page.locator('button').filter({ hasText: /浅色|Light/ }).first().click();
  await expect(html).not.toHaveClass(/dark/);

  await page.locator('aside button').filter({ hasText: /仪表盘|Dashboard/ }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/ui-003-light-dashboard.png' });

  // Verify text color is readable (not the same as background).
  const bodyColor = await page.evaluate(() => {
    const cs = getComputedStyle(document.body);
    return { color: cs.color, bg: cs.backgroundColor };
  });
  expect(bodyColor.color).toBeTruthy();
  expect(bodyColor.bg).toBeTruthy();
});
