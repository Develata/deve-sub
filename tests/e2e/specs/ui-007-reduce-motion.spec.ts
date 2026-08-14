import { test, expect } from '../fixtures/auth';

test('UI-007: reduce motion — non-essential animations disabled after system setting', async ({ authedPage: page }) => {
  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');

  const checkboxes = page.locator('input[type="checkbox"]');
  const reduceMotionCheckbox = checkboxes.nth(1);
  await reduceMotionCheckbox.check();

  const html = page.locator('html');
  await expect(html).toHaveClass(/reduce-motion/);

  const stored = await page.evaluate(() => localStorage.getItem('reduce-motion'));
  expect(stored).toBe('true');

  await page.reload();
  await page.waitForLoadState('networkidle');
  await expect(page.locator('aside')).toBeVisible({ timeout: 10000 });

  await expect(html).toHaveClass(/reduce-motion/);

  const hasReducedMotion = await page.evaluate(() => {
    return document.documentElement.classList.contains('reduce-motion');
  });
  expect(hasReducedMotion).toBe(true);

  await page.locator('aside button').filter({ hasText: /仪表盘|Dashboard/ }).click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: 'screenshots/ui-007-reduce-motion.png' });
});
