import { test, expect, importNodes, createTemplate, templateYaml } from './fixture';
import { readFileSync } from 'fs';
import { resolve } from 'path';

export const clashYaml = `proxy-groups:
  - name: PROXY
    type: select
    include-all-proxies: true
    proxies: [DIRECT]
rules:
  - DOMAIN-SUFFIX,example.com,DIRECT
  - IP-CIDR,192.168.0.0/16,DIRECT,no-resolve
  - MATCH,PROXY
`;

test('FUNC-CLASH-FIDELITY provider DNS order and colliding node names survive generation', async ({ api }, info) => {
  const imported = await api.post('/api/v1/nodes/import', { data: { source_type: 'uri_list',
    content: ['PROXY', 'Repeated', 'Repeated'].map((name, i) => `trojan://fixture@clash-${i}.example.com:443#${name}`).join('\n') } });
  expect(imported.status()).toBe(200);
  const ids = (await imported.json()).outcomes.map((o: { data: string }) => o.data);
  const yaml = readFileSync(resolve(__dirname, '../../fixtures/clash-routing.yaml'), 'utf8');
  const response = await api.post('/api/v1/templates', { data: { name: 'Native fidelity', spec_yaml: yaml } });
  expect(response.status(), await response.text()).toBe(201);
  const created = await response.json();
  const result = await api.post(`/api/v1/templates/${created.template.id}/generate?profile=mihomo&mode=strict`);
  expect(result.status(), await result.text()).toBe(200);
  const content = (await result.json()).content;
  expect(content).toContain('rule-set:z-specific');
  expect(content).toContain('rule-set:a-general');
  expect(content.indexOf('rule-set:z-specific')).toBeLessThan(content.indexOf('rule-set:a-general'));
  for (const id of ids) expect(content).toContain(`[${id}]`);
  expect(content).toContain('AND,((DOMAIN-SUFFIX,example.com),(NETWORK,UDP)),REJECT');
  const stored = (await (await api.get(`/api/v1/templates/${created.template.id}/versions/active`)).json()).version;
  expect(stored.spec_yaml).toBe(yaml);
  await info.attach('generated-native-clash.yaml', { body: content, contentType: 'application/yaml' });
});

test('FUNC-CLASH-ROUNDTRIP native routing YAML saves and generates unchanged rules', async ({ api }) => {
  await importNodes(api);
  const response = await api.post('/api/v1/templates', { data: { name: 'Clash routing', spec_yaml: clashYaml } });
  expect(response.status(), await response.text()).toBe(201);
  const created = await response.json();
  expect(created.version.spec_yaml).toBe(clashYaml);
  const result = await api.post(`/api/v1/templates/${created.template.id}/generate?profile=mihomo&mode=strict`);
  expect(result.status(), await result.text()).toBe(200);
  const { content } = await result.json();
  expect(content).toContain('MATCH,PROXY');
  expect(content).toContain('IP-CIDR,192.168.0.0/16,DIRECT,no-resolve');
  expect(content).not.toContain('include-all-proxies:');
  const groupSection = content.match(/proxy-groups:[\s\S]*?(?=\nrules:)/)?.[0];
  expect(groupSection).toBeDefined();
  for (const member of ['DIRECT', 'Node-0', 'Node-1', 'Node-2']) expect(groupSection).toContain(`- ${member}`);
  const preview = await api.post(`/api/v1/templates/${created.template.id}/preview?profile=mihomo&mode=strict`);
  expect(preview.status()).toBe(200);
  expect((await preview.json()).content).toBe(content);
  const wrongProfile = await api.post(`/api/v1/templates/${created.template.id}/generate?profile=sing-box&mode=lenient`);
  expect(wrongProfile.status()).toBe(400);
  const bad = await api.put(`/api/v1/templates/${created.template.id}`, { data: {
    name: 'Clash routing', description: '', spec_yaml: clashYaml.replace('MATCH,PROXY', 'MATCH,missing') } });
  expect(bad.status()).toBe(400);
  expect((await (await api.get(`/api/v1/templates/${created.template.id}/versions`)).json()).versions).toHaveLength(1);
  const active = await api.get(`/api/v1/templates/${created.template.id}/generations/active?profile=mihomo`);
  expect((await active.json()).content).toBe(content);
});

test('FUNC-TEMPLATE-CONCURRENT parallel saves and rollbacks preserve unique complete versions', async ({ api }) => {
  const template = await createTemplate(api);
  const path = `/api/v1/templates/${template.id}`;
  const first = (await (await api.get(`${path}/versions`)).json()).versions[0];
  const writes = await Promise.all(Array.from({ length: 12 }, (_, i) => api.put(path, { data: {
    name: template.name, description: `concurrent-${i}`, spec_yaml: templateYaml(`concurrent-${i}`) } })));
  expect(writes.map(r => r.status())).toEqual(Array(12).fill(200));
  const snapshots = await Promise.all(writes.map(r => r.json()));
  expect(new Set(snapshots.map(r => r.version.version)).size).toBe(12);
  const mixed = await Promise.all(Array.from({ length: 8 }, (_, i) => i % 2
    ? api.post(`${path}/rollback`, { data: { version_id: first.id } })
    : api.put(path, { data: { name: template.name, description: `mixed-${i}`, spec_yaml: templateYaml(`mixed-${i}`) } })));
  expect(mixed.map(r => r.status())).toEqual(Array(8).fill(200));
  const versions = (await (await api.get(`${path}/versions`)).json()).versions;
  expect(versions.map((v: { version: number }) => v.version)).toEqual(Array.from({ length: 17 }, (_, i) => 17 - i));
  expect(versions.filter((v: { is_active: boolean }) => v.is_active)).toHaveLength(1);
  const current = (await (await api.get(path)).json()).template;
  expect(current.active_version_id).toBe(versions.find((v: { is_active: boolean }) => v.is_active).id);
});

