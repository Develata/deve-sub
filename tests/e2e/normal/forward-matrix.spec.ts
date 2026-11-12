import { writeFileSync } from 'fs';
import { spawnSync } from 'child_process';
import { test, expect, setupInBrowser, navigate, importInBrowser, templateInBrowser } from './fixture';
import { openSubscription, saveAndCopy } from './subscription-helpers';
import { families, formats, profiles, samples, content, rowId } from './forward-samples';
import { checkOutput, coreCheck, inspectInBrowser, verifyRead, sha } from './forward-checks';

for (const format of formats) for (const entry of ['UI', 'API']) {
  test(`forward proxies ${format} through independent ${entry} setup import selection and delivery`, async ({ page, instance, playwright }, info) => {
    const rows = samples(format), records: object[] = [];
    const api = await playwright.request.newContext({ baseURL: instance.url, extraHTTPHeaders: { Origin: instance.url } });
    try {
      if (entry === 'UI') await setupInBrowser(page);
      else {
        const credentials = { username: 'normal-admin', password: 'SyntheticPassword2026' };
        expect((await api.post('/api/v1/auth/setup', { data: credentials })).status()).toBe(201);
        expect((await api.post('/api/v1/auth/login', { data: credentials })).status()).toBe(200);
        // This is explicitly the API lane; login is visible, without storageState injection.
        await page.goto('/');
        await page.locator('input[autocomplete=username]').fill(credentials.username);
        await page.locator('input[autocomplete=current-password]').fill(credentials.password);
        await page.getByRole('button', { name: /^登录$|^Sign In$|^Login$/ }).click();
        await expect(page.locator('main')).toBeVisible();
        await page.getByRole('button', { name: 'EN', exact: true }).click();
      }
      const reader = entry === 'UI' ? page.request : api;
      const imports = [rows.filter((s, i) => i % 10 === 0), rows.filter((s, i) => i % 10 !== 0)];
      // First import one normal node from every family, then the remaining mixed batch.
      for (const first of imports[0]) {
        if (entry === 'UI') await importInBrowser(page, content(format, [first]), format);
        else expect((await api.post('/api/v1/nodes/import', { data: { source_type: format, content: content(format, [first]) } })).status()).toBe(200);
      }
      if (entry === 'UI') {
        await importInBrowser(page, content(format, imports[1]), format);
        await importInBrowser(page, content(format, rows), 'auto');
        if (format === 'mihomo_yaml') await importInBrowser(page, content(format, rows, true), 'auto');
      } else {
        for (const [kind, selected] of [[format, imports[1]], ['auto', rows]] as const) {
          const response = await api.post('/api/v1/nodes/import', { data: { source_type: kind, content: content(format, [...selected]) } });
          expect(response.status()).toBe(200);
          const result = await response.json();
          expect(result.failed).toBe(0); expect(result.unsupported).toBe(0);
          if (kind === 'auto') expect(result.duplicate_nodes).toBe(rows.length);
        }
        if (format === 'mihomo_yaml') {
          const response = await api.post('/api/v1/nodes/import', { data: { source_type: 'auto', content: content(format, rows, true) } });
          expect(response.status()).toBe(200); expect((await response.json()).duplicate_nodes).toBe(rows.length);
        }
      }
      const ids = await verifyRead(reader, rows);
      await navigate(page, /^Nodes$/);
      await inspectInBrowser(page, rows);
      await page.screenshot({ path: info.outputPath('imported-visible.png'), fullPage: true });
      // Read-only storage inspection proves encryption, without seeding database state.
      const storage = spawnSync('python3', ['-c', `
import sys,sqlite3,json,pathlib
root=pathlib.Path(sys.argv[1]); db=sqlite3.connect(f'file:{root}/deve-sub.db?mode=ro',uri=True)
rows=db.execute('select protocol_config_json_encrypted,authentication_json_encrypted,tls_json_encrypted from nodes').fetchall()
assert len(rows)==int(sys.argv[2])
assert all(row[0] and row[1] for row in rows)
assert all('TEST:p@ss' not in str(row) and 'synthetic+user' not in str(row) for row in rows)
assert all(b'TEST:p@ss' not in p.read_bytes() for p in root.glob('deve-sub.db*'))
print(json.dumps({'nodes':len(rows),'sensitive_columns_encrypted':True,'database_seeded':False}))
`, instance.directory, String(rows.length)], { encoding: 'utf8' });
      expect(storage.status, storage.stderr).toBe(0);
      writeFileSync(info.outputPath('storage-check.json'), storage.stdout);
      const native = rows.filter(s => s.family !== 'socks5h');
      const template = async (name: string, selected: typeof rows) => {
        const selector = { mode: 'fixed', nodeIds: selected.map(s => ids[s.name]) };
        if (entry === 'UI') return (await templateInBrowser(page, name, selector, profiles)).id;
        const result = await api.post('/api/v1/templates', { data: { name, description: 'Synthetic normal matrix',
          spec_yaml: JSON.stringify({ apiVersion: 'deve-sub.io/v1', kind: 'SubscriptionTemplate', metadata: { name },
            spec: { targetProfiles: profiles, nodeSelector: selector, proxyGroups: [], rules: [] } }) } });
        expect(result.status()).toBe(201); return (await result.json()).template.id;
      };
      const nativeTemplate = await template('native-selection', native);
      const allTemplate = await template('complete-selection', rows);
      const deliveries: { profile: string; url: string; selected: typeof rows; digest: string }[] = [];
      for (const profile of profiles.filter(p => p !== 'shadowrocket')) {
        const selected = ['json', 'uri_list'].includes(profile) ? rows : native;
        const templateId = ['json', 'uri_list'].includes(profile) ? allTemplate : nativeTemplate;
        let url: string;
        if (entry === 'UI') { await openSubscription(page, `normal-${profile}`, templateId, profile); url = await saveAndCopy(page); }
        else {
          const response = await api.post('/api/v1/subscriptions', { data: { name: `normal-${profile}`, slug: `normal-${profile}`,
            template_id: templateId, profile, node_selection: { mode: 'inherit_template' } } });
          expect(response.status()).toBe(201); url = `${instance.url}/sub/${(await response.json()).token_plaintext}/${profile}`;
        }
        const response = await reader.get(url); expect(response.status()).toBe(200);
        const body = await response.text();
        checkOutput(profile, body, selected, ids);
        const proof = coreCheck(profile, body, `delivered-${profile}`, info);
        deliveries.push({ profile, url, selected, digest: sha(body) });
        records.push({ entry, format, profile, names: selected.map(s => s.name), ...proof, status: 'PASS' });
      }
      // Unsupported targets must fail before publishing a token, including mixed all-unsupported pools.
      const rejects: [string, string][] = [['shadowrocket', nativeTemplate]];
      if (rows.some(s => s.family === 'socks5h')) {
        const remoteTemplate = await template('remote-selection', rows.filter(s => s.family === 'socks5h'));
        for (const p of ['mihomo', 'sing-box', 'xray', 'v2ray', 'shadowrocket']) rejects.push([p, remoteTemplate]);
      }
      for (const [profile, templateId] of rejects) {
        let result;
        if (entry === 'UI') {
          const modal = await openSubscription(page, `reject-${profile}`, templateId, profile);
          const pending = page.waitForResponse(r => r.url().endsWith('/subscriptions') && r.request().method() === 'POST');
          await modal.getByRole('button', { name: 'Save', exact: true }).click();
          result = await pending; expect(result.status()).toBe(422);
          await expect(modal.getByRole('alert')).toContainText('no compatible nodes');
          await expect(modal.locator('#subscription-url')).toHaveCount(0);
          await modal.getByRole('button', { name: 'Cancel', exact: true }).click();
        } else result = await api.post('/api/v1/subscriptions', { data: {
          name: `reject-${profile}`, slug: `reject-${profile}`, template_id: templateId, profile, node_selection: { mode: 'inherit_template' } } });
        expect(result.status()).toBe(422);
        const error = await result.json(); expect(error.message).toContain('no compatible nodes');
        records.push({ entry, format, profile, action: 'rejected_before_token', status: 'PASS', reason: error.message });
      }
      expect((await (await reader.get('/api/v1/subscriptions')).json()).subscriptions).toHaveLength(6);
      await instance.restart(); await page.reload(); await expect(page.locator('main')).toBeVisible();
      expect(await verifyRead(reader, rows)).toEqual(ids);
      for (const delivered of deliveries) {
        const response = await reader.get(delivered.url); expect(response.status()).toBe(200);
        const body = await response.text(); checkOutput(delivered.profile, body, delivered.selected, ids);
        expect(sha(body)).toBe(delivered.digest);
      }
      writeFileSync(info.outputPath('forward-matrix.json'), JSON.stringify({
        at: new Date().toISOString(), format, entry, api_preseed: false, samples: rows,
        rows: families.filter(f => rows.some(s => s.family === f)).map(f => rowId(format, f, entry)),
        addressScope: 'Reserved literal addresses and domain spelling preserved; no endpoint DNS or proxy connectivity claimed.',
        unsupportedScope: 'Shadowrocket output mapping unverified; explicit remote DNS not mapped to native outbound configuration.',
        records, restart: 'PASS', encryption: 'PASS',
      }, null, 2));
    } finally { await api.dispose(); }
  });
}
