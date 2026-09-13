import { test, expect } from '../fixtures/auth';

test('UI-009 (mobile): can view subscriptions, refresh sources, and copy links', async ({ authedPage: page }, testInfo) => {
  await expect(page.locator('main')).toBeVisible({ timeout: 10000 });

  const hamburger = page.locator('button[aria-label="Menu"]');
  const mobile = (page.viewportSize()?.width ?? 1280) < 768;
  if (mobile) {
    await expect(hamburger).toBeVisible();
    await hamburger.click();
  } else {
    await expect(hamburger).toBeHidden();
  }

  await page.locator('aside button').filter({ hasText: /长期订阅|Subscriptions/ }).click();
  await page.waitForLoadState('networkidle');

  const subTable = page.locator('table');
  await expect(subTable).toBeVisible({ timeout: 10000 });
  await expect(page.locator('tr').filter({ hasText: 'test-sub' })).toBeVisible();

  const copyBtn = page.locator('button').filter({ hasText: /复制|Copy/ }).first();
  await expect(copyBtn).toBeVisible();

  // The list deliberately does not return plaintext short codes; Copy fetches
  // the detail resource. Missing-code guidance must leave its action visible.
  const row = page.locator('tr').filter({ hasText: 'test-sub' });
  const list = await page.request.get('/api/v1/subscriptions');
  const subscription = (await list.json()).subscriptions.find((s: { slug: string }) => s.slug === 'test-sub');
  const detail = await page.request.get(`/api/v1/subscriptions/${subscription.id}`);
  if (!(await detail.json()).subscription.short_code) {
    await copyBtn.click();
    await expect(page.getByRole('alert')).toContainText(/尚未生成|Generate a short code first/);
    await expect(row).toBeVisible();
  }
  const generated = page.waitForResponse(r => r.url().endsWith('/regenerate-short-code') && r.request().method() === 'POST');
  await row.getByRole('button', { name: /重生成短码|Regenerate Code/ }).click();
  const code = (await (await generated).json()).code;
  expect(code).toMatch(/^[A-Za-z0-9]{22}$/);
  await row.getByRole('button', { name: /复制|Copy/ }).click();
  await expect(row.getByRole('button', { name: '✓' })).toBeVisible();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(new URL(copied).pathname).toBe(`/s/${code}/mihomo`);
  expect((await page.request.get(copied)).status()).toBe(200);

  const prior = await page.request.post(`/api/v1/subscriptions/${subscription.id}/rotate-token`, { data: { grace_seconds: 0 } });
  const oldToken = (await prior.json()).token_plaintext;
  await row.getByRole('button', { name: /轮换\s*Token|Rotate Token/ }).click();
  await expect(page.getByText(/旧 Token 链接立即失效|old token URL will stop working immediately/)).toBeVisible();
  const rotation = page.waitForRequest(r => r.url().endsWith('/rotate-token') && r.method() === 'POST');
  await page.locator('div.fixed').getByRole('button', { name: /轮换\s*Token|Rotate Token/ }).click();
  expect((await rotation).postDataJSON()).toEqual({ grace_seconds: 0 });
  const urlInput = page.locator('input[readonly]');
  await expect(urlInput).toHaveValue(/\/sub\/[A-Za-z0-9_-]{43}\/mihomo$/);
  expect((await page.request.get(await urlInput.inputValue())).status()).toBe(200);
  expect((await page.request.get(`/sub/${oldToken}/mihomo`)).status()).toBe(404);
  expect((await page.request.get(copied)).status()).toBe(200);
  await page.locator('div.fixed').getByRole('button', { name: 'OK', exact: true }).click();

  await page.screenshot({ path: testInfo.outputPath('ui-009-mobile-subscriptions.png') });

  if (mobile) await hamburger.click();
  await page.locator('aside button').filter({ hasText: /订阅源|Sources/ }).click();
  await page.waitForLoadState('networkidle');

  const sourceTable = page.locator('table');
  await expect(sourceTable).toBeVisible({ timeout: 10000 });
  await expect(page.locator('tr').filter({ hasText: 'test-source' })).toBeVisible();

  const refreshBtn = page.locator('button').filter({ hasText: /刷新|Refresh/ }).first();
  await expect(refreshBtn).toBeVisible();

  await page.screenshot({ path: testInfo.outputPath('ui-009-mobile-sources.png') });
});
