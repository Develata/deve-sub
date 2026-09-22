import { test, expect, navigate } from './fixture';
import type { Page } from '@playwright/test';

async function expectSourceActionsOnScreen(page: Page) {
  await expect(page.getByRole('table')).toBeVisible();
  for (const label of [/^刷新$|^Refresh$/, /^编辑$|^Edit$/, /^删除$|^Delete$/]) {
    const action = page.locator('tbody tr').first().getByRole('button', { name: label });
    await expect.poll(() => action.evaluate(element => {
      const bounds = element.getBoundingClientRect();
      return bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight;
    })).toBe(true);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
}


test('FUNC-SOURCE-FORM source editor supports keyboard, bounded layout and pending recovery', async ({ api, page }, info) => {
  await api.get('/api/v1/auth/me');
  await navigate(page, /^订阅源$|^Sources$/);
  const add = page.getByRole('button', { name: /^添加订阅源$|^Add Source$/ });
  await add.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  const name = dialog.getByLabel(/^名称$|^Name$/);
  const url = dialog.getByLabel('URL', { exact: true });
  await expect(name).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.getByRole('button', { name: /^保存$|^Save$/ })).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(name).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(add).toBeFocused();
  await add.click();
  await page.setViewportSize({ width: 390, height: 420 });
  await expect.poll(async () => dialog.evaluate(el => {
    const r = el.getBoundingClientRect();
    return r.top >= 0 && r.bottom <= innerHeight && r.left >= 0 && r.right <= innerWidth;
  })).toBe(true);
  await name.fill('Source keyboard fixture');
  await url.fill('https://source.example.com/sub');
  const interval = dialog.getByLabel(/更新间隔|Interval/);
  await interval.clear();
  await expect(interval).toHaveValue('');
  await interval.fill('7200');
  await page.screenshot({ path: info.outputPath('source-form-short-viewport.png') });
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let submissions = 0;
  await page.route('**/api/v1/sources', async route => {
    if (route.request().method() !== 'POST') return route.continue();
    submissions++;
    await held;
    await route.fulfill({ status: 503, contentType: 'application/json',
      body: JSON.stringify({ error: 'fixture_unavailable', message: 'Fixture save unavailable' }) });
  });
  try {
    await name.press('Enter');
    await expect.poll(() => submissions).toBe(1);
    await expect(name).toBeDisabled();
    await expect(dialog.getByRole('button', { name: /^取消$|^Cancel$/ })).toBeDisabled();
    await page.keyboard.press('Escape');
    await page.keyboard.press('Enter');
    await expect(dialog).toBeVisible();
    expect(submissions).toBe(1);
  } finally { release(); }
  await expect(dialog.getByRole('alert')).toContainText('Fixture save unavailable');
  await expect(name).toHaveValue('Source keyboard fixture');
  await expect(interval).toHaveValue('7200');
  await expect(name).toBeEnabled();
  await page.unroute('**/api/v1/sources');
  await dialog.getByRole('button', { name: /^保存$|^Save$/ }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('tbody tr')).toContainText('Source keyboard fixture');
  const sources = (await (await api.get('/api/v1/sources')).json()).sources;
  expect(sources).toHaveLength(1);
  expect(sources[0].update_interval_secs).toBe(7200);
  const row = page.locator('tbody tr').filter({ hasText: 'Source keyboard fixture' });
  await row.getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await expect(name).toHaveValue('Source keyboard fixture');
  await expect(interval).toHaveValue('7200');
  await expect(url).toHaveValue('');
  const update = page.waitForRequest(request => request.method() === 'PUT' && request.url().endsWith(`/sources/${sources[0].id}`));
  await name.fill('Source edited fixture');
  await name.press('Enter');
  expect((await update).postDataJSON()).not.toHaveProperty('url');
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('tbody tr')).toContainText('Source edited fixture');
  const edited = (await (await api.get(`/api/v1/sources/${sources[0].id}`)).json()).source;
  expect(edited.name).toBe('Source edited fixture');
  expect(edited.update_interval_secs).toBe(7200);
  expect(edited.url).toBe('https://source.example.com/***');
  await page.locator('tbody tr').getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await expect(url).toHaveValue('');
  const replacementHost = `${'replacement'.repeat(5)}.example.com`;
  const replacementUrl = `https://${replacementHost}/new-sub`;
  await url.fill(replacementUrl);
  const replacement = page.waitForRequest(request => request.method() === 'PUT' && request.url().endsWith(`/sources/${sources[0].id}`));
  await name.press('Enter');
  expect((await replacement).postDataJSON().url).toBe(replacementUrl);
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('tbody tr')).toContainText(`https://${replacementHost}/***`);
  await expectSourceActionsOnScreen(page);
  await page.getByRole('button', { name: 'EN', exact: true }).click();
  await expectSourceActionsOnScreen(page);
  await page.screenshot({ path: info.outputPath('source-form-completed.png') });
  await page.setViewportSize({ width: 1280, height: 720 });
  await expectSourceActionsOnScreen(page);
  expect(await page.locator('table').evaluate(element => getComputedStyle(element).display)).toBe('table');
  await expect(page.getByRole('columnheader', { name: 'URL', exact: true })).toBeVisible();
});

test('FUNC-SOURCE-DELETE confirmation identifies its source and defaults to cancel', async ({ api, page }) => {
  const created = await api.post('/api/v1/sources', { data: {
    name: 'Source delete fixture', source_type: 'uri_list', url: 'https://source.example.com/sub' } });
  expect(created.status()).toBe(201);
  const id = (await created.json()).source.id;
  await navigate(page, /^订阅源$|^Sources$/);
  const row = page.locator('tbody tr').filter({ hasText: 'Source delete fixture' });
  const remove = row.getByRole('button', { name: /^删除$|^Delete$/ });
  await remove.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText('Source delete fixture');
  await expect(dialog.getByRole('button', { name: /^取消$|^Cancel$/ })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(dialog).toHaveCount(0);
  await expect(remove).toBeFocused();
  expect((await api.get(`/api/v1/sources/${id}`)).status()).toBe(200);
  await remove.click();
  await dialog.getByRole('button', { name: /^删除$|^Delete$/ }).click();
  await expect(dialog).toHaveCount(0);
  await expect(row).toHaveCount(0);
  expect((await api.get(`/api/v1/sources/${id}`)).status()).toBe(404);
});
