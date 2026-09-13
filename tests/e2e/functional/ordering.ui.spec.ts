import { test, expect, navigate, templateYaml } from './fixture';
import type { Route } from '@playwright/test';

test('FUNC-LIST-ORDER an old failed response cannot replace a newer successful list', async ({ api, page }) => {
  expect((await api.post('/api/v1/sources', { data: { name: 'Current-source', source_type: 'uri_list', url: 'https://example.com/sub' } })).status()).toBe(201);
  const snapshot = await (await api.get('/api/v1/sources')).json();
  let held: Route | undefined;
  let requests = 0;
  await page.route('**/api/v1/sources', async route => {
    if (++requests === 2) { held = route; return; }
    await route.fulfill({ json: snapshot });
  });
  await navigate(page, /^订阅源$|^Sources$/);
  await expect(page.locator('tbody tr')).toHaveCount(1);
  const refresh = page.locator('main').getByRole('button', { name: /^刷新$|^Refresh$/ }).first();
  await refresh.click();
  await expect.poll(() => held !== undefined).toBeTruthy();
  await refresh.click();
  await expect(page.locator('tbody tr')).toHaveCount(1);
  const failed = page.waitForResponse(r => r.url().endsWith('/api/v1/sources') && r.status() === 500);
  await held!.fulfill({ status: 500, json: { code: 'storage_error', message: 'Fixture stale failure' } });
  await failed;
  // Flush browser rendering after the old response, then verify the newer state.
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(page.locator('tbody tr')).toHaveCount(1);
  await expect(page.locator('main')).not.toContainText('Fixture stale failure');
});

test('FUNC-TEMPLATE-ORDER late template A cannot overwrite the editor or save of B', async ({ api, page }) => {
  const templates = [];
  for (const name of ['Template-A', 'Template-B']) {
    const response = await api.post('/api/v1/templates', { data: { name, spec_yaml: templateYaml(name) } });
    expect(response.status()).toBe(201); templates.push((await response.json()).template);
  }
  const aVersions = await (await api.get(`/api/v1/templates/${templates[0].id}/versions/active`)).json();
  let held: Route | undefined;
  await page.route(`**/api/v1/templates/${templates[0].id}/versions/active`, route => { held = route; });
  await navigate(page, /^模板$|^Templates$/);
  await page.locator('tr').filter({ hasText: 'Template-A' }).getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  const modal = page.locator('div.fixed');
  await expect.poll(() => held !== undefined).toBeTruthy();
  await expect(modal.getByRole('button', { name: /^保存$|^Save$/ })).toBeDisabled();
  await modal.getByRole('button', { name: /^取消$|^Cancel$/ }).click();
  await page.locator('tr').filter({ hasText: 'Template-B' }).getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await expect(modal.locator('textarea')).toHaveValue(templateYaml('Template-B'));
  const late = page.waitForResponse(r => r.url().endsWith(`/templates/${templates[0].id}/versions/active`));
  await held!.fulfill({ json: aVersions });
  await late;
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(modal.locator('textarea')).toHaveValue(templateYaml('Template-B'));
  const saved = page.waitForResponse(r => r.url().endsWith(`/templates/${templates[1].id}`) && r.request().method() === 'PUT');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await saved).status()).toBe(200);
  const versions = (await (await api.get(`/api/v1/templates/${templates[1].id}/versions`)).json()).versions;
  expect(versions.find((v: { is_active: boolean }) => v.is_active).spec_yaml).toBe(templateYaml('Template-B'));
});
