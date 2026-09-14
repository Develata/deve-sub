import { test, expect, importNodes, createTemplate } from './fixture';

test('FUNC-DELIVERY-RETENTION concurrent selectors retain independent last-good output', async ({ api }) => {
  const nodes = await importNodes(api, 12);
  const template = await createTemplate(api);
  const paths = await Promise.all(nodes.map(async (id, index) => {
    const response = await api.post('/api/v1/subscriptions', { data: {
      name: `retention-${index}`, slug: `retention-${index}`, template_id: template.id,
      profile: 'mihomo', node_selection: { mode: 'fixed', nodeIds: [id] },
    } });
    expect(response.status()).toBe(201);
    return `/sub/${(await response.json()).token_plaintext}/mihomo`;
  }));
  const originals = await Promise.all(paths.map(async (path, index) => {
    const response = await api.get(path);
    expect(response.status()).toBe(200);
    const content = await response.text();
    expect(content).toContain(`node-${index}.example.com`);
    for (let other = 0; other < nodes.length; other++) {
      if (other !== index) expect(content).not.toContain(`node-${other}.example.com`);
    }
    return content;
  }));
  expect((await api.post('/api/v1/nodes/batch-enabled', {
    data: { node_ids: nodes, enabled: false },
  })).status()).toBe(200);
  await Promise.all(paths.map(async (path, index) => {
    const response = await api.get(path);
    expect(response.status(), `fallback for selector ${index}`).toBe(200);
    expect(await response.text()).toBe(originals[index]);
  }));
});
