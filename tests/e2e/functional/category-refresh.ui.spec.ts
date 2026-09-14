import { test, expect, importNodes, navigate } from './fixture';
import type { APIRequestContext, Page, Route } from '@playwright/test';

const refresh = (page: Page) => page.getByRole('button', { name: /刷新分类与节点|Refresh categories and nodes/ });
const batchDisable = (page: Page) => page.getByRole('button', { name: /^批量禁用$|^Batch Disable$/ });
const checkbox = (page: Page, id: string) => page.locator(`[data-node-row="${id}"]`).getByRole('checkbox');
async function tag(api: APIRequestContext, name: string, node: string) {
  const response = await api.post('/api/v1/tags', { data: { name } });
  expect(response.status()).toBe(201);
  const tag = (await response.json()).tag;
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.id] } })).status()).toBe(204);
  return tag;
}

test('FUNC-CATEGORY-SELECTION refresh discards selected nodes that left the visible category', async ({ api, page }) => {
  const [node] = await importNodes(api, 1);
  const category = await tag(api, 'Before refresh', node);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.locator(`[data-node-category="${category.id}"]`).click();
  await checkbox(page, node).check();
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [] } })).status()).toBe(204);
  await refresh(page).click();
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '0');
  await expect(page.locator(`[data-node-category="${category.id}"]`)).toHaveAttribute('aria-pressed', 'true');
  await expect(batchDisable(page)).toHaveCount(0);
});

test('FUNC-CATEGORY-REFRESH-PAGE refreshing first page prunes selections from unloaded pages', async ({ api, page }) => {
  const ids = await importNodes(api, 2);
  const snapshot = (await (await api.get('/api/v1/nodes?include_inactive=true')).json()).nodes;
  await page.route('**/api/v1/nodes?*', route => route.fulfill({ json:
    new URL(route.request().url()).searchParams.has('cursor')
      ? { nodes: snapshot.filter((n: { id: string }) => n.id === ids[1]), next_cursor: null }
      : { nodes: snapshot.filter((n: { id: string }) => n.id === ids[0]), next_cursor: 'fixture-next' },
  }));
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.getByRole('button', { name: /^加载更多$|^Load More$/ }).click();
  await checkbox(page, ids[1]).check();
  await refresh(page).click();
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '1');
  await expect(batchDisable(page)).toHaveCount(0);
});

test('FUNC-CATEGORY-BATCH-ORDER a late batch completion preserves a newer selection', async ({ api, page }) => {
  const [a, b] = await importNodes(api, 2);
  const alpha = await tag(api, 'Alpha', a);
  const beta = await tag(api, 'Beta', b);
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.locator(`[data-node-category="${alpha.id}"]`).click();
  await checkbox(page, a).check();
  let held: Route | undefined;
  await page.route('**/api/v1/nodes/batch-enabled', route => { held = route; });
  await batchDisable(page).click();
  await expect.poll(() => held !== undefined).toBeTruthy();
  expect(held!.request().postDataJSON()).toEqual({ node_ids: [a], enabled: false });
  await page.locator(`[data-node-category="${beta.id}"]`).click();
  await checkbox(page, b).check();
  const completed = page.waitForResponse(r => r.url().endsWith('/nodes/batch-enabled'));
  await held!.continue();
  expect((await completed).status()).toBe(200);
  await expect(batchDisable(page)).toBeEnabled();
  await expect(checkbox(page, b)).toBeChecked();
  expect((await (await api.get(`/api/v1/nodes/${a}`)).json()).node.is_active).toBe(false);
  expect((await (await api.get(`/api/v1/nodes/${b}`)).json()).node.is_active).toBe(true);
});

test('FUNC-CATEGORY-NODE-RECOVERY a failed next page preserves visible rows and supports retry', async ({ api, page }) => {
  const ids = await importNodes(api, 2);
  const snapshot = (await (await api.get('/api/v1/nodes?include_inactive=true')).json()).nodes;
  let failNextPage = true;
  await page.route('**/api/v1/nodes?*', route => {
    const next = new URL(route.request().url()).searchParams.has('cursor');
    if (next && failNextPage) {
      failNextPage = false;
      return route.fulfill({ status: 500, json: { message: 'Fixture page unavailable' } });
    }
    return route.fulfill({ json: next
      ? { nodes: snapshot.filter((n: { id: string }) => n.id === ids[1]), next_cursor: null }
      : { nodes: snapshot.filter((n: { id: string }) => n.id === ids[0]), next_cursor: 'fixture-next' },
    });
  });
  await navigate(page, /^节点管理$|^Nodes$/);
  await checkbox(page, ids[0]).check();
  await page.getByRole('button', { name: /^加载更多$|^Load More$/ }).click();
  await expect(page.locator('main')).toContainText('Fixture page unavailable');
  await expect(checkbox(page, ids[0])).toBeVisible();
  await expect(checkbox(page, ids[0])).toBeChecked();
  await page.getByRole('button', { name: /^加载更多$|^Load More$/ }).click();
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '2');
  await expect(page.locator('main')).not.toContainText('Fixture page unavailable');
  let failRefresh = true;
  await page.route('**/api/v1/nodes?include_inactive=true&limit=10000', route => {
    if (failRefresh) {
      failRefresh = false;
      return route.fulfill({ status: 500, json: { message: 'Fixture refresh unavailable' } });
    }
    return route.fulfill({ json: { nodes: snapshot, next_cursor: null } });
  });
  await refresh(page).click();
  await expect(page.getByRole('alert')).toContainText('Fixture refresh unavailable');
  await expect(checkbox(page, ids[0])).toBeChecked();
  await expect(page.locator('#nodes-scroll')).toHaveAttribute('data-total-rows', '2');
  await refresh(page).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(checkbox(page, ids[0])).toBeChecked();
});

test('FUNC-CATEGORY-BATCH-RESELECT same-ID reselection is newer intent while unchanged selection is cleared', async ({ api, page }) => {
  const [node] = await importNodes(api, 1);
  await navigate(page, /^节点管理$|^Nodes$/);
  let held: Route | undefined;
  await page.route('**/api/v1/nodes/batch-enabled', route => { held = route; });
  for (const reselect of [true, false]) {
    await checkbox(page, node).check();
    held = undefined;
    await batchDisable(page).click();
    await expect.poll(() => held !== undefined).toBeTruthy();
    if (reselect) {
      await checkbox(page, node).uncheck();
      await checkbox(page, node).check();
    }
    const completed = page.waitForResponse(r => r.url().endsWith('/nodes/batch-enabled'));
    await held!.continue();
    expect((await completed).status()).toBe(200);
    await expect(page.locator('main')).toContainText(/已禁用 1 个节点|Disabled 1 nodes/);
    if (reselect) {
      await expect(batchDisable(page)).toBeEnabled();
      await expect(checkbox(page, node)).toBeChecked();
    } else {
      await expect(batchDisable(page)).toHaveCount(0);
      await expect(checkbox(page, node)).not.toBeChecked();
    }
  }
});
