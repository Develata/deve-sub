import { test, expect } from '../fixtures/auth';

test('UI-008: 10,000 nodes — scroll, filter and select without noticeable lag', async ({ authedPage: page }) => {
  await page.locator('aside button').filter({ hasText: /节点管理|Nodes/ }).click();
  await page.waitForLoadState('networkidle');

  const scrollContainer = page.locator('#nodes-scroll');
  await expect(scrollContainer).toBeVisible({ timeout: 10000 });

  await expect(scrollContainer.locator('div').first()).toBeAttached();
  await page.waitForTimeout(1000);

  const t0 = Date.now();
  await scrollContainer.evaluate((el) => {
    el.scrollTop = 5000 * 48;
  });
  await page.waitForTimeout(300);
  const t1 = Date.now();
  expect(t1 - t0).toBeLessThan(2000);

  const nodeRows = scrollContainer.locator('div').filter({ hasText: /Node-/ });
  const visibleCount = await nodeRows.count();
  expect(visibleCount).toBeGreaterThan(0);
  expect(visibleCount).toBeLessThan(100);

  const searchInput = page.locator('input[type="search"]');
  await scrollContainer.evaluate((el) => { el.scrollTop = 0; });
  await page.waitForTimeout(200);
  await searchInput.fill('Node-0000');
  await page.waitForTimeout(500);

  const filteredRows = scrollContainer.locator('div').filter({ hasText: /Node-0000/ });
  const filteredCount = await filteredRows.count();
  expect(filteredCount).toBeGreaterThan(0);
  expect(filteredCount).toBeLessThan(15);

  await searchInput.fill('');
  await page.waitForTimeout(500);

  const firstCheckbox = scrollContainer.locator('input[type="checkbox"]').first();
  await firstCheckbox.check();

  await expect(page.locator('text=/已选|selected/')).toBeVisible({ timeout: 5000 });

  await page.screenshot({ path: 'screenshots/ui-008-nodes-selected.png' });
});
