import { test, expect, importNodes, navigate } from './fixture';
import type { APIRequestContext, Page, Route } from '@playwright/test';

type Tag = { id: string; name: string; color: string | null };
type Node = { id: string; display_name: string; protocol: string; region: string | null;
  is_active: boolean; tags: Tag[]; [key: string]: unknown };
const rows = (page: Page) => page.locator('#nodes-scroll [data-node-row]');
const row = (page: Page, id: string) => page.locator(`[data-node-row="${id}"]`);
const refresh = (page: Page) => page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ });
const selectFiltered = (page: Page) => page.getByRole('button', { name: /^选择筛选结果$|^Select filtered$/ });
const clearSelection = (page: Page) => page.getByRole('button', { name: /^清除选择$|^Clear Selection$/ });
const category = (page: Page, id: string) => page.locator(`[data-node-category="${id}"]`);

async function snapshot(api: APIRequestContext): Promise<Node[]> {
  const response = await api.get('/api/v1/nodes?include_inactive=true&limit=10000');
  expect(response.status()).toBe(200);
  return (await response.json()).nodes;
}

async function createTag(api: APIRequestContext, name: string): Promise<Tag> {
  const response = await api.post('/api/v1/tags', { data: { name } });
  expect(response.status()).toBe(201);
  return (await response.json()).tag;
}

async function expectRows(page: Page, ids: string[]) {
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', String(ids.length));
  await expect.poll(() => rows(page).evaluateAll(elements => elements.map(el => el.getAttribute('data-node-row')))).toEqual(ids);
}

// Inspect the existing typed intent, so a correct count cannot hide wrong IDs
// or selection restricted to the small rendered window.
async function expectBatchIds(page: Page, ids: string[]) {
  const submitted = page.waitForRequest(request => request.url().endsWith('/api/v1/nodes/batch-enabled') && request.method() === 'POST');
  const completed = page.waitForResponse(response => response.url().endsWith('/api/v1/nodes/batch-enabled') && response.request().method() === 'POST');
  const reloaded = page.waitForResponse(response => new URL(response.url()).pathname === '/api/v1/nodes');
  await page.getByRole('button', { name: /^批量禁用$|^Batch Disable$/ }).click();
  const body = (await submitted).postDataJSON();
  expect(body.enabled).toBe(false);
  expect([...body.node_ids].sort()).toEqual([...ids].sort());
  expect((await completed).status()).toBe(200);
  await (await reloaded).finished();
  await expect(clearSelection(page)).toHaveCount(0);
}

test('FUNC-NODE-PROJECTION-SOURCE replacement and shrink keep current IDs; failed refresh retains a nonempty cursor', async ({ api, page }) => {
  const ids = await importNodes(api, 3);
  const nodes = await snapshot(api);
  const [a, b, c] = ids.map(id => nodes.find(node => node.id === id)!);
  let current = [a, b];
  let nextCursor: string | null = 'fixture-more';
  let failRefresh = false;
  const cursors: string[] = [];
  await page.route('**/api/v1/nodes?*', route => {
    const cursor = new URL(route.request().url()).searchParams.get('cursor');
    if (cursor) { cursors.push(cursor); return route.fulfill({ json: { nodes: [c], next_cursor: null } }); }
    if (failRefresh) {
      failRefresh = false;
      return route.fulfill({ status: 503, json: { message: 'Fixture projection refresh unavailable' } });
    }
    return route.fulfill({ json: { nodes: current, next_cursor: nextCursor } });
  });
  await navigate(page, /^节点管理$|^Nodes$/);
  await expectRows(page, [a.id, b.id]);
  await row(page, b.id).getByRole('checkbox').check();
  failRefresh = true;
  await refresh(page).click();
  await expect(page.getByRole('alert')).toContainText('Fixture projection refresh unavailable');
  await expectRows(page, [a.id, b.id]);
  await expect(row(page, b.id).getByRole('checkbox')).toBeChecked();
  const more = page.getByRole('button', { name: /^加载更多$|^Load More$/ });
  await expect(more).toBeEnabled();
  await more.click();
  await expectRows(page, [a.id, b.id, c.id]);
  expect(cursors).toEqual(['fixture-more']);
  await expect(row(page, b.id).getByRole('checkbox')).toBeChecked();
  await expect(page.getByRole('alert')).toHaveCount(0);

  // B changes from index 1 to index 0; a stale index would select C.
  await page.getByRole('searchbox').fill(b.display_name);
  current = [b, c]; nextCursor = null;
  await refresh(page).click();
  await expect(category(page, 'all').locator('.node-category-count')).toHaveText('2');
  await expectRows(page, [b.id]);
  await selectFiltered(page).click();
  await expectBatchIds(page, [b.id]);

  await page.getByRole('searchbox').fill('');
  current = [a, b];
  await refresh(page).click();
  await expectRows(page, [a.id, b.id]);
  await page.getByRole('searchbox').fill(b.display_name);
  current = [b];
  await refresh(page).click();
  await expect(category(page, 'all').locator('.node-category-count')).toHaveText('1');
  await expectRows(page, [b.id]);
  await selectFiltered(page).click();
  await expectBatchIds(page, [b.id]);
});

