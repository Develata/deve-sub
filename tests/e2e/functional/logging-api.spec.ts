import { readFileSync, readdirSync } from 'fs';
import { join } from 'path';
import { test, expect, importNodes, createTemplate } from './fixture';

test.describe('automatic audit retention', () => {
  test.use({ oldAuditCount: 12, auditRetentionDays: 0, auditRetentionEnv: 90 });
  test('audit005_runtime_retention_reclaims_only_expired_events', async ({ api }) => {
    await expect.poll(async () => (await (await api.get('/api/v1/audit-logs?action=fixture.old')).json()).entries.length).toBe(0);
    expect((await (await api.get('/api/v1/audit-logs/policy')).json()).retention_days).toBe(90);
    const receipts = (await (await api.get('/api/v1/audit-logs?action=audit.cleanup')).json()).entries;
    expect(receipts).toHaveLength(1);
    expect(JSON.parse(receipts[0].details_json)).toMatchObject({ deleted: 12, reason: 'retention' });
    expect(receipts[0].actor_id).toBeNull();
    expect((await (await api.get('/api/v1/audit-logs?action=auth.login')).json()).entries).toHaveLength(1);
  });
});

test('log001_default_logs_correlate_results_and_redact_secrets', async ({ api, server }, info) => {
  const denied = await api.get('/sub/fixture-delivery-secret/mihomo?token=fixture-query-secret', {
    headers: { 'x-request-id': 'fixture-untrusted-id', Cookie: 'fixture-cookie-secret' },
  });
  expect(denied.status()).toBe(404);
  const id = denied.headers()['x-request-id'];
  expect(id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
  const [node] = await importNodes(api, 1);
  const tag = await (await api.post('/api/v1/tags', { data: { name: 'fixture-log-tag' } })).json();
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.tag.id] } })).status()).toBe(204);
  const template = await createTemplate(api);
  const created = await api.post('/api/v1/subscriptions', { data: { name: 'fixture-logs', slug: 'fixture-logs', template_id: template.id, profile: 'mihomo', node_selection: { mode: 'dynamic' } } });
  expect(created.status()).toBe(201);
  const token = (await created.json()).token_plaintext;
  const delivered = await api.get(`/sub/${token}/mihomo`);
  expect(delivered.status()).toBe(200);
  const deliveryId = delivered.headers()['x-request-id'];
  const cached = await api.get(`/sub/${token}/mihomo`, { headers: { 'If-None-Match': delivered.headers().etag } });
  expect(cached.status()).toBe(304);
  const cachedId = cached.headers()['x-request-id'];
  const directory = info.outputPath('server-logs');
  const text = () => readdirSync(directory).map(file => readFileSync(join(directory, file), 'utf8')).join('\n');
  await expect.poll(() => text()).toContain(id);
  await expect.poll(() => text()).toContain(cachedId);
  const log = text();
  const line = log.split('\n').find(line => line.includes(id) && line.includes('completed'))!;
  expect(line).toContain('status=404');
  expect(line).toContain('duration_ms=');
  expect(line).toContain('uri=/sub/***/mihomo');
  expect(line).toContain('method=GET');
  expect(log.split('\n').find(line => line.includes(deliveryId) && line.includes('completed'))).toContain('status=200');
  expect(log.split('\n').find(line => line.includes(cachedId) && line.includes('completed'))).toContain('status=304');
  expect(log).not.toContain(token);
  for (const secret of ['fixture-delivery-secret', 'fixture-query-secret', 'fixture-untrusted-id', 'fixture-cookie-secret', '\u001b[']) expect(log).not.toContain(secret);
  expect(log).not.toContain('uri=/health/live');
  expect(log).not.toContain('pruned=0');
  expect((await (await api.get('/api/v1/audit-logs?action=node.import')).json()).entries).toHaveLength(1);
  expect((await (await api.get('/api/v1/audit-logs?action=tag.create')).json()).entries).toHaveLength(1);
  expect((await (await api.get('/api/v1/audit-logs?action=node.tags.update')).json()).entries).toHaveLength(1);
  expect(new URL(server).hostname).toBe('127.0.0.1');
});
