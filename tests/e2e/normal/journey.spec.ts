import { type Page } from '@playwright/test';
import { writeFileSync } from 'fs';
import { createHash } from 'crypto';
import { test, expect, setupInBrowser, navigate, importInBrowser, templateInBrowser } from './fixture';

const proxies = [
  { name: 'SOCKS 正常', type: 'socks5', server: '192.0.2.10', port: 1080, username: 'test+user', password: 'TEST:p@ss/#%' },
  { name: 'HTTP 正常', type: 'http', server: 'http.example', port: 8080, username: 'test-user', password: 'TEST_HTTP_PASSWORD' },
  { name: 'HTTPS IPv6', type: 'http', server: '2001:db8::20', port: 8443, tls: true, sni: 'certificate.example', 'skip-cert-verify': false },
];

async function openSubscription(page: Page, name: string, template: string, profile = 'json') {
  await navigate(page, /^长期订阅$|^Subscriptions$/);
  await page.getByRole('button', { name: /^添加订阅$|^Add Subscription$/ }).click();
  const modal = page.getByRole('dialog');
  await modal.locator('#subscription-name').fill(name);
  await modal.locator('#subscription-slug').fill(name);
  await modal.locator('#subscription-template').selectOption(template);
  await modal.locator('#subscription-profile').selectOption(profile);
  return modal;
}

async function saveAndCopy(page: Page) {
  const modal = page.getByRole('dialog');
  const response = page.waitForResponse(r => r.url().endsWith('/subscriptions') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await response).status()).toBe(201);
  const url = await modal.locator('#subscription-url').inputValue();
  await modal.getByRole('button', { name: /^复制$|^Copy$/ }).click();
  await expect(modal.getByRole('status')).toContainText(/已复制|Copied/);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(url);
  await modal.getByRole('button', { name: 'OK', exact: true }).click();
  return url;
}

async function delivered(page: Page, url: string, expected: string[]) {
  const response = await page.request.get(url);
  expect(response.status()).toBe(200);
  const nodes = await response.json();
  expect(nodes.map((node: { display_name: string }) => node.display_name).sort()).toEqual([...expected].sort());
  return nodes;
}

