import { test, expect, importNodes, navigate, createTemplate, templateYaml } from './fixture';
import type { Route } from '@playwright/test';

test('FUNC-CLASH-EDITOR default example creates previews and reopens without V3 boilerplate', async ({ api, page }, info) => {
  const nodes = await importNodes(api);
  await navigate(page, /^模板$|^Templates$/);
  await page.getByRole('button', { name: /^新建模板$|^New Template$/ }).click();
  const modal = page.locator('div.fixed');
  const editor = modal.getByRole('textbox', { name: 'Clash routing YAML' });
  await expect(editor).toHaveValue(/proxy-groups:[\s\S]*MATCH,PROXY/);
  const authored = await editor.inputValue();
  await modal.locator('input[type=text]').first().fill('Web Clash');
  const saved = page.waitForResponse(r => r.url().endsWith('/templates') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await saved).status()).toBe(201);
  const row = page.locator('tr').filter({ hasText: 'Web Clash' });
  await row.getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await modal.getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await expect(modal.locator('textarea[readonly]')).toHaveValue(/MATCH,PROXY/);
  await expect(modal.locator('textarea[readonly]')).toHaveValue(/Node-0/);
  await page.screenshot({ path: info.outputPath('clash-preview.png'), fullPage: true });
  expect((await api.post('/api/v1/nodes/batch-enabled', { data: { node_ids: nodes, enabled: false } })).status()).toBe(200);
  await modal.getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await expect(modal).toContainText('no compatible nodes');
  await expect(modal.locator('textarea[readonly]')).toHaveCount(0);
  await modal.locator('select').first().selectOption('sing-box');
  await expect(modal.locator('textarea[readonly]')).toHaveCount(0);
  await modal.getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await expect(modal).toContainText('target_profiles');
  await expect(modal.locator('textarea[readonly]')).toHaveCount(0);
  await modal.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await row.getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await expect(editor).toHaveValue(authored);
  await page.screenshot({ path: info.outputPath('clash-editor.png'), fullPage: true });
  await expect(modal.getByRole('button', { name: /^保存$|^Save$/ })).toBeEnabled();
});

test('FUNC-TEMPLATE-HISTORY old active versions remain editable beyond 100 snapshots', async ({ api, page }) => {
  const template = await createTemplate(api);
  const path = `/api/v1/templates/${template.id}`;
  const first = (await (await api.get(`${path}/versions`)).json()).versions[0];
  for (let start = 0; start < 100; start += 10) {
    const batch = await Promise.all(Array.from({ length: 10 }, (_, i) => api.put(path, { data: {
      name: template.name, description: `history-${start + i}`, spec_yaml: templateYaml(`history-${start + i}`) } })));
    expect(batch.map(r => r.status())).toEqual(Array(10).fill(200));
  }
  expect((await api.post(`${path}/rollback`, { data: { version_id: first.id } })).status()).toBe(200);
  await navigate(page, /^模板$|^Templates$/);
  await page.getByRole('button', { name: /^版本$|^Versions$/ }).click();
  const modal = page.locator('div.fixed');
  await modal.getByRole('button', { name: /加载更多|Load more/i }).click();
  await expect(modal.getByRole('cell', { name: 'v1', exact: true })).toBeVisible();
  await modal.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await page.getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await expect(modal.locator('textarea')).toHaveValue(first.spec_yaml);
  const saved = page.waitForResponse(r => r.url().endsWith(path) && r.request().method() === 'PUT');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await (await saved).json()).version.version).toBe(102);
});

test('FUNC-TEMPLATE-DIALOG late history and preview cannot contaminate another template', async ({ api, page }) => {
  await importNodes(api);
  const templates = [];
  for (const name of ['Dialog-A', 'Dialog-B']) {
    const response = await api.post('/api/v1/templates', { data: { name, spec_yaml: templateYaml(name) } });
    expect(response.status()).toBe(201); templates.push((await response.json()).template);
  }
  const modal = page.locator('div.fixed');
  const aPath = `/api/v1/templates/${templates[0].id}`;
  const aHistory = (await (await api.get(`${aPath}/versions`)).json());
  const bHistory = (await (await api.get(`/api/v1/templates/${templates[1].id}/versions`)).json());
  let held: Route | undefined;
  await page.route(`**${aPath}/versions`, route => { held = route; });
  await navigate(page, /^模板$|^Templates$/);
  const row = (name: string) => page.locator('tr').filter({ hasText: name });
  await row('Dialog-A').getByRole('button', { name: /^版本$|^Versions$/ }).click();
  await expect.poll(() => held !== undefined).toBeTruthy();
  await modal.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await row('Dialog-B').getByRole('button', { name: /^版本$|^Versions$/ }).click();
  await expect(modal).toContainText(bHistory.versions[0].created_at);
  const late = page.waitForResponse(r => r.url().endsWith(`${aPath}/versions`));
  await held!.fulfill({ json: { ...aHistory, versions: [{ ...aHistory.versions[0], version: 999 }] } });
  await late;
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(modal).not.toContainText('v999');
  await modal.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  held = undefined;
  await page.route(`**${aPath}/preview?*`, route => { held = route; });
  await row('Dialog-A').getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await modal.getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await expect.poll(() => held !== undefined).toBeTruthy();
  await expect(modal.locator('select').first()).toBeDisabled();
  await modal.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await row('Dialog-B').getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await expect(modal.getByRole('button', { name: /^预览$|^Preview$/ })).toBeEnabled();
  const latePreview = page.waitForResponse(r => r.url().includes(`${aPath}/preview?`));
  await held!.fulfill({ json: { profile: 'mihomo', content: 'STALE-A', included_node_ids: [], excluded: [], warnings: [] } });
  await latePreview;
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await expect(modal).not.toContainText('STALE-A');
  await modal.getByRole('button', { name: /^预览$|^Preview$/ }).click();
  await expect(modal.locator('textarea[readonly]')).toHaveValue(/Node-0/);
});
