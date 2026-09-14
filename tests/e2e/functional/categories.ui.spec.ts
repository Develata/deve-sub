import { test, expect, importNodes, navigate } from './fixture';
import type { APIRequestContext, Page, Route } from '@playwright/test';

const category = (page: Page, id: string) => page.locator(`[data-node-category="${id}"]`);
const rowCount = (page: Page, n: number) => expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', String(n));
async function createTag(api: APIRequestContext, name: string) {
  const response = await api.post('/api/v1/tags', { data: { name, color: '#2563eb' } });
  expect(response.status()).toBe(201);
  return (await response.json()).tag as { id: string; name: string };
}

test('FUNC-CATEGORY-VISIBLE every manual category including empty ones is directly selectable', async ({ api, page }, info) => {
  const nodes = await importNodes(api, 4);
  const japan = await createTag(api, '日本 · 常用');
  const streaming = await createTag(api, '流媒体');
  const empty = await createTag(api, '备用 · 暂无节点');
  for (const [i, tags] of [[0, [japan.id, streaming.id]], [1, [japan.id]], [2, [streaming.id]]] as const) {
    expect((await api.put(`/api/v1/nodes/${nodes[i]}/tags`, { data: { tag_ids: tags } })).status()).toBe(204);
  }
  await navigate(page, /^节点管理$|^Nodes$/);
  await rowCount(page, 4);
  // A screenshot before the assertion also captures the original missing-directory defect.
  await page.screenshot({ path: `/tmp/deve-sub-categories-${info.project.name}.png`, fullPage: true });
  await expect(category(page, empty.id)).toBeVisible();
  await expect(category(page, empty.id)).toContainText('0');
  await expect(category(page, japan.id)).toContainText('2');
  await expect(category(page, streaming.id)).toContainText('2');
  await category(page, japan.id).click();
  await rowCount(page, 2);
  await expect(category(page, japan.id)).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('searchbox').fill('Node-0');
  await rowCount(page, 1);
  await expect(category(page, japan.id)).toContainText('2');
  await page.getByRole('searchbox').fill('');
  await page.locator(`[data-node-row="${nodes[0]}"]`).getByRole('checkbox').check();
  await category(page, 'untagged').click();
  await rowCount(page, 1);
  await expect(page.locator(`[data-node-row="${nodes[3]}"]`)).toBeVisible();
  await expect(page.getByRole('button', { name: /批量标签|Batch Tags/ })).toHaveCount(0);
  await category(page, empty.id).click();
  await rowCount(page, 0);
  await expect(category(page, empty.id)).toHaveAttribute('aria-pressed', 'true');
  await category(page, streaming.id).focus();
  await page.keyboard.press('Enter');
  await rowCount(page, 2);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await page.screenshot({ path: `/tmp/deve-sub-categories-filtered-${info.project.name}.png`, fullPage: true });
  await navigate(page, /^节点管理$|^Nodes$/);
  await expect(category(page, empty.id)).toBeVisible();
  await category(page, japan.id).click();
  await rowCount(page, 2);
});

test('FUNC-CATEGORY-LIFECYCLE selected category survives rename and last membership removal until deletion', async ({ api, page }) => {
  const [node] = await importNodes(api, 1);
  const tag = await createTag(api, 'Before');
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.id] } })).status()).toBe(204);
  await navigate(page, /^节点管理$|^Nodes$/);
  await category(page, tag.id).click();
  await page.getByRole('button', { name: /管理标签|Manage tags/ }).click();
  let manager = page.getByRole('dialog', { name: /管理标签|Manage tags/ });
  await manager.locator(`[data-tag-id="${tag.id}"]`).getByRole('button', { name: /^编辑$|^Edit$/ }).click();
  await manager.getByRole('textbox', { name: /新标签名称|New tag name/ }).fill('After');
  await manager.getByRole('button', { name: /^保存$|^Save$/ }).click();
  await expect(category(page, tag.id)).toContainText('After');
  await manager.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await expect(category(page, tag.id)).toHaveAttribute('aria-pressed', 'true');
  await page.locator(`[data-node-row="${node}"]`).getByRole('button', { name: /^标签$|^Tags$/ }).click();
  let assignment = page.getByRole('dialog', { name: /标签管理|Manage Tags/ });
  await assignment.getByRole('checkbox').uncheck();
  await assignment.getByRole('button', { name: /^保存$|^Save$/ }).click();
  await rowCount(page, 0);
  await expect(category(page, tag.id)).toHaveAttribute('aria-pressed', 'true');
  await expect(category(page, tag.id)).toContainText('0');
  await page.getByRole('button', { name: /管理标签|Manage tags/ }).click();
  manager = page.getByRole('dialog', { name: /管理标签|Manage tags/ });
  await manager.locator(`[data-tag-id="${tag.id}"]`).getByRole('button', { name: /^删除$|^Delete$/ }).click();
  await manager.getByRole('alert').getByRole('button', { name: /^删除$|^Delete$/ }).click();
  await expect(category(page, tag.id)).toHaveCount(0);
  await manager.getByRole('button', { name: /^关闭$|^Close$/ }).click();
  await expect(category(page, 'all')).toHaveAttribute('aria-pressed', 'true');
  await rowCount(page, 1);
});

