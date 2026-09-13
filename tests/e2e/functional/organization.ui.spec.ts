import { test, expect, importNodes, navigate, createTemplate, templateYaml } from './fixture';

test('FUNC-TAG-EDIT existing membership loads and can be cleared', async ({ api, page }) => {
  const [node] = await importNodes(api);
  const tag = (await (await api.post('/api/v1/tags', { data: { name: 'Production' } })).json()).tag;
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.id] } })).status()).toBe(204);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.locator(`[data-node-row="${node}"]`).getByRole('button', { name: /^标签$|^Tags$/ }).click();
  const label = page.locator('label').filter({ hasText: 'Production' });
  await expect(label.getByRole('checkbox')).toBeChecked();
  await label.getByRole('checkbox').uncheck();
  const saved = page.waitForResponse(r => r.url().endsWith(`/nodes/${node}/tags`) && r.request().method() === 'PUT');
  await page.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await saved).status()).toBe(204);
  expect((await (await api.get(`/api/v1/nodes/${node}`)).json()).node.tags).toEqual([]);
});

test('FUNC-TAG-MANAGE create, rename, batch assignment, filter and delete', async ({ api, page }, info) => {
  const nodes = await importNodes(api);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.getByRole('button', { name: /管理标签|Manage tags/ }).click();
  let manager = page.getByRole('dialog', { name: /管理标签|Manage tags/ });
  await manager.getByRole('textbox', { name: /新标签名称|New tag name/ }).fill('Japan');
  await manager.locator('input[type=color]').fill('#2563eb');
  const created = page.waitForResponse(r => r.url().endsWith('/api/v1/tags') && r.request().method() === 'POST');
  await manager.getByRole('button', { name: /^创建标签$|^Create Tag$/ }).click();
  const tag = (await (await created).json()).tag;
  await manager.locator(`[data-tag-id="${tag.id}"]`).getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await manager.getByRole('textbox', { name: /新标签名称|New tag name/ }).fill('日本 · 常用');
  const renamed = page.waitForResponse(r => r.url().endsWith(`/tags/${tag.id}`) && r.request().method() === 'PATCH');
  await manager.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await renamed).status()).toBe(200);
  await manager.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  for (const id of nodes.slice(0, 2)) await page.locator(`[data-node-row="${id}"]`).getByRole('checkbox').check();
  await page.getByRole('button', { name: /批量标签|Batch Tags/ }).click();
  const assignment = page.getByRole('dialog', { name: /标签|Tags/ });
  await assignment.locator('label').filter({ hasText: '日本 · 常用' }).getByRole('checkbox').check();
  const applied = page.waitForResponse(r => r.url().endsWith('/nodes/batch-tags') && r.request().method() === 'POST');
  await assignment.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await applied).status()).toBe(204);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.getByRole('combobox', { name: /按标签筛选|Filter by tag/ }).selectOption(tag.id);
  await expect(page.locator('[data-node-row]')).toHaveCount(2);
  await expect(page.locator('[data-node-row]').first().locator('.node-tag')).toHaveText('日本 · 常用');
  await expect(page.locator('[data-node-row]').first().locator('.node-tag-dot')).toHaveCSS('background-color', 'rgb(37, 99, 235)');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await page.screenshot({ path: info.outputPath('tags-filtered.png'), fullPage: true });
  await page.getByRole('button', { name: /管理标签|Manage tags/ }).click();
  manager = page.getByRole('dialog', { name: /管理标签|Manage tags/ });
  await manager.locator(`[data-tag-id="${tag.id}"]`).getByRole('button', { name: /^删除$|^Delete$/ }).click();
  const deleted = page.waitForResponse(r => r.url().endsWith(`/tags/${tag.id}`) && r.request().method() === 'DELETE');
  await manager.getByRole('alert').getByRole('button', { name: /^删除$|^Delete$/ }).click();
  expect((await deleted).status()).toBe(204);
  await manager.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await expect(page.locator('[data-node-row]')).toHaveCount(3);
  for (const id of nodes) expect((await (await api.get(`/api/v1/nodes/${id}`)).json()).node.tags).toEqual([]);
});

