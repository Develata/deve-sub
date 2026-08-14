import { test, expect } from '@playwright/test';

// DS-AUD-049: most authenticated UI specs use an API-injected cookie fixture
// (fixtures/auth.ts), which bypasses the real login form, CSRF, Secure cookie,
// session refresh, and 2FA challenge paths. This suite exercises the real
// browser auth flow through the login form so those paths stay covered.
//
// The first three tests are marked fixme because they are blocked by
// DS-AUD-002: the setup-status probe sends a password shorter than
// MIN_PASSWORD_LEN, so the backend returns 400 (invalid password) before
// checking whether an admin exists. The frontend interprets 400 as
// "NeedsSetup" and shows the setup wizard instead of the login page — even
// on an already-initialized instance. These tests will pass once Phase C
// introduces a side-effect-free GET /auth/status endpoint and fixes the
// probe logic. Remove `test.fixme` at that time.

test.describe('Real browser auth (DS-AUD-049)', () => {

  test.fixme('standard login through form enters the app', async ({ page }) => {
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

  test.fixme('wrong password shows error and stays on login page', async ({ page }) => {
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

  test.fixme('session survives a full page reload', async ({ page }) => {
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

  // DS-AUD-003: the frontend login handler treats any Ok(_) from the API as
  // success and immediately calls on_success, even when the backend returned
  // requires_2fa=true with a challenge_token and no session cookie. A 2FA-
  // enabled admin is therefore pushed into the authenticated shell with no
  // session, and every subsequent API call fails with 401.
  //
  // This test is marked fixme until Phase C implements the
  // Credentials -> TwoFactorChallenge -> Authenticated state machine.
  test.fixme('2FA challenge is presented when admin has 2FA enabled (DS-AUD-003)', async () => {
    // Phase C work: create a 2FA-enabled admin via API, login through the
    // form with credentials, and assert a TOTP/recovery-code input step
    // appears before the app shell. Currently the frontend has no 2FA UI at
    // all (no i18n keys, no challenge handler, no /auth/login/2fa call).
  });
});