test('FUNC-CATEGORY-INLINE creating a tag then cancelling assignment retains the empty category', async ({ api, page }) => {
  const [node] = await importNodes(api, 1);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.locator(`[data-node-row="${node}"]`).getByRole('button', { name: /^标签$|^Tags$/ }).click();
  const modal = page.getByRole('dialog', { name: /标签管理|Manage Tags/ });
  await modal.getByPlaceholder(/新标签名称|New tag name/).fill('Later');
  const response = page.waitForResponse(r => r.url().endsWith('/tags') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /创建标签|Create Tag/ }).click();
  const tag = (await (await response).json()).tag;
  await modal.getByRole('button', { name: /^取消$|^Cancel$/ }).click();
  await expect(category(page, tag.id)).toBeVisible();
  await expect(category(page, tag.id)).toContainText('0');
  await category(page, tag.id).click();
  await rowCount(page, 0);
  expect((await (await api.get(`/api/v1/nodes/${node}`)).json()).node.tags).toEqual([]);
});

test('FUNC-CATEGORY-PAGES categories remain reachable before their nodes are loaded', async ({ api, page }) => {
  const nodes = await importNodes(api, 2);
  const tag = await createTag(api, 'Next page');
  expect((await api.put(`/api/v1/nodes/${nodes[1]}/tags`, { data: { tag_ids: [tag.id] } })).status()).toBe(204);
  const all = (await (await api.get('/api/v1/nodes?include_inactive=true')).json()).nodes;
  // Small controlled pages exercise the real UI boundary without 10,001 fixture rows.
  await page.route('**/api/v1/nodes?*', route => route.fulfill({ json:
    new URL(route.request().url()).searchParams.has('cursor')
      ? { nodes: all.filter((n: { id: string }) => n.id === nodes[1]), next_cursor: null }
      : { nodes: all.filter((n: { id: string }) => n.id === nodes[0]), next_cursor: 'fixture-next' },
  }));
  await navigate(page, /^节点管理$|^Nodes$/);
  await category(page, tag.id).click();
  await rowCount(page, 0);
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('0');
  await page.getByRole('button', { name: /^加载更多$|^Load More$/ }).click();
  await rowCount(page, 1);
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('1');
  await expect(page.locator(`[data-node-row="${nodes[1]}"]`)).toBeVisible();
  await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
  await rowCount(page, 0);
  await expect(category(page, tag.id)).toHaveAttribute('aria-pressed', 'true');
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('0');
  // A page requested before refresh must not be appended afterwards.
  let held: Route | undefined;
  await page.route('**/api/v1/nodes?*cursor=*', route => { held = route; });
  await page.getByRole('button', { name: /^加载更多$|^Load More$/ }).click();
  await expect.poll(() => held !== undefined).toBeTruthy();
  await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
  await expect(page.getByRole('button', { name: /^加载更多$|^Load More$/ })).toBeEnabled();
  const stale = page.waitForResponse(r => r.url().includes('cursor='));
  await held!.fulfill({ json: { nodes: all.filter((n: { id: string }) => n.id === nodes[1]), next_cursor: null } });
  await stale;
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await rowCount(page, 0);
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('0');
  await page.unroute('**/api/v1/nodes?*cursor=*');
  await page.getByRole('button', { name: /^加载更多$|^Load More$/ }).click();
  await rowCount(page, 1);
});