test('FUNC-NODE-PROJECTION-CONTENT identical indices publish changed row fields before the independent catalog arrives', async ({ api, page }) => {
  const [a, b] = await importNodes(api, 2);
  const tag = await createTag(api, 'Original category');
  expect((await api.put(`/api/v1/nodes/${a}/tags`, { data: { tag_ids: [tag.id] } })).status()).toBe(204);
  const source = await snapshot(api);
  let current = [a, b].map((id, index) => ({ ...source.find(node => node.id === id)!, region: index === 0 ? 'JP' : 'US' }));
  await page.route('**/api/v1/nodes?*', route => route.fulfill({ json: { nodes: current, next_cursor: null } }));
  let holdCatalog = false;
  let held: Route | undefined;
  await page.route('**/api/v1/tags', route => {
    if (holdCatalog) { holdCatalog = false; held = route; return; }
    return route.continue();
  });
  await navigate(page, /^节点管理$|^Nodes$/);
  await expectRows(page, [a, b]);
  await expect(row(page, a).getByRole('rowheader')).toContainText('Node-0');
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('1');
  expect((await api.patch(`/api/v1/tags/${tag.id}`, { data: { name: 'Renamed category' } })).status()).toBe(200);
  // Every facet and [0, 1] index stays equal: regions swap, tag ID/count stays.
  current = current.map((node, index) => ({ ...node, region: index === 0 ? 'US' : 'JP',
    ...(index === 0 ? { display_name: 'Renamed 東京', is_active: false,
      tags: [{ ...tag, name: 'Renamed category' }] } : {}) }));
  holdCatalog = true;
  await refresh(page).click();
  await expect.poll(() => held !== undefined).toBe(true);
  await expectRows(page, [a, b]);
  await expect(row(page, a).getByRole('rowheader')).toContainText('Renamed 東京');
  await expect(row(page, a).getByRole('rowheader')).toContainText('Renamed category');
  await expect(row(page, a).getByRole('cell').nth(2)).toHaveText('US');
  await expect(row(page, a).getByRole('cell').nth(3)).toHaveText(/^禁用$|^Disabled$/);
  await expect(row(page, b).getByRole('cell').nth(2)).toHaveText('JP');
  await expect(category(page, tag.id)).toContainText('Original category');
  await held!.continue();
  await expect(category(page, tag.id)).toContainText('Renamed category');
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('1');
  await expect(category(page, 'untagged').locator('.node-category-count')).toHaveText('1');
});

