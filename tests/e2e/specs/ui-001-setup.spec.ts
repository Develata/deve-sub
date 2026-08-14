import { test, expect } from '@playwright/test';

test('UI-001: first launch shows admin setup wizard with no default weak password', async ({ page }) => {
  await page.goto('/');
  await page.waitForLoadState('networkidle');

  // Setup wizard heading appears (Chinese or English).
  const heading = page.locator('h1').filter({ hasText: /初始化管理员|Admin Setup/ });
  await expect(heading).toBeVisible({ timeout: 15000 });

  // Password field exists and is empty (no pre-filled default).
  const passwordInput = page.locator('input[type="password"]').first();
  await expect(passwordInput).toHaveValue('');

  // Username field exists and is empty.
  const usernameInput = page.locator('input[type="text"]').first();
  await expect(usernameInput).toHaveValue('');

  // Submit button exists with setup label.
  const submitBtn = page.locator('button').filter({ hasText: /创建管理员|Create Admin/ });
  await expect(submitBtn).toBeVisible();

  // Fill and submit: username + strong password + confirm.
  await usernameInput.fill('admin');
  const pw = 'TestPassword12345';
  await page.locator('input[type="password"]').nth(0).fill(pw);
  await page.locator('input[type="password"]').nth(1).fill(pw);
  await submitBtn.click();

  // After setup, the app transitions to login (admin exists now).
  await expect(page.locator('h1').filter({ hasText: /登录|Sign in/ })).toBeVisible({ timeout: 10000 });
});