for (const width of [1280, 390]) {
  test(`blank browser journey imports saves copies refreshes and restarts at ${width}px`, async ({ page, instance }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await setupInBrowser(page);
    await importInBrowser(page, JSON.stringify({ proxies }));
    await importInBrowser(page, 'socks5h://test-user:TEST_REMOTE_PASSWORD@remote.example:1080#Remote-DNS', 'uri_list');
    const nodes = (await (await page.request.get('/api/v1/nodes')).json()).nodes;
    expect(nodes).toHaveLength(4);
    const byName = Object.fromEntries(nodes.map((node: any) => [node.display_name, node]));
    for (const expected of proxies) {
      const node = byName[expected.name];
      expect(node.protocol).toBe(expected.type.toUpperCase());
      expect(node.port).toBe(expected.port);
      expect(node.is_active).toBe(true);
      expect(node.host.replace(/^\[|\]$/g, '')).toBe(expected.server);
      expect(node.unsupported_reason ?? null).toBeNull();
    }
    expect(byName['HTTPS IPv6'].connection).toMatchObject({
      tls_enabled: true, skip_cert_verify: false, server_name: 'certificate.example',
    });
    expect(byName['Remote-DNS'].connection.destination_dns).toBe('remote');
    expect(byName['SOCKS 正常'].connection.authentication).toBe('user_password');
    await page.getByRole('button', { name: 'Node details HTTPS IPv6', exact: true }).click();
    await expect(page.getByRole('dialog')).toContainText('certificate.example');
    await page.screenshot({ path: info.outputPath('node-details.png') });
    await page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true }).click();

    const template = await templateInBrowser(page, 'fixed-template',
      { mode: 'fixed', nodeIds: [byName['SOCKS 正常'].id] });
    let modal = await openSubscription(page, 'inherited', template.id);
    await expect(modal.locator('#subscription-selection')).toHaveValue('inherit_template');
    const inherited = await saveAndCopy(page);
    modal = await openSubscription(page, 'fixed', template.id);
    await modal.locator('#subscription-selection').selectOption('fixed');
    await modal.getByRole('checkbox', { name: /HTTPS IPv6/ }).check();
    await page.screenshot({ path: info.outputPath('fixed-selection.png') });
    const fixed = await saveAndCopy(page);
    modal = await openSubscription(page, 'dynamic', template.id);
    await modal.locator('#subscription-selection').selectOption('dynamic');
    await modal.getByRole('button', { name: 'Add filter', exact: true }).click();
    await modal.getByLabel('Value', { exact: true }).fill('http');
    const dynamic = await saveAndCopy(page);
    const original = await delivered(page, inherited, ['SOCKS 正常']);
    expect(original[0].authentication).toEqual({ type: 'user_password', data: { username: 'test+user', password: 'TEST:p@ss/#%' } });
    const secure = await delivered(page, fixed, ['HTTPS IPv6']);
    expect(secure[0].tls).toMatchObject({ enabled: true, server_name: 'certificate.example', skip_cert_verify: false });
    await delivered(page, dynamic, ['HTTP 正常', 'HTTPS IPv6']);

    const outputs: object[] = [];
    for (const profile of ['mihomo', 'sing-box', 'xray', 'v2ray', 'uri_list']) {
      await openSubscription(page, `native-${profile}`, template.id, profile);
      const url = await saveAndCopy(page);
      const response = await page.request.get(url);
      expect(response.status()).toBe(200);
      const content = await response.body();
      writeFileSync(info.outputPath(`delivered-${profile}.txt`), content);
      outputs.push({ profile, bytes: content.length, sha256: createHash('sha256').update(content).digest('hex') });
    }
    await info.attach('delivered-artifacts', { body: JSON.stringify(outputs, null, 2), contentType: 'application/json' });

    for (const [name, mode] of [['inherited', 'inherit_template'], ['fixed', 'fixed'], ['dynamic', 'dynamic']]) {
      await page.locator('tr').filter({ hasText: name }).getByRole('button', { name: /^Edit$/ }).click();
      await expect(page.getByRole('dialog').locator('#subscription-selection')).toHaveValue(mode);
      if (mode === 'fixed') await expect(page.getByRole('dialog').getByRole('checkbox', { name: /HTTPS IPv6/ })).toBeChecked();
      if (mode === 'dynamic') await expect(page.getByRole('dialog').getByLabel('Value', { exact: true })).toHaveValue('http');
      await page.getByRole('dialog').getByRole('button', { name: 'Cancel', exact: true }).click();
    }
    await importInBrowser(page, 'http://later.example:8081#Later-HTTP', 'auto');
    await page.reload();
    await expect(page.locator('main')).toBeVisible();
    await delivered(page, inherited, ['SOCKS 正常']);
    await delivered(page, fixed, ['HTTPS IPv6']);
    await delivered(page, dynamic, ['HTTP 正常', 'HTTPS IPv6', 'Later-HTTP']);
    await instance.restart();
    await page.reload();
    await expect(page.locator('main')).toBeVisible();
    await delivered(page, inherited, ['SOCKS 正常']);
    await delivered(page, fixed, ['HTTPS IPv6']);
    await delivered(page, dynamic, ['HTTP 正常', 'HTTPS IPv6', 'Later-HTTP']);

    const restricted = await templateInBrowser(page, 'mihomo-only', { mode: 'dynamic' }, ['mihomo']);
    modal = await openSubscription(page, 'invalid-profile', restricted.id, 'xray');
    let response = page.waitForResponse(r => r.url().endsWith('/subscriptions') && r.request().method() === 'POST');
    await modal.getByRole('button', { name: 'Save', exact: true }).click();
    expect((await response).status()).toBe(422);
    await expect(modal.getByRole('alert')).toContainText('target_profiles');
    await expect(modal.locator('#subscription-url')).toHaveCount(0);
    await page.screenshot({ path: info.outputPath('incompatible-save.png') });
    await modal.getByRole('button', { name: 'Cancel', exact: true }).click();
    modal = await openSubscription(page, 'empty-fixed', template.id);
    await modal.locator('#subscription-selection').selectOption('fixed');
    response = page.waitForResponse(r => r.url().endsWith('/subscriptions') && r.request().method() === 'POST');
    await modal.getByRole('button', { name: 'Save', exact: true }).click();
    expect((await response).status()).toBe(422);
    await expect(modal.getByRole('alert')).toContainText('no compatible nodes');
    await expect(modal.locator('#subscription-url')).toHaveCount(0);
    await modal.getByRole('button', { name: 'Cancel', exact: true }).click();
    expect((await (await page.request.get('/api/v1/subscriptions')).json()).subscriptions).toHaveLength(8);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: info.outputPath('after-restart.png') });
  });
}
