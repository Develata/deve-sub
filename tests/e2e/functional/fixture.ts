import { test as base, expect, type APIRequestContext, type Page } from '@playwright/test';
import { createServer } from 'net';
import { randomInt } from 'crypto';
import { resolve } from 'path';
import { Servers } from '../server-lifecycle';

const root = resolve(__dirname, '../../..');
type Fixtures = { server: string; api: APIRequestContext; auditRetentionDays: number; auditRetentionEnv: number | undefined; oldAuditCount: number };

/** Every case owns its database/process/session; only race requests share state. */
export const test = base.extend<Fixtures>({
  auditRetentionDays: [90, { option: true }],
  auditRetentionEnv: [undefined, { option: true }],
  oldAuditCount: [0, { option: true }],
  server: async ({ auditRetentionDays, auditRetentionEnv, oldAuditCount }, use, info) => {
    const port = await new Promise<number>((resolve, reject) => {
      const socket = createServer();
      let attempts = 0;
      socket.on('error', (error: NodeJS.ErrnoException) => {
        if (error.code === 'EADDRINUSE' && ++attempts < 20) socket.listen(randomInt(20000, 30000), '127.0.0.1');
        else reject(error);
      });
      // Linux client ephemeral ports can claim a listen(0) allocation during
      // migration. Use a checked port below that range for test servers.
      socket.once('listening', () => {
        const port = (socket.address() as { port: number }).port;
        socket.close(error => error ? reject(error) : resolve(port));
      });
      socket.listen(randomInt(20000, 30000), '127.0.0.1');
    });
    const servers = new Servers(process.env.DEVE_SUB_BINARY ?? resolve(root, 'target/release/deve-sub'),
      process.env.DEVE_SUB_WEB_DIST ?? resolve(root, 'apps/web/dist'), info.outputPath('server-logs'));
    try { await servers.start(port, { auditRetentionDays, auditRetentionEnv, oldAuditCount }); await use(`http://127.0.0.1:${port}`); }
    finally { await servers.stop(); }
  },
  baseURL: async ({ server }, use) => use(server),
  page: async ({ page }, use, info) => {
    const exceptions: string[] = [];
    const consoleMessages: string[] = [];
    page.on('pageerror', error => exceptions.push(error.message));
    page.on('console', message => {
      if (['warning', 'error'].includes(message.type())) consoleMessages.push(message.text());
    });
    try { await use(page); }
    finally {
      if (consoleMessages.length) await info.attach('browser-console', { body: JSON.stringify(consoleMessages), contentType: 'application/json' });
      expect(exceptions, 'unexpected JavaScript exceptions').toEqual([]);
    }
  },
  api: async ({ playwright, server }, use) => {
    const api = await playwright.request.newContext({ baseURL: server, timeout: 10_000,
      extraHTTPHeaders: { Origin: server } });
    try {
      const credentials = { username: 'fixture-admin', password: 'FixturePassword12345' };
      expect((await api.post('/api/v1/auth/setup', { data: credentials })).status()).toBe(201);
      expect((await api.post('/api/v1/auth/login', { data: credentials })).status()).toBe(200);
      await use(api);
    } finally { await api.dispose(); }
  },
  storageState: async ({ api }, use) => use(await api.storageState()),
});
export { expect };

export async function importNodes(api: APIRequestContext, count = 3) {
  const response = await api.post('/api/v1/nodes/import', { data: { source_type: 'uri_list',
    content: Array.from({ length: count }, (_, i) => `trojan://fixture@node-${i}.example.com:443#Node-${i}`).join('\n') } });
  expect(response.status()).toBe(200);
  return (await response.json()).outcomes.map((o: { data: string }) => o.data) as string[];
}

export const templateYaml = (name = 'fixture-template') => `apiVersion: deve-sub.io/v1
kind: SubscriptionTemplate
metadata:
  name: ${name}
  version: 1
spec:
  targetProfiles: [mihomo]
  nodeSelector: {mode: dynamic}
  proxyGroups: []
  rules: []
  dns: {}
  tun: {}
  output: {}
`;

export async function createTemplate(api: APIRequestContext) {
  const response = await api.post('/api/v1/templates', { data: {
    name: 'fixture-template', description: 'Fixture template', spec_yaml: templateYaml() } });
  expect(response.status()).toBe(201);
  return (await response.json()).template;
}

export async function navigate(page: Page, label: RegExp) {
  await page.goto('/');
  await expect(page.locator('main')).toBeVisible();
  const sidebar = page.locator('aside');
  if (!(await sidebar.isVisible())) await page.getByRole('button', { name: /菜单|menu/i }).click();
  await sidebar.getByText(label, { exact: true }).click();
}
