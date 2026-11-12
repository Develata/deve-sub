import { test as base, expect, type Page } from '@playwright/test';
import { resolve } from 'path';
import { readFileSync, readdirSync, writeFileSync } from 'fs';
import { createHash } from 'crypto';
import { execFileSync } from 'child_process';
import { NormalServer } from './server';

const root = resolve(__dirname, '../../..');
export const test = base.extend<{ instance: NormalServer }>({
  instance: async ({ browser }, use, info) => {
    const binary = process.env.DEVE_SUB_BINARY ?? resolve(root, 'target/release/deve-sub');
    const web = process.env.DEVE_SUB_WEB_DIST ?? resolve(root, 'apps/web/dist');
    const hash = (path: string) => createHash('sha256').update(readFileSync(path)).digest('hex');
    const files = readdirSync(web, { recursive: true, withFileTypes: true }).filter(file => file.isFile())
      .map(file => resolve(file.parentPath, file.name)).sort();
    const provenance = { at: new Date().toISOString(), base: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root }).toString().trim(),
      platform: `${process.platform}/${process.arch}`, browser: browser.version(),
      browserExecutable: process.env.DEVE_SUB_BROWSER ?? 'Playwright pinned browser',
      binarySha256: hash(binary), web: files.map(path => ({ path: path.slice(web.length + 1), sha256: hash(path) })) };
    writeFileSync(info.outputPath('candidate-artifacts.json'), JSON.stringify(provenance, null, 2));
    const server = new NormalServer(binary, web, info.outputPath('server-logs'));
    try { await server.start(); await use(server); } finally { await server.close(); }
  },
  baseURL: async ({ instance }, use) => use(instance.url),
  page: async ({ page }, use, info) => {
    const errors: string[] = [], consoleErrors: string[] = [];
    const writes: { method: string; path: string; status: number }[] = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()); });
    page.on('response', response => {
      const request = response.request(), path = new URL(response.url()).pathname;
      if (path.startsWith('/api/v1/') && !['GET', 'HEAD'].includes(request.method()))
        writes.push({ method: request.method(), path, status: response.status() });
    });
    try { await use(page); } finally {
      await info.attach('browser-writes', { body: JSON.stringify(writes, null, 2), contentType: 'application/json' });
      await info.attach('browser-errors', { body: JSON.stringify({ errors, consoleErrors }), contentType: 'application/json' });
      expect(errors, 'uncaught browser exceptions').toEqual([]);
    }
  },
});
export { expect };

/** All setup/authentication writes originate from visible controls. */
export async function setupInBrowser(page: Page) {
  await page.goto('/');
  await expect(page).toHaveTitle(/Deve Sub/);
  await page.locator('input[autocomplete=username]').fill('normal-admin');
  await page.locator('input[autocomplete=new-password]').nth(0).fill('SyntheticPassword2026');
  await page.locator('input[autocomplete=new-password]').nth(1).fill('SyntheticPassword2026');
  await page.getByRole('button', { name: /^创建管理员$|^Create Admin$/ }).click();
  await expect(page.locator('input[autocomplete=current-password]')).toBeVisible();
  await page.locator('input[autocomplete=username]').fill('normal-admin');
  await page.locator('input[autocomplete=current-password]').fill('SyntheticPassword2026');
  await page.getByRole('button', { name: /^登录$|^Sign In$|^Login$/ }).click();
  await expect(page.locator('main')).toBeVisible();
  await page.getByRole('button', { name: 'EN', exact: true }).click();
}

export async function navigate(page: Page, label: RegExp) {
  const sidebar = page.locator('aside');
  await expect(sidebar).toHaveCSS('display', /^(none|flex)$/);
  if (!(await sidebar.isVisible())) await page.getByRole('button', { name: /^菜单$|^Menu$/ }).click();
  await sidebar.getByText(label, { exact: true }).click();
}

export async function importInBrowser(page: Page, content: string, format = 'mihomo_yaml') {
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.getByRole('button', { name: /^导入节点$|^Import Nodes$/ }).click();
  const modal = page.getByRole('dialog');
  await modal.locator('#node-import-format').selectOption(format);
  await modal.locator('#node-import-content').fill(content);
  const response = page.waitForResponse(r => r.url().endsWith('/nodes/import') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /^导入节点$|^Import Nodes$/ }).click();
  expect((await response).status()).toBe(200);
  await modal.getByRole('button', { name: /^完成$|^Done$/ }).click();
}

export async function templateInBrowser(page: Page, name: string, selector: object, profiles = ['mihomo', 'sing-box', 'xray', 'v2ray', 'json', 'uri_list']) {
  await navigate(page, /^模板$|^Templates$/);
  await page.getByRole('button', { name: /^新建模板$|^New Template$/ }).click();
  const modal = page.locator('.node-dialog-panel');
  await modal.locator('input[type=text]').nth(0).fill(name);
  await modal.locator('input[type=text]').nth(1).fill('Synthetic normal acceptance');
  await modal.getByRole('textbox', { name: 'Clash routing YAML' }).fill(JSON.stringify({
    apiVersion: 'deve-sub.io/v1', kind: 'SubscriptionTemplate', metadata: { name },
    spec: { targetProfiles: profiles, nodeSelector: selector, proxyGroups: [], rules: [] },
  }, null, 2));
  const response = page.waitForResponse(r => r.url().endsWith('/templates') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  const result = await response;
  expect(result.status()).toBe(201);
  await expect(modal).toHaveCount(0);
  return (await result.json()).template;
}
