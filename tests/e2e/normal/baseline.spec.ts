import { readFileSync, writeFileSync } from 'fs';
import { resolve } from 'path';
import { spawnSync } from 'child_process';
import { test, expect, setupInBrowser, importInBrowser, templateInBrowser, navigate } from './fixture';
import { openSubscription, saveAndCopy } from './subscription-helpers';
import { coreCheck, sha } from './forward-checks';

type Sample = { name: string; protocol: string; source_type: string; input: string; profiles: string[]; fields: Record<string, unknown> };
const samples: Sample[] = JSON.parse(readFileSync(resolve(__dirname, '../../fixtures/subscription-acceptance/protocols.json'), 'utf8'));
const profiles = ['json', 'uri_list', 'shadowrocket', 'mihomo', 'sing-box', 'xray', 'v2ray'];
const agents: Record<string, string[]> = {
  mihomo: ['Mihomo/1.19.30', 'FlClash/0.8.92'], 'sing-box': ['sing-box/1.14.3'],
  xray: ['Xray/26.3.27'], v2ray: ['V2Ray/5.53.0', 'v2rayN/7.0', 'v2rayNG/1.0'],
  shadowrocket: ['Shadowrocket/2.2'],
};
const pointer = (value: any, path: string) => path.slice(1).split('/').reduce((v, key) => v?.[key.replace(/~1/g, '/').replace(/~0/g, '~')], value);

function outputNames(profile: string, content: string): string[] {
  if (profile === 'shadowrocket') content = Buffer.from(content, 'base64').toString('utf8');
  if (profile === 'uri_list' || profile === 'shadowrocket') return content.trim().split('\n').map(line => {
    if (line.startsWith('vmess://')) return JSON.parse(Buffer.from(line.slice(8), 'base64').toString('utf8')).ps;
    return decodeURIComponent(new URL(line).hash.slice(1));
  });
  if (profile === 'mihomo') {
    const parsed = spawnSync('python3', ['-c', 'import sys,yaml,json; print(json.dumps(yaml.safe_load(sys.stdin.read())))'], { input: content, encoding: 'utf8' });
    expect(parsed.status, parsed.stderr).toBe(0);
    return JSON.parse(parsed.stdout).proxies.map((n: any) => n.name);
  }
  const document = JSON.parse(content);
  if (profile === 'json') return document.map((n: any) => n.display_name);
  return [...document.outbounds.filter((n: any) => n.type !== 'shadowtls'), ...(document.endpoints ?? [])].map(n => n.tag);
}

