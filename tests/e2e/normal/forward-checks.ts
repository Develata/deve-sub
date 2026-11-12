import { expect, type APIRequestContext, type Page, type TestInfo } from '@playwright/test';
import { spawnSync } from 'child_process';
import { createHash } from 'crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'fs';
import { resolve } from 'path';
import { type ProxySample } from './forward-samples';

export const sha = (data: string | Buffer) => createHash('sha256').update(data).digest('hex');
export function checkDto(node: any, s: ProxySample) {
  expect(node.display_name).toBe(s.name);
  expect(node.protocol).toBe(s.family.startsWith('SOCKS') || s.family === 'socks5h' ? 'SOCKS5' : 'HTTP');
  expect(node.host.replace(/^\[|\]$/g, '')).toBe(s.host);
  expect(node.port).toBe(s.port);
  expect(node.is_active).toBe(true);
  expect(node.source_label).toBe('manual');
  expect(node.connection.authentication).toBe(s.username ? 'user_password' : 'none');
  expect(node.connection.tls_enabled).toBe(s.tls ? true : null);
  expect(node.connection.server_name).toBe(s.sni ?? null);
  expect(node.connection.skip_cert_verify).toBe(s.verify ?? null);
  expect(node.connection.destination_dns).toBe(s.dns ?? null);
  expect(JSON.stringify(node)).not.toContain('TEST:p@ss');
}

export function checkOutput(profile: string, text: string, rows: ProxySample[], ids: Record<string, string>) {
  let nodes: any[];
  if (profile === 'mihomo') {
    const parsed = spawnSync('python3', ['-c', 'import sys,yaml,json; print(json.dumps(yaml.safe_load(sys.stdin.read())))'], { input: text, encoding: 'utf8' });
    expect(parsed.status, parsed.stderr).toBe(0);
    nodes = JSON.parse(parsed.stdout).proxies;
  } else if (profile === 'uri_list') {
    nodes = text.trim().split('\n').map(line => new URL(line));
  } else {
    const value = JSON.parse(text);
    nodes = profile === 'json' ? value : value.outbounds;
  }
  expect(nodes).toHaveLength(rows.length);
  const name = (node: any) => profile === 'json' ? node.display_name : profile === 'mihomo' ? node.name
    : profile === 'uri_list' ? decodeURIComponent(node.hash.slice(1)) : node.tag;
  expect(nodes.map(name).sort()).toEqual(rows.map(s => s.name).sort());
  for (const s of rows) {
    const n = nodes.find(node => name(node) === s.name)!;
    if (profile === 'json') {
      expect(n.id).toBe(ids[s.name]);
      expect(n.protocol).toBe(s.family.startsWith('SOCKS') || s.family === 'socks5h' ? 'Socks5' : 'Http');
      expect(n.endpoint.port).toBe(s.port);
      expect(Object.values(n.endpoint.host)).toContain(s.host);
      expect(n.authentication).toEqual(s.username ? { type: 'user_password', data: { username: s.username, password: s.password } } : { type: 'none' });
      expect(n.tls?.enabled ?? false).toBe(s.tls);
      if (s.tls) expect(n.tls).toMatchObject({ server_name: s.sni, skip_cert_verify: s.verify });
      if (s.dns) expect(n.config).toMatchObject({ Socks5: { dns_resolution: s.dns } });
    } else if (profile === 'uri_list') {
      expect(n.protocol).toBe(`${s.family === 'SOCKS5' ? 'socks' : s.family.toLowerCase()}:`);
      expect(n.hostname.replace(/^\[|\]$/g, '')).toBe(s.host);
      expect(Number(n.port)).toBe(s.port);
      expect(decodeURIComponent(n.username)).toBe(s.username ?? '');
      expect(decodeURIComponent(n.password)).toBe(s.password ?? '');
      if (s.tls) { expect(n.searchParams.get('sni')).toBe(s.sni); expect(n.searchParams.get('skip-cert-verify')).toBe('0'); }
    } else if (profile === 'mihomo' || profile === 'sing-box') {
      expect(n.type).toBe(s.family === 'SOCKS5' ? profile === 'mihomo' ? 'socks5' : 'socks' : 'http');
      expect(n.server).toBe(s.host); expect(n.port ?? n.server_port).toBe(s.port);
      expect(n.username).toBe(s.username); expect(n.password).toBe(s.password);
      if (profile === 'mihomo') {
        expect(n.tls ?? false).toBe(s.tls);
        if (s.tls) { expect(n.sni).toBe(s.sni); expect(n['skip-cert-verify']).toBe(s.verify); }
      } else {
        expect(n.tls?.enabled ?? false).toBe(s.tls);
        if (s.tls) expect(n.tls).toMatchObject({ server_name: s.sni, insecure: s.verify });
      }
    } else {
      expect(n.protocol).toBe(s.family === 'SOCKS5' ? 'socks' : 'http');
      const server = n.settings.servers[0];
      expect(server.address).toBe(s.host); expect(server.port).toBe(s.port);
      expect(server.users).toEqual(s.username ? [{ user: s.username, pass: s.password }] : undefined);
      expect(n.streamSettings?.security ?? 'none').toBe(s.tls ? 'tls' : 'none');
      if (s.tls) expect(n.streamSettings.tlsSettings).toMatchObject({ serverName: s.sni, allowInsecure: s.verify });
    }
  }
}

