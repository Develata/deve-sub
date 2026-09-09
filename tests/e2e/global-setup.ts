import { existsSync } from 'fs';
import { join, resolve } from 'path';
import { FRESH_PORT, SEEDED_PORT, RUN_ID } from './runtime-config';
import { Servers } from './server-lifecycle';

const REPO_ROOT = resolve(__dirname, '..', '..');
const BINARY = process.env.DEVE_SUB_BINARY ?? join(REPO_ROOT, 'target', 'release', 'deve-sub');
const WEB_DIST = process.env.DEVE_SUB_WEB_DIST ?? join(REPO_ROOT, 'apps', 'web', 'dist');
const ADMIN_USER = 'admin';
const ADMIN_PASS = 'TestPassword12345';

async function apiCall(port: number, method: string, path: string, body?: unknown, cookie?: string): Promise<{ status: number; data: any; cookie?: string }> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };
  if (cookie) headers['Cookie'] = cookie;
  if (method === 'POST') headers['Origin'] = `http://127.0.0.1:${port}`;

  const res = await fetch(`http://127.0.0.1:${port}/api/v1${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  const setCookie = res.headers.get('set-cookie');
  const data = res.status === 204 ? null : await res.json().catch(() => null);
  return { status: res.status, data, cookie: setCookie?.split(';')[0] };
}

async function seedServer(port: number): Promise<void> {
  // 1. Create admin via setup endpoint.
  const setupRes = await apiCall(port, 'POST', '/auth/setup', {
    username: ADMIN_USER,
    password: ADMIN_PASS,
  });
  if (setupRes.status !== 201) throw new Error(`setup failed: ${setupRes.status} ${JSON.stringify(setupRes.data)}`);

  // 2. Login to get session cookie.
  const loginRes = await apiCall(port, 'POST', '/auth/login', {
    username: ADMIN_USER,
    password: ADMIN_PASS,
  });
  if (loginRes.status !== 200) throw new Error(`login failed: ${loginRes.status}`);
  const cookie = loginRes.cookie!;
  if (!cookie) throw new Error('no session cookie returned from login');

  // 3. Create a template.
  const SPEC_YAML = [
    'apiVersion: deve-sub.io/v1',
    'kind: SubscriptionTemplate',
    '',
    'metadata:',
    '  name: default-mihomo',
    '  description: Default Mihomo template',
    '  version: 1',
    '',
    'spec:',
    '  targetProfiles:',
    '    - mihomo',
    '  variables: {}',
    '  nodeSelector:',
    '    mode: dynamic',
    '  proxyGroups: []',
    '  rules: []',
    '  dns: {}',
    '  tun: {}',
    '  output: {}',
  ].join('\n');

  const tmplRes = await apiCall(port, 'POST', '/templates', {
    name: 'default-mihomo',
    description: 'Default Mihomo template',
    spec_yaml: SPEC_YAML,
  }, cookie);
  if (tmplRes.status !== 201) throw new Error(`template create failed: ${tmplRes.status} ${JSON.stringify(tmplRes.data)}`);
  const templateId = tmplRes.data.template.id;

  // 4. Create a subscription.
  const subRes = await apiCall(port, 'POST', '/subscriptions', {
    name: 'test-sub',
    slug: 'test-sub',
    template_id: templateId,
    profile: 'mihomo',
    node_selection: { mode: 'dynamic' },
  }, cookie);
  if (subRes.status !== 201) throw new Error(`subscription create failed: ${subRes.status} ${JSON.stringify(subRes.data)}`);

  // 5. Create a source.
  const srcRes = await apiCall(port, 'POST', '/sources', {
    name: 'test-source',
    source_type: 'uri_list',
    url: 'https://example.com/sub.txt',
    auto_update: false,
    update_interval_secs: 3600,
    keep_on_fail: true,
  }, cookie);
  if (srcRes.status !== 201) throw new Error(`source create failed: ${srcRes.status} ${JSON.stringify(srcRes.data)}`);

  // 6. Import 10,000 nodes for UI-008.
  const lines: string[] = [];
  for (let i = 0; i < 10000; i++) {
    lines.push(`trojan://pass-${i}@host-${i}.example.com:443#Node-${String(i).padStart(4, '0')}`);
  }
  const importRes = await apiCall(port, 'POST', '/nodes/import', {
    content: lines.join('\n'),
    source_type: 'uri_list',
  }, cookie);
  if (importRes.status !== 200) throw new Error(`node import failed: ${importRes.status} ${JSON.stringify(importRes.data)}`);

  console.log(`[seed] port ${port}: admin created, template + subscription + source + 10k nodes seeded`);
}

export default async function globalSetup(): Promise<() => Promise<void>> {
  if (!existsSync(BINARY)) throw new Error(`binary not found: ${BINARY}`);
  if (!existsSync(join(WEB_DIST, 'index.html'))) throw new Error(`index.html not found in ${WEB_DIST}`);
  const servers = new Servers(BINARY, WEB_DIST, join(__dirname, 'test-results', RUN_ID, 'server-logs'));
  try {
    await servers.start(FRESH_PORT);
    await servers.start(SEEDED_PORT);
    await seedServer(SEEDED_PORT);
    console.log(`[setup] run ${RUN_ID}: fresh=${FRESH_PORT}, seeded=${SEEDED_PORT}`);
    // Keeping handles in this closure avoids PID reuse and shared PID files.
    return () => servers.stop();
  } catch (error) {
    await servers.stop();
    throw error;
  }
}