test('FUNC-NODE-PROJECTION-SELECT-NOW adjacent input and select use current filters and all loaded matches', async ({ api, page }) => {
  const ids = await importNodes(api, 50);
  await navigate(page, /^节点管理$|^Nodes$/);
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '50');
  await expect(rows(page)).toHaveCount(18);
  // Two events in one task exercise the interval before the next DOM commit.
  await page.evaluate(() => {
    const input = document.querySelector('input[type="search"]') as HTMLInputElement;
    const button = Array.from(document.querySelectorAll<HTMLButtonElement>('main button'))
      .find(button => /^(选择筛选结果|Select filtered)$/.test(button.textContent ?? ''))!;
    input.value = 'Node-12';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    button.click();
  });
  await expectRows(page, [ids[12]]);
  await expect(row(page, ids[12]).getByRole('checkbox')).toBeChecked();
  await expectBatchIds(page, [ids[12]]);

  await page.getByRole('searchbox').fill('');
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '50');
  await selectFiltered(page).click();
  await expectBatchIds(page, ids);
  await page.getByRole('searchbox').fill('Node-1');
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '11');
  await selectFiltered(page).click();
  await expectBatchIds(page, [ids[1], ...ids.slice(10, 20)]);
});

test('FUNC-NODE-PROJECTION-FILTERS empty-source dependencies and intersecting filters retain explicit selection', async ({ api, page }) => {
  const ids = await importNodes(api, 4);
  const tag = await createTag(api, 'Filter category');
  const source = await snapshot(api);
  const names = ['ALPHA 東京', 'alpha 大阪', 'Beta 東京', 'alpha 名古屋'];
  const data = ids.map((id, index) => ({ ...source.find(node => node.id === id)!, display_name: names[index],
    protocol: index < 2 ? 'Trojan' : 'Vless', region: index === 1 ? 'US' : 'JP',
    is_active: index % 2 === 0, tags: index < 2 ? [tag] : [] }));
  let current: Node[] = [];
  await page.route('**/api/v1/nodes?*', route => route.fulfill({ json: { nodes: current, next_cursor: null } }));
  await navigate(page, /^节点管理$|^Nodes$/);
  await expectRows(page, []);
  const search = page.getByRole('searchbox');
  const protocol = page.locator('main select').first();
  const region = page.getByRole('combobox', { name: /^地区$|^Region$/ });
  const status = page.getByRole('combobox', { name: /^状态$|^Status$/ });
  await search.fill('ALPHA');
  await status.selectOption('enabled');
  await category(page, tag.id).click();
  current = data;
  await refresh(page).click();
  await expectRows(page, [ids[0]]);
  await protocol.selectOption('Vless');
  await expectRows(page, []);
  await protocol.selectOption('Trojan');
  await expectRows(page, [ids[0]]);
  await region.selectOption('US');
  await expectRows(page, []);
  await status.selectOption('disabled');
  await expectRows(page, [ids[1]]);
  await search.fill('東京');
  await expectRows(page, []);
  await search.fill('ALPHA');
  await expectRows(page, [ids[1]]);
  await category(page, 'all').click();
  await region.selectOption('JP');
  await expectRows(page, []);
  await protocol.selectOption('Vless');
  await expectRows(page, [ids[3]]);
  await status.selectOption('');
  await search.fill('東京');
  await expectRows(page, [ids[2]]);
  await status.selectOption('enabled');
  await row(page, ids[2]).getByRole('checkbox').check();
  await protocol.selectOption('Trojan');
  await expectRows(page, [ids[0]]);
  await expect(clearSelection(page)).toBeVisible();
  await search.fill('no-such-node');
  await expectRows(page, []);
  await expect(clearSelection(page)).toBeVisible();
  await protocol.selectOption('');
  await status.selectOption('');
  await region.selectOption('');
  await search.fill('');
  await expectRows(page, ids);
  await expect(row(page, ids[2]).getByRole('checkbox')).toBeChecked();
  await category(page, 'untagged').click();
  await expectRows(page, ids.slice(2));
  await expect(clearSelection(page)).toHaveCount(0);
  await category(page, tag.id).click();
  await expectRows(page, ids.slice(0, 2));
  await expect(category(page, 'all').locator('.node-category-count')).toHaveText('4');
  await expect(category(page, tag.id).locator('.node-category-count')).toHaveText('2');
  await expect(category(page, 'untagged').locator('.node-category-count')).toHaveText('2');
});