test('FUNC-TEMPLATE-PIN fallback stays inside the pinned version and deletion explains references', async ({ api }) => {
  const nodes = await importNodes(api);
  const template = await createTemplate(api);
  const path = `/api/v1/templates/${template.id}`;
  async function subscription(slug: string, pin?: number) {
    const data = { name: slug, slug, template_id: template.id, profile: 'mihomo', node_selection: { mode: 'dynamic' } };
    const response = await api.post('/api/v1/subscriptions', { data });
    expect(response.status()).toBe(201);
    const created = await response.json();
    if (pin) {
      const { template_id, ...editable } = data;
      expect((await api.put(`/api/v1/subscriptions/${created.subscription.id}`, { data: { ...editable, template_version_pin: pin } })).status()).toBe(200);
    }
    return `/sub/${created.token_plaintext}/mihomo`;
  }
  const pinned = await subscription('pinned', 1);
  const original = await (await api.get(pinned)).text();
  const newerYaml = templateYaml().replace('  dns: {}', '  dns: {nameserver: [https://dns.example.com/dns-query]}');
  expect((await api.put(path, { data: { name: template.name, description: 'v2', spec_yaml: newerYaml } })).status()).toBe(200);
  const following = await subscription('following');
  const newer = await (await api.get(following)).text();
  expect(newer).not.toBe(original);
  const deletion = await api.delete(path);
  expect(deletion.status()).toBe(409);
  expect((await deletion.json()).error).toBe('template_in_use');
  expect((await api.post('/api/v1/nodes/batch-enabled', { data: { node_ids: nodes, enabled: false } })).status()).toBe(200);
  const fallbacks = await Promise.all([api.get(pinned), api.get(following)]);
  expect(fallbacks.map(r => r.status())).toEqual([200, 200]);
  expect(await fallbacks[0].text()).toBe(original);
  expect(await fallbacks[1].text()).toBe(newer);
});

test('FUNC-CLASH-VALIDATION unavailable members block locally and invalid references preserve good output', async ({ api }) => {
  const nodes = await importNodes(api);
  const created = await api.post('/api/v1/templates', { data: { name: 'explicit node', spec_yaml: clashYaml.replace('include-all-proxies: true', 'proxies: [Node-0]').replace('    proxies: [DIRECT]\n', '') } });
  expect(created.status()).toBe(201);
  const template = (await created.json()).template;
  const path = `/api/v1/templates/${template.id}`;
  const result = await api.post(`${path}/generate?profile=mihomo&mode=strict`);
  expect(result.status()).toBe(200);
  const original = (await result.json()).content;
  expect((await api.post('/api/v1/nodes/batch-enabled', { data: { node_ids: [nodes[0]], enabled: false } })).status()).toBe(200);
  const updated = await api.post(`${path}/generate?profile=mihomo&mode=strict`);
  expect(updated.status()).toBe(200);
  const safe = await updated.json();
  const good = safe.content;
  expect(good).not.toBe(original);
  expect(good).toContain('- REJECT');
  expect(good).not.toContain('node-0.example.com');
  expect(safe.warnings.join(' ')).toContain('Node-0');
  const invalidEdit = await api.put(path, { data: { name: template.name, description: '', spec_yaml: clashYaml.replace('MATCH,PROXY', 'MATCH,unknown') } });
  expect(invalidEdit.status()).toBe(400);
  expect((await (await api.get(`${path}/generations/active?profile=mihomo`)).json()).content).toBe(good);
  const invalid = ['rules: [MATCH,PROXY]', 'rules: ["RULE-SET,missing,DIRECT"]',
    'rules: ["MATCH,PROXY"]\nproxies: []', 'rules: ["MATCH,PROXY"]\nscript: forbidden'];
  const failures = await Promise.all(invalid.map((spec_yaml, i) => api.post('/api/v1/templates', { data: { name: `invalid-${i}`, spec_yaml } })));
  expect(failures.map(r => r.status())).toEqual(invalid.map(() => 400));
  expect((await (await api.get('/api/v1/templates')).json()).templates).toHaveLength(1);
});

test('FUNC-TEMPLATE-VERSION update after rollback allocates a new history number', async ({ api }) => {
  const template = await createTemplate(api);
  const path = `/api/v1/templates/${template.id}`;
  const first = (await (await api.get(`${path}/versions`)).json()).versions[0];
  const body = { name: template.name, description: 'version two', spec_yaml: templateYaml('second') };
  expect((await api.put(path, { data: body })).status()).toBe(200);
  expect((await api.post(`${path}/rollback`, { data: { version_id: first.id } })).status()).toBe(200);
  const edited = await api.put(path, { data: { ...body, description: 'after rollback' } });
  expect(edited.status(), await edited.text()).toBe(200);
  expect((await edited.json()).version.version).toBe(3);
  const versions = (await (await api.get(`${path}/versions`)).json()).versions;
  expect(versions.map((v: { version: number }) => v.version)).toEqual([3, 2, 1]);
  expect(versions.filter((v: { is_active: boolean }) => v.is_active)).toHaveLength(1);
});
