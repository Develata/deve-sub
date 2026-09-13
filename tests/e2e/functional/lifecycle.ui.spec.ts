import { test, expect, navigate, templateYaml } from './fixture';

test('FUNC-PAGINATION sources, templates and subscriptions remain reachable beyond the first page', async ({ api, page }) => {
  const templates: string[] = [];
  for (let i = 0; i < 51; i++) {
    const name = `Fixture-${String(i).padStart(2, '0')}`;
    const response = await api.post('/api/v1/templates', { data: { name, spec_yaml: templateYaml(name) } });
    expect(response.status()).toBe(201); templates.push((await response.json()).template.id);
    expect((await api.post('/api/v1/subscriptions', { data: { name, slug: `fixture-${i}`, template_id: templates[0], profile: 'mihomo', node_selection: { mode: 'dynamic' } } })).status()).toBe(201);
    if (i < 21) expect((await api.post('/api/v1/sources', { data: { name, source_type: 'uri_list', url: `https://source-${i}.example.com/sub` } })).status()).toBe(201);
  }
  for (const [label, count] of [[/^订阅源$|^Sources$/, 21], [/^模板$|^Templates$/, 51], [/^长期订阅$|^Subscriptions$/, 51]] as const) {
    await navigate(page, label);
    await page.getByRole('button', { name: /加载更多|Load more/i }).click();
    await expect(page.locator('tbody tr')).toHaveCount(count);
  }
  await page.getByRole('button', { name: /^添加订阅$|^Add Subscription$/ }).click();
  await expect(page.locator('div.fixed select').first().locator('option')).toHaveCount(51);
});

test('FUNC-SOURCE-JOBS overlapping refreshes retain independent state and failure recovery', async ({ api, page }) => {
  const ids: string[] = [];
  for (const name of ['Source-A', 'Source-B']) {
    const r = await api.post('/api/v1/sources', { data: { name, source_type: 'uri_list', url: 'https://example.com/sub' } });
    expect(r.status()).toBe(201); ids.push((await r.json()).source.id);
  }
  let finishA = false;
  let finishB = false;
  for (const [i, id] of ids.entries()) {
    await page.route(`**/api/v1/sources/${id}/refresh`, route => route.fulfill({ status: 202, contentType: 'application/json', body: JSON.stringify({ job_id: `fixture-job-${i}`, source_id: id, status: 'running' }) }));
  }
  // Only job transport is controlled; real UI state and source lists are used.
  await page.route('**/api/v1/sources/refresh-jobs/*', route => {
    const a = route.request().url().endsWith('0');
    const done = a ? finishA : finishB;
    return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({
      id: a ? 'fixture-job-0' : 'fixture-job-1', source_id: ids[a ? 0 : 1], status: done ? (a ? 'completed' : 'failed') : 'running',
      phase: 'fetching', started_at: '2026-01-01T00:00:00Z', duplicate_nodes: 0, new_nodes: 0, reactivated_nodes: 0, missing_nodes: 0, not_modified: false,
      error_message: done && !a ? 'Fixture refresh failure' : null,
    }) });
  });
  await navigate(page, /^订阅源$|^Sources$/);
  const a = page.locator('tr').filter({ hasText: 'Source-A' });
  const b = page.locator('tr').filter({ hasText: 'Source-B' });
  await a.getByRole('button', { name: /^刷新$|^Refresh$/ }).click();
  await b.getByRole('button', { name: /^刷新$|^Refresh$/ }).click();
  await expect(a.getByRole('button', { name: /加载|Loading/ })).toBeDisabled();
  await expect(b.getByRole('button', { name: /加载|Loading/ })).toBeDisabled();
  finishA = true;
  await expect(a.getByRole('button', { name: /^刷新$|^Refresh$/ })).toBeEnabled();
  await expect(b.getByRole('button', { name: /加载|Loading/ })).toBeDisabled();
  finishB = true;
  await expect(page.getByRole('status')).toContainText('Fixture refresh failure');
  await expect(b.getByRole('button', { name: /^刷新$|^Refresh$/ })).toBeEnabled();
  await expect(page.locator('tbody tr')).toHaveCount(2);
});

test('FUNC-REQUEST-TIMEOUT a stalled request releases the UI after the deadline', async ({ api, page }) => {
  await api.get('/api/v1/auth/me');
  await page.route('**/api/v1/nodes?*', async () => { /* Keep transport pending until the real AbortSignal fires. */ });
  await navigate(page, /^节点管理$|^Nodes$/);
  await expect(page.locator('main')).toContainText(/TimeoutError|AbortError|aborted/i, { timeout: 40_000 });
});
