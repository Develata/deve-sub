import { test, expect } from '../fixtures/auth';

test('UI-006: custom accent color and product name persist after reload', async ({ authedPage: page }) => {
  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');

  const customCheckbox = page.locator('input[type="checkbox"]').first();
  const isAlreadyChecked = await customCheckbox.isChecked();
  if (!isAlreadyChecked) {
    await customCheckbox.check();
  }

  const colorInput = page.locator('input[type="color"]');
  await expect(colorInput).toBeVisible();
  await colorInput.fill('#0066cc');

  const accentVar = await page.evaluate(() => {
    const el = document.documentElement;
    return el.style.getPropertyValue('--color-accent');
  });
  expect(accentVar).toContain('0066cc');

  const nameInput = page.locator('input[type="text"]').last();
  await nameInput.fill('My Custom Sub');
  await page.locator('button').filter({ hasText: /保存|Save/ }).click();

  const savedCheck = page.locator('span').filter({ hasText: '✓' });
  await expect(savedCheck).toBeVisible({ timeout: 5000 });

  const storedName = await page.evaluate(() => localStorage.getItem('custom-product-name'));
  expect(storedName).toBe('My Custom Sub');

  const storedAccent = await page.evaluate(() => localStorage.getItem('custom-accent'));
  expect(storedAccent).toContain('0066cc');

  await page.reload();
  await page.waitForLoadState('networkidle');
  await expect(page.locator('aside')).toBeVisible({ timeout: 10000 });

  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');

  const accentAfterReload = await page.evaluate(() => {
    return localStorage.getItem('custom-accent');
  });
  expect(accentAfterReload).toContain('0066cc');

  const nameAfterReload = await page.evaluate(() => {
    return localStorage.getItem('custom-product-name');
  });
  expect(nameAfterReload).toBe('My Custom Sub');
});
