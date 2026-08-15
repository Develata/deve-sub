import { test, expect } from '@playwright/test';

// DS-AUD-049: most authenticated UI specs use an API-injected cookie fixture
// (fixtures/auth.ts), which bypasses the real login form, CSRF, Secure cookie,
// session refresh, and 2FA challenge paths. This suite exercises the real
// browser auth flow through the login form so those paths stay covered.

test.describe('Real browser auth (DS-AUD-049)', () => {

  test('standard login through form enters the app', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    // Seeded server already has an admin, so we land on the login page.
    await expect(page.locator('h1').filter({ hasText: /登录 Deve Sub|Sign in to Deve Sub/ }))
      .toBeVisible({ timeout: 15000 });

    // Fill the login form and submit.
    await page.locator('input[type="text"]').first().fill('admin');
    await page.locator('input[type="password"]').first().fill('TestPassword12345');
    await page.locator('button').filter({ hasText: /^登录$|^Login$/ }).click();

    // The app shell (sidebar) should become visible — proving a real session
    // was established through the form, not an injected cookie.
    await expect(page.locator('aside')).toBeVisible({ timeout: 15000 });
  });

  test('wrong password shows error and stays on login page', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('h1').filter({ hasText: /登录 Deve Sub|Sign in to Deve Sub/ }))
      .toBeVisible({ timeout: 15000 });

    await page.locator('input[type="text"]').first().fill('admin');
    await page.locator('input[type="password"]').first().fill('WrongPassword123');
    await page.locator('button').filter({ hasText: /^登录$|^Login$/ }).click();

    // Error message appears.
    await expect(page.locator('p').filter({ hasText: /用户名或密码错误|Invalid username or password/ }))
      .toBeVisible({ timeout: 10000 });
    // Still on the login page — no sidebar.
    await expect(page.locator('aside')).not.toBeVisible();
  });

  test('session survives a full page reload', async ({ page }) => {
    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('h1').filter({ hasText: /登录 Deve Sub|Sign in to Deve Sub/ }))
      .toBeVisible({ timeout: 15000 });

    await page.locator('input[type="text"]').first().fill('admin');
    await page.locator('input[type="password"]').first().fill('TestPassword12345');
    await page.locator('button').filter({ hasText: /^登录$|^Login$/ }).click();
    await expect(page.locator('aside')).toBeVisible({ timeout: 15000 });

    // Reload — the session cookie should keep us in the app.
    await page.reload();
    await page.waitForLoadState('networkidle');
    await expect(page.locator('aside')).toBeVisible({ timeout: 15000 });
  });

  // DS-AUD-003: the frontend login handler must transition to a
  // TwoFactorChallenge stage when the backend returns requires_2fa=true,
  // instead of calling on_success immediately (which would push the user
  // into the authenticated shell with no session cookie).
  //
  // The seeded server's admin does not have 2FA enabled, so we mock the
  // POST /auth/login response via page.route() to return requires_2fa=true
  // with a challenge_token. This tests the frontend state machine directly.
  test('2FA challenge is presented when login returns requires_2fa (DS-AUD-003)', async ({ page }) => {
    await page.route('**/api/v1/auth/login', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          user: {
            id: '01KZAAAAAAAAAAAAAAAAAAAA00',
            username: 'admin',
            role: 'admin',
            enabled: true,
            traffic_quota: 0,
            two_factor_enabled: true,
            created_at: '2025-01-01T00:00:00Z',
          },
          requires_2fa: true,
          challenge_token: 'mock-challenge-token',
        }),
      });
    });

    await page.goto('/');
    await page.waitForLoadState('networkidle');

    await expect(page.locator('h1').filter({ hasText: /登录 Deve Sub|Sign in to Deve Sub/ }))
      .toBeVisible({ timeout: 15000 });

    await page.locator('input[type="text"]').first().fill('admin');
    await page.locator('input[type="password"]').first().fill('TestPassword12345');
    await page.locator('button').filter({ hasText: /^登录$|^Login$/ }).click();

    // The 2FA challenge UI must appear — not the app shell.
    await expect(page.locator('h1').filter({ hasText: /两步验证|Two-Factor Authentication/ }))
      .toBeVisible({ timeout: 10000 });
    await expect(page.locator('aside')).not.toBeVisible();

    // A code input field and verify button must be present.
    await expect(page.locator('input[type="text"]').first()).toBeVisible();
    await expect(page.locator('button').filter({ hasText: /^验证$|^Verify$/ })).toBeVisible();
  });
});