test('FUNC-OVERRIDE reopening editor preserves security and region fields', async ({ api, page }) => {
  const [node] = await importNodes(api);
  const original = { display_name: 'Before', region: 'JP', enabled: false, sni: 'sni.example.com',
    skip_cert_verify: false, fingerprint: 'chrome', sort_order: 9 };
  expect((await api.patch(`/api/v1/nodes/${node}/override`, { data: original })).status()).toBe(200);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.locator(`[data-node-row="${node}"]`).getByRole('button', { name: /覆盖|Override/ }).click();
  const modal = page.locator('div.fixed');
  await expect(modal.locator('input[type=text]').nth(2)).toHaveValue('sni.example.com');
  await modal.locator('input[type=text]').first().fill('After');
  const saved = page.waitForResponse(r => r.url().endsWith(`/nodes/${node}/override`) && r.request().method() === 'PATCH');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await saved).status()).toBe(200);
  expect((await (await api.get(`/api/v1/nodes/${node}/override`)).json()).override).toEqual({ ...original, display_name: 'After' });
});

test('FUNC-TEMPLATE-ROLLBACK history confirmation restores the previous specification', async ({ api, page }) => {
  const template = await createTemplate(api);
  const updated = await api.put(`/api/v1/templates/${template.id}`, { data: {
    name: template.name, description: 'v2', spec_yaml: templateYaml('version-two') } });
  expect(updated.status()).toBe(200);
  await navigate(page, /^模板$|^Templates$/);
  await page.getByRole('button', { name: /^版本$|^Versions$/ }).click();
  await page.locator('tr').filter({ hasText: 'v1' }).getByRole('button', { name: /^回滚$|^Rollback$/ }).click();
  const rolled = page.waitForResponse(r => r.url().endsWith(`/templates/${template.id}/rollback`) && r.request().method() === 'POST');
  await page.locator('div.fixed').getByRole('button', { name: /^回滚$|^Rollback$/ }).click();
  expect((await rolled).status()).toBe(200);
  expect((await (await api.get(`/api/v1/templates/${template.id}`)).json()).template.active_version).toBe(1);
  const versions = (await (await api.get(`/api/v1/templates/${template.id}/versions`)).json()).versions;
  expect(versions).toHaveLength(2);
  expect(versions.find((v: {is_active: boolean}) => v.is_active).spec_yaml).toContain('name: fixture-template');
});

test('FUNC-SUB-CREATE Web form creates a retrievable subscription', async ({ api, page }) => {
  await importNodes(api);
  await createTemplate(api);
  await navigate(page, /^长期订阅$|^Subscriptions$/);
  await page.getByRole('button', { name: /^添加订阅$|^Add Subscription$/ }).click();
  const modal = page.locator('div.fixed');
  await modal.locator('input[type=text]').nth(0).fill('Web subscription');
  await modal.locator('input[type=text]').nth(1).fill('web-subscription');
  const response = page.waitForResponse(r => r.url().endsWith('/api/v1/subscriptions') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await response).status()).toBe(201);
  const url = modal.locator('input[readonly]');
  await expect(url).toHaveValue(/\/sub\/[^/]+\/mihomo$/);
  expect((await api.get(await url.inputValue())).status()).toBe(200);
});

test('FUNC-TAG-MODES batch remove preserves other tags and replace can clear them', async ({ api, page }) => {
  const nodes = await importNodes(api, 2);
  const tags = [];
  for (const name of ['Keep', 'Remove']) tags.push((await (await api.post('/api/v1/tags', { data: { name } })).json()).tag);
  for (const id of nodes) expect((await api.put(`/api/v1/nodes/${id}/tags`, { data: { tag_ids: tags.map(t => t.id) } })).status()).toBe(204);
  for (const mode of ['remove', 'replace']) {
    await navigate(page, /^节点管理$|^Nodes$/);
    for (const id of nodes) await page.locator(`[data-node-row="${id}"]`).getByRole('checkbox').check();
    await page.getByRole('button', { name: /批量标签|Batch Tags/ }).click();
    const modal = page.getByRole('dialog', { name: /标签|Tags/ });
    await modal.getByRole('combobox').selectOption(mode);
    if (mode === 'remove') await modal.locator('label').filter({ hasText: 'Remove' }).getByRole('checkbox').check();
    const saved = page.waitForResponse(r => r.url().endsWith('/nodes/batch-tags') && r.request().method() === 'POST');
    await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
    expect((await saved).status()).toBe(204);
    for (const id of nodes) {
      const members = (await (await api.get(`/api/v1/nodes/${id}`)).json()).node.tags.map((t: { id: string }) => t.id);
      expect(members).toEqual(mode === 'remove' ? [tags[0].id] : []);
    }
  }
});
