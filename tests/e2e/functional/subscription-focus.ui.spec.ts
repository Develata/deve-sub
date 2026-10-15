import { test, expect, navigate, createTemplate, importNodes } from './fixture';
import type { Route } from '@playwright/test';

test('FUNC-SUB-COMPLETION pending, failed and one-time link states retain focus through retry and keyboard copy', async ({ api, page }) => {
  await importNodes(api);
  await createTemplate(api);
  await page.setViewportSize({ width: page.viewportSize()!.width, height: 420 });
  await navigate(page, /^长期订阅$|^Subscriptions$/);
  const add = page.getByRole('button', { name: /^添加订阅$|^Add Subscription$/ });
  const dialog = page.getByRole('dialog');
  const name = 'Completion focus fixture';
  let id = '';
  for (const operation of ['create', 'rotate', 'temporary'] as const) {
    let opener = add;
    if (operation !== 'create') {
      const row = page.getByRole('row').filter({ hasText: name });
      if (await row.locator('details').getAttribute('open') === null) await row.locator('summary').click();
      opener = row.getByRole('button', { name: operation === 'rotate' ? /轮换\s*Token|Rotate Token/ : /临时链接|Temp Link/ });
    }
    await opener.click();
    if (operation === 'create') {
      await dialog.locator('#subscription-name').fill(name);
      await dialog.locator('#subscription-slug').fill('completion-focus-fixture');
    } else if (operation === 'temporary') {
      await dialog.locator('#subscription-temp-expiry').fill(new Date(Date.now() + 86400000).toISOString());
    }
    const endpoint = operation === 'create' ? '/api/v1/subscriptions'
      : `/api/v1/subscriptions/${id}/${operation === 'rotate' ? 'rotate-token' : 'temp-links'}`;
    let held: Route | undefined;
    let writes = 0;
    await page.route(`**${endpoint}`, route => {
      if (route.request().method() !== 'POST') return route.continue();
      writes++;
      held = route;
    });
    const submit = dialog.getByRole('button', { name: operation === 'rotate' ? /轮换\s*Token|Rotate Token/ : /^保存$|^Save$/ });
    await submit.click();
    await expect.poll(() => !!held).toBe(true);
    await expect(dialog.getByRole('status')).toBeFocused();
    await held!.fulfill({ status: 503, json: { error: 'fixture_failure', message: 'Fixture save failure' } });
    await expect(dialog.getByRole('alert')).toHaveText('Fixture save failure');
    await expect(dialog.getByRole('alert')).toBeFocused();
    expect(writes).toBe(1);
    if (operation === 'create') {
      await expect(dialog.locator('#subscription-name')).toHaveValue(name);
      await page.screenshot({ scale: 'css', path: test.info().outputPath('subscription-error-focus.png') });
    }
    // Retry entirely with the keyboard after focus moves to the error.
    await page.keyboard.press('Tab');
    await expect(dialog.getByRole('button', { name: /^取消$|^Cancel$/ })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(submit).toBeFocused();
    held = undefined;
    await page.keyboard.press('Enter');
    await expect.poll(() => !!held).toBe(true);
    await expect(dialog.getByRole('status')).toBeFocused();
    await held!.continue();
    const url = dialog.locator('input[readonly]');
    await expect(url).toBeFocused();
    const value = await url.inputValue();
    expect((await api.get(value)).status()).toBe(200);
    if (operation === 'create') {
      const items = (await (await api.get('/api/v1/subscriptions')).json()).subscriptions;
      id = items.find((sub: { slug: string }) => sub.slug === 'completion-focus-fixture').id;
      await page.screenshot({ scale: 'css', path: test.info().outputPath('subscription-link-focus.png') });
    }
    await page.keyboard.press('Tab');
    await expect(dialog.getByRole('button', { name: /^复制$|^Copy$/ })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(dialog.getByRole('status')).toContainText(/已复制|Copied/);
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(value);
    await page.keyboard.press('Tab');
    await expect(dialog.getByRole('button', { name: 'OK', exact: true })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(dialog).toHaveCount(0);
    await expect(opener).toBeFocused();
    expect(writes).toBe(2);
    await page.unroute(`**${endpoint}`);
  }
});