for (const width of [1280, 390]) {
  test(`baseline eleven protocols plus forward proxies from blank UI at ${width}px`, async ({ page, instance }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await setupInBrowser(page);
    // Expectations are versioned, authored fixtures, not data inferred from output.
    for (const sample of samples) await importInBrowser(page, sample.input, sample.source_type);
    const nodes = (await (await page.request.get('/api/v1/nodes?limit=100')).json()).nodes;
    expect(nodes).toHaveLength(samples.length);
    for (const sample of samples) {
      const node = nodes.find((n: any) => n.display_name === sample.name);
      expect(node.is_active).toBe(true);
      await page.getByRole('searchbox').fill(sample.name);
      await page.getByRole('button', { name: `Node details ${sample.name}`, exact: true }).click();
      const dialog = page.getByRole('dialog');
      await expect(dialog).toContainText(String(sample.fields['/endpoint/port']));
      await expect(dialog).toContainText((sample.fields['/endpoint/host'] as { value: string }).value);
      await dialog.getByRole('button', { name: 'Close', exact: true }).click();
    }
    await page.getByRole('searchbox').fill('');
    const proof: object[] = [], deliveries: { url: string; digest: string }[] = [];
    for (const profile of profiles) {
      const accepted = samples.filter(sample => sample.profiles.includes(profile));
      const selected = accepted.map(sample => nodes.find((n: any) => n.display_name === sample.name).id);
      const template = await templateInBrowser(page, `baseline-${profile}`, { mode: 'fixed', nodeIds: selected }, [profile]);
      await openSubscription(page, `baseline-${profile}`, template.id, profile);
      const url = await saveAndCopy(page);
      const response = await page.request.get(url);
      expect(response.status()).toBe(200);
      const body = await response.text();
      expect(outputNames(profile, body).sort()).toEqual(accepted.map(sample => sample.name).sort());
      if (profile === 'json') {
        const canonical = JSON.parse(body);
        for (const sample of accepted) {
          const node = canonical.find((n: any) => n.display_name === sample.name);
          for (const [path, expected] of Object.entries(sample.fields))
            expect(pointer(node, path), `${sample.name} ${path}`).toEqual(expected);
        }
      }
      const validation = coreCheck(profile, body, `delivered-${profile}`, info);
      const uaChecks: object[] = [];
      if (agents[profile]) {
        const row = page.getByRole('row').filter({ has: page.getByRole('cell', { name: `baseline-${profile}`, exact: true }) });
        await row.locator('summary').click();
        await row.getByRole('button', { name: 'Regenerate Code', exact: true }).click();
        await page.getByRole('dialog').getByRole('button', { name: 'Regenerate Code', exact: true }).click();
        await expect(page.getByRole('dialog')).toHaveCount(0);
        const subscriptions = (await (await page.request.get('/api/v1/subscriptions')).json()).subscriptions;
        const short = subscriptions.find((s: any) => s.name === `baseline-${profile}`).short_code;
        expect(short).toBeTruthy();
        await expect(row).toContainText(short);
        for (const userAgent of agents[profile]) {
          for (const route of [url.slice(0, -(profile.length + 1)), `${instance.url}/s/${short}`]) {
            const auto = await page.request.get(route, { headers: { 'User-Agent': userAgent } });
            expect(auto.status()).toBe(200); expect(sha(await auto.text())).toBe(sha(body));
            expect((await page.request.get(route, { headers: { 'User-Agent': userAgent, 'If-None-Match': auto.headers().etag } })).status()).toBe(304);
          }
          uaChecks.push({ userAgent, tokenAndShortRoute: 200, etag: 304 });
        }
        // Client branding is not an independent public profile route.
        for (const alias of ['flclash', 'v2rayN', 'v2rayNG'])
          expect((await page.request.get(url.slice(0, -(profile.length)) + alias)).status()).toBe(404);
      }
      proof.push({ profile, selectedNames: accepted.map(sample => sample.name), selectedIds: selected,
        ...validation, uaChecks, uriOrShadowrocketLimit: ['uri_list', 'shadowrocket'].includes(profile) ? 'Syntax/name assertions; no native GUI client import' : null });
      deliveries.push({ url, digest: sha(body) });
    }
    await instance.restart();
    for (const delivery of deliveries) {
      const response = await page.request.get(delivery.url);
      expect(response.status()).toBe(200); expect(sha(await response.text())).toBe(delivery.digest);
    }
    await navigate(page, /^Nodes$/);
    await expect(page.getByText('13 matching / 13 loaded', { exact: true })).toBeVisible();
    await page.screenshot({ path: info.outputPath('baseline-after-restart.png'), fullPage: true });
    writeFileSync(info.outputPath('baseline-proof.json'), JSON.stringify({
      status: 'PASS', at: new Date().toISOString(), width, entry: 'UI', preseeded: false,
      samples: samples.map(s => ({ name: s.name, sourceType: s.source_type, protocol: s.protocol })),
      canonicalFieldAssertions: samples.reduce((sum, sample) => sum + Object.keys(sample.fields).length, 0),
      outputs: proof, restart: 'all seven exact body hashes preserved',
      limits: ['This imports each protocol through its authored input format, not the full 7×7 input/output cross product.',
        'Native cores validate configuration offline; no proxy connectivity or native GUI refresh is claimed.'],
    }, null, 2));
  });
}