test('FUNC-CATEGORY-ORDER stale catalog success and failure cannot undo a newer rename', async ({ api, page }) => {
  const tag = await createTag(api, 'Old');
  await navigate(page, /^节点管理$|^Nodes$/);
  await category(page, tag.id).click();
  let held: Route | undefined;
  let holdNext = false;
  await page.route('**/api/v1/tags', route => {
    if (holdNext) { holdNext = false; held = route; return; }
    return route.continue();
  });
  for (const staleFailure of [false, true]) {
    const snapshot = await (await api.get('/api/v1/tags')).json();
    held = undefined;
    holdNext = true;
    await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
    await expect.poll(() => held !== undefined).toBeTruthy();
    const name = staleFailure ? 'Newest' : 'New';
    expect((await api.patch(`/api/v1/tags/${tag.id}`, { data: { name } })).status()).toBe(200);
    await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
    await expect(category(page, tag.id)).toContainText(name);
    const response = page.waitForResponse(r => r.url().endsWith('/api/v1/tags'));
    await held!.fulfill(staleFailure
      ? { status: 500, json: { code: 'storage_error', message: 'Fixture stale catalog failure' } }
      : { json: snapshot });
    await response;
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await expect(category(page, tag.id)).toContainText(name);
    await expect(category(page, tag.id)).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('main')).not.toContainText('Fixture stale catalog failure');
  }
});

test('FUNC-CATEGORY-RECOVERY catalog errors preserve nodes and existing categories and allow retry', async ({ api, page }) => {
  await importNodes(api, 1);
  const tag = await createTag(api, 'Still here');
  const fail = (route: Route) => route.fulfill({ status: 500, json: { code: 'storage_error', message: 'Fixture catalog unavailable' } });
  await page.route('**/api/v1/tags', fail);
  await navigate(page, /^节点管理$|^Nodes$/);
  await rowCount(page, 1);
  await expect(page.getByRole('alert')).toContainText('Fixture catalog unavailable');
  await expect(page.locator('main')).not.toContainText(/还没有手动分类|No manual categories yet/);
  await page.unroute('**/api/v1/tags', fail);
  await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
  await category(page, tag.id).click();
  await rowCount(page, 0);
  await page.route('**/api/v1/tags', fail);
  await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
  await expect(page.getByRole('alert')).toContainText('Fixture catalog unavailable');
  await expect(category(page, tag.id)).toHaveAttribute('aria-pressed', 'true');
  await expect(category(page, tag.id)).toBeVisible();
  await page.unroute('**/api/v1/tags', fail);
  await page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(category(page, tag.id)).toHaveAttribute('aria-pressed', 'true');
});

test('FUNC-CATEGORY-LAYOUT long names, many empty categories and themes remain usable', async ({ api, page }, info) => {
  const errors: string[] = [];
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  const long = await createTag(api, '长分类'.repeat(42));
  const tags = await Promise.all(Array.from({ length: 18 }, (_, i) => createTag(api, `分类-${String(i).padStart(2, '0')}`)));
  await navigate(page, /^节点管理$|^Nodes$/);
  await expect(page).toHaveTitle(/Deve Sub/);
  expect(new URL(page.url()).hostname).toBe('127.0.0.1');
  await expect(page.locator('h2')).toHaveText(/节点管理|Node Management/);
  await expect(page.locator('[data-node-category]')).toHaveCount(21);
  await category(page, long.id).click();
  await expect(category(page, long.id)).toContainText(long.name);
  await category(page, tags[17].id).click();
  await expect(category(page, tags[17].id)).toHaveAttribute('aria-pressed', 'true');
  const geometry = await page.locator('.node-category-options').evaluate(el => ({
    height: el.getBoundingClientRect().height, width: el.clientWidth, scrollWidth: el.scrollWidth,
    buttons: Array.from(el.querySelectorAll('button')).map(button => button.getBoundingClientRect().height),
  }));
  expect(geometry.height).toBeLessThanOrEqual(240);
  expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.width);
  expect(Math.min(...geometry.buttons)).toBeGreaterThanOrEqual(44);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  await navigate(page, /^设置$|^Settings$/);
  await page.getByRole('button', { name: /^深色$|^Dark$/ }).click();
  await expect(page.locator('html')).toHaveClass(/dark/);
  await navigate(page, /^节点管理$|^Nodes$/);
  await category(page, tags[0].id).click();
  await page.screenshot({ path: `/tmp/deve-sub-categories-dark-${info.project.name}.png`, fullPage: true });
  await page.getByRole('button', { name: /^EN$/ }).click();
  await expect(page.getByRole('region', { name: 'Manual categories' })).toBeVisible();
  await expect(category(page, 'untagged')).toContainText('Untagged');
  if (info.project.name === 'functional-desktop') {
    await page.setViewportSize({ width: 768, height: 1024 });
    await category(page, long.id).click();
    await expect(category(page, long.id)).toHaveAttribute('aria-pressed', 'true');
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
    await page.screenshot({ path: '/tmp/deve-sub-categories-tablet.png', fullPage: true });
  }
  expect(errors).toEqual([]);
});