export function coreCheck(profile: string, body: string, name: string, info: TestInfo) {
  const dir = info.outputPath(name); mkdirSync(dir, { recursive: true });
  const path = resolve(dir, profile === 'mihomo' ? 'config.yaml' : 'config.json');
  writeFileSync(path, body);
  if (!['mihomo', 'sing-box', 'xray', 'v2ray'].includes(profile)) return { bodySha256: sha(body) };
  const binary = resolve(process.env.DEVE_SUB_VALIDATORS ?? '../../target/validators', profile);
  const args = { mihomo: ['-t', '-d', dir, '-f'], 'sing-box': ['check', '-c'], xray: ['run', '-test', '-config'], v2ray: ['test', '-c'] }[profile]!;
  const result = spawnSync(binary, [...args, path], { timeout: 10_000, encoding: 'utf8', cwd: dir });
  writeFileSync(resolve(dir, 'validator.log'), result.stdout + result.stderr);
  expect(result.status, result.stdout + result.stderr).toBe(0);
  expect((result.stdout + result.stderr).toLowerCase()).not.toContain('download');
  return { exitCode: result.status, bodySha256: sha(body), validatorSha256: sha(readFileSync(binary)) };
}

export async function inspectInBrowser(page: Page, rows: ProxySample[]) {
  for (const s of rows) {
    await page.getByRole('searchbox').fill(s.name);
    await page.getByRole('button', { name: `Node details ${s.name}`, exact: true }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText(s.host);
    await expect(dialog).toContainText(String(s.port));
    if (s.tls) await expect(dialog).toContainText(s.sni!);
    await expect(dialog).not.toContainText(s.password ?? 'TEST:p@ss');
    await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  }
  await page.getByRole('searchbox').fill('');
}

export async function verifyRead(api: APIRequestContext, rows: ProxySample[]) {
  const response = await api.get('/api/v1/nodes?limit=100');
  expect(response.status()).toBe(200);
  const nodes = (await response.json()).nodes;
  expect(nodes).toHaveLength(rows.length);
  const ids: Record<string, string> = {};
  for (const s of rows) {
    const node = nodes.find((n: any) => n.display_name === s.name);
    checkDto(node, s); ids[s.name] = node.id;
    const detail = await api.get(`/api/v1/nodes/${node.id}`);
    expect(detail.status()).toBe(200); checkDto((await detail.json()).node, s);
  }
  return ids;
}
