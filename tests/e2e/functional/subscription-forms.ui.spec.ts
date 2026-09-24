import { test, expect, navigate, createTemplate } from './fixture';

const subscriptions = /^长期订阅$|^Subscriptions$/;
const addSubscription = /^添加订阅$|^Add Subscription$/;
const save = /^保存$|^Save$/;
const cancel = /^取消$|^Cancel$/;

// OUT-016: malformed limits must not silently turn an existing cap into unlimited.
test('subscription quota preserves invalid drafts and only blank removes the limit', async ({ api, page }) => {
  const template = await createTemplate(api);
  const created = await api.post('/api/v1/subscriptions', { data: {
    name: 'Quota fixture', slug: 'quota-fixture', template_id: template.id,
    profile: 'mihomo', node_selection: { mode: 'dynamic' }, traffic_limit: 1024,
  } });
  expect(created.status()).toBe(201);
  const id = (await created.json()).subscription.id;
  await navigate(page, subscriptions);
  await page.locator('tr').filter({ hasText: 'Quota fixture' }).getByRole('button', { name: /编辑|Edit/ }).click();
  const modal = page.locator('div.fixed').filter({ has: page.locator('input') });
  const quota = modal.locator('input').nth(2);
  let writes = 0;
  page.on('request', request => { if (request.method() === 'PUT') writes++; });
  for (const value of ['-1', '1.5', '9223372036854775808', '18446744073709551616', '0']) {
    await quota.fill(value);
    await modal.getByRole('button', { name: save }).click();
    await expect(modal.getByRole('alert')).toContainText(/正整数|positive integer/);
    await expect(quota).toHaveValue(value);
    expect(writes).toBe(0);
    if (value === '0') await page.screenshot({ path: test.info().outputPath('quota-validation.png'), fullPage: true });
    expect((await (await api.get(`/api/v1/subscriptions/${id}`)).json()).subscription.traffic_limit).toBe(1024);
  }
  // Inspect raw JSON: JavaScript Number cannot represent this exact boundary.
  await quota.fill('9223372036854775807');
  const boundary = page.waitForResponse(response => response.request().method() === 'PUT' && response.url().endsWith(`/subscriptions/${id}`));
  await modal.getByRole('button', { name: save }).click();
  const boundaryResponse = await boundary;
  expect(boundaryResponse.status()).toBe(200);
  expect(boundaryResponse.request().postData()).toContain('"traffic_limit":9223372036854775807');
  expect(await boundaryResponse.text()).toContain('"traffic_limit":9223372036854775807');
  await expect(modal).toHaveCount(0);
  await page.locator('tr').filter({ hasText: 'Quota fixture' }).getByRole('button', { name: /编辑|Edit/ }).click();
  await expect(quota).toHaveValue('9223372036854775807');
  await quota.fill('2048');
  await modal.getByRole('button', { name: save }).click();
  await expect(modal).toHaveCount(0);
  expect((await (await api.get(`/api/v1/subscriptions/${id}`)).json()).subscription.traffic_limit).toBe(2048);
  await page.locator('tr').filter({ hasText: 'Quota fixture' }).getByRole('button', { name: /编辑|Edit/ }).click();
  await quota.fill('');
  await modal.getByRole('button', { name: save }).click();
  await expect(modal).toHaveCount(0);
  expect((await (await api.get(`/api/v1/subscriptions/${id}`)).json()).subscription.traffic_limit).toBeUndefined();
});

for (const kind of ['subscription', 'user'] as const) {
  test(`${kind} pending form cannot close or switch and failed save keeps the draft`, async ({ api, page }) => {
    if (kind === 'subscription') await createTemplate(api);
    await navigate(page, kind === 'subscription' ? subscriptions : /^用户$|^Users$/);
    const add = page.getByRole('button', { name: kind === 'subscription' ? addSubscription : /^新建用户$|^New User$/ });
    await add.click();
    const modal = page.locator('div.fixed').filter({ has: page.locator('input') });
    const inputs = modal.locator('input');
    await inputs.nth(0).fill('pending-fixture');
    await inputs.nth(1).fill(kind === 'subscription' ? 'pending-fixture' : 'FixturePassword12345');
    const endpoint = `/api/v1/${kind === 'subscription' ? 'subscriptions' : 'users'}`;
    let finish!: () => void;
    const held = new Promise<void>(resolve => { finish = resolve; });
    let writes = 0;
    await page.route(`**${endpoint}`, async route => {
      if (route.request().method() !== 'POST') return route.continue();
      writes++;
      if (writes > 1) return route.continue();
      await held;
      await route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'fixture_failure', message: 'Fixture save failure' }) });
    });
    try {
      const sent = page.waitForRequest(request => request.url().endsWith(endpoint) && request.method() === 'POST');
      await modal.getByRole('button', { name: save }).click();
      await sent;
      await expect(modal.getByRole('button', { name: cancel })).toBeDisabled();
      await expect(inputs.first()).toBeDisabled();
      await modal.dispatchEvent('click');
      await page.keyboard.press('Escape');
      // Exercise handlers too: disabled controls alone do not guard callbacks.
      await add.dispatchEvent('click');
      await expect(modal).toBeVisible();
      await expect(inputs.first()).toHaveValue('pending-fixture');
      expect(writes).toBe(1);
      finish();
      await expect(modal.getByRole('alert')).toHaveText('Fixture save failure');
      await expect(inputs.first()).toBeEnabled();
      await page.screenshot({ path: test.info().outputPath('failed-draft.png'), fullPage: true });
      await expect(inputs.first()).toHaveValue('pending-fixture');
      await modal.getByRole('button', { name: save }).click();
      if (kind === 'subscription') await expect(modal.locator('input[readonly]')).toBeVisible();
      else await expect(modal).toHaveCount(0);
      expect(writes).toBe(2);
    } finally { finish(); }
  });
}
