import { test, expect } from '@playwright/test';
import { execFileSync } from 'child_process';
import { writeFileSync } from 'fs';
import { setupInBrowser, importInBrowser, templateInBrowser } from '../normal/fixture';
import { openSubscription, saveAndCopy } from '../normal/subscription-helpers';
import { coreCheck, sha } from '../normal/forward-checks';

test('isolated Compose runtime pairs native binary Web assets and persistent volume', async ({ page }, info) => {
  const container = process.env.DEVE_SUB_RUNTIME_CONTAINER;
  expect(container, 'exact isolated container ID required').toMatch(/^[0-9a-f]{12,64}$/);
  expect(execFileSync('docker', ['exec', container!, 'id', '-u'], { encoding: 'utf8' }).trim()).toBe('1000');
  await setupInBrowser(page);
  await importInBrowser(page, 'proxies:\n  - {name: Runtime-SOCKS, type: socks5, server: 192.0.2.31, port: 1080}\n  - {name: Runtime-HTTPS, type: http, server: 2001:db8::31, port: 8443, tls: true, sni: certificate.example, skip-cert-verify: false}');
  const nodes = (await (await page.request.get('/api/v1/nodes')).json()).nodes;
  expect(nodes).toHaveLength(2);
  const template = await templateInBrowser(page, 'runtime-template', { mode: 'fixed', nodeIds: nodes.map((n: any) => n.id) });
  const deliveries: { profile: string; url: string; digest: string }[] = [];
  const validators: object[] = [];
  for (const profile of ['json', 'mihomo', 'sing-box', 'xray', 'v2ray']) {
    await openSubscription(page, `runtime-${profile}`, template.id, profile);
    const url = await saveAndCopy(page);
    const response = await page.request.get(url); expect(response.status()).toBe(200);
    const content = await response.text();
    if (profile === 'json') {
      const output = JSON.parse(content);
      expect(output.map((n: any) => n.id).sort()).toEqual(nodes.map((n: any) => n.id).sort());
      expect(output.find((n: any) => n.display_name === 'Runtime-HTTPS').tls).toMatchObject({ enabled: true, server_name: 'certificate.example', skip_cert_verify: false });
    }
    validators.push({ profile, ...coreCheck(profile, content, `delivered-${profile}`, info) });
    deliveries.push({ profile, url, digest: sha(content) });
  }
  execFileSync('docker', ['restart', container!], { timeout: 30_000, stdio: 'pipe' });
  await expect.poll(async () => {
    try { return (await page.request.get('/health/ready', { timeout: 1000 })).status(); } catch { return 0; }
  }, { timeout: 30000 }).toBe(200);
  await page.reload(); await expect(page.locator('main')).toBeVisible();
  for (const delivered of deliveries) {
    const response = await page.request.get(delivered.url);
    expect(response.status()).toBe(200); expect(sha(await response.text())).toBe(delivered.digest);
  }
  const binaryHash = execFileSync('docker', ['exec', container!, 'sha256sum', '/app/deve-sub'], { encoding: 'utf8' }).split(' ')[0];
  const image = execFileSync('docker', ['inspect', container!, '--format', '{{.Image}}'], { encoding: 'utf8' }).trim();
  await page.screenshot({ path: info.outputPath('compose-after-restart.png'), fullPage: true });
  writeFileSync(info.outputPath('compose-proof.json'), JSON.stringify({
    status: 'PASS', at: new Date().toISOString(), binaryHash, image, uid: 1000,
    preseeded: false, entry: 'Blank UI over loopback published container port', validators,
    persistentVolumeRestart: 'PASS', packaging: 'Local offline runtime assembly; repository source build separately blocked on DNS',
  }, null, 2));
});
