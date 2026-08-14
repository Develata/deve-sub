import { spawn, spawnSync, ChildProcess } from 'child_process';
import { writeFileSync, mkdirSync, existsSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';

const BINARY = '/home/pwh/deve-sub/target/release/deve-sub';
const WEB_DIST = '/home/pwh/deve-sub/apps/web/dist';
const FRESH_PORT = 18080;
const SEEDED_PORT = 18081;
const ADMIN_USER = 'admin';
const ADMIN_PASS = 'TestPassword12345';

interface ServerHandle {
  proc: ChildProcess;
  dbDir: string;
  port: number;
}

const servers: ServerHandle[] = [];

function makeConfig(port: number, dbPath: string, dbDir: string): string {
  return JSON.stringify({
    product_name: 'Deve Sub',
    server: {
      bind: `127.0.0.1:${port}`,
      serve_web: true,
      web_dist_dir: WEB_DIST,
    },
    database: { path: dbPath },
    security: {
      master_key_path: join(dbDir, 'master.key'),
      allow_master_key_generation: true,
      session_ttl_secs: 86400,
      cookie_secure: false,
      max_login_attempts: 100,
      lockout_duration_secs: 1,
      trust_proxy_headers: false,
    },
    geoip: { mmdb_path: null },
  }, null, 2);
}

async function waitForHealth(port: number, timeoutMs = 30000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/health/live`);
      if (res.ok) return;
    } catch {
      // server not ready yet
    }
    await new Promise(r => setTimeout(r, 500));
  }
  throw new Error(`server on port ${port} did not become healthy within ${timeoutMs}ms`);
}

function startServer(port: number): ServerHandle {
  const dbDir = join(tmpdir(), `deve-sub-e2e-${port}-${Date.now()}`);
  mkdirSync(dbDir, { recursive: true });
  const dbPath = join(dbDir, 'deve-sub.db');
  const configPath = join(dbDir, 'config.json');
  writeFileSync(configPath, makeConfig(port, dbPath, dbDir));

  // Run migrations first (synchronous, must complete before serve).
  const { status } = spawnSync(BINARY, ['migrate', '--db-path', dbPath], {
    stdio: ['pipe', 'pipe', 'pipe'],
    timeout: 30000,
  });
  if (status !== 0) throw new Error(`migrate failed for port ${port}`);

  const proc = spawn(BINARY, ['serve', '--config', configPath], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, RUST_LOG: 'warn' },
  });

  proc.stdout?.on('data', (d) => { /* swallow */ });
  proc.stderr?.on('data', (d) => { /* swallow */ });

  servers.push({ proc, dbDir, port });
  return { proc, dbDir, port };
}

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

export default async function globalSetup(): Promise<void> {
  // Verify binary and web dist exist.
  if (!existsSync(BINARY)) throw new Error(`binary not found: ${BINARY}`);
  if (!existsSync(WEB_DIST)) throw new Error(`web dist not found: ${WEB_DIST}`);
  if (!existsSync(join(WEB_DIST, 'index.html'))) throw new Error(`index.html not found in web dist`);

  // Start fresh server (no admin) for UI-001.
  console.log('[setup] starting fresh server on port', FRESH_PORT);
  startServer(FRESH_PORT);
  await waitForHealth(FRESH_PORT);

  // Start seeded server for UI-002..010.
  console.log('[setup] starting seeded server on port', SEEDED_PORT);
  startServer(SEEDED_PORT);
  await waitForHealth(SEEDED_PORT);

  // Seed the second server.
  console.log('[setup] seeding server on port', SEEDED_PORT);
  await seedServer(SEEDED_PORT);

  console.log('[setup] both servers ready');

  const pidFile = join(tmpdir(), 'deve-sub-e2e-pids.json');
  writeFileSync(pidFile, JSON.stringify(servers.map(s => s.proc.pid)));
}
