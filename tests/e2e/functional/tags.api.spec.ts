import { test, expect, importNodes } from './fixture';

test('FUNC-TAG-INPUT tag normalization, Unicode and reference errors', async ({ api }) => {
  const [node] = await importNodes(api);
  expect((await api.post('/api/v1/tags', { data: { name: '   ' } })).status()).toBe(400);
  const response = await api.post('/api/v1/tags', { data: { name: `  ${'标'.repeat(43)}  ` } });
  expect(response.status()).toBe(201);
  const tag = (await response.json()).tag;
  expect(tag.name).toBe('标'.repeat(43));
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.id, tag.id] } })).status()).toBe(204);
  expect((await api.get(`/api/v1/nodes/${node}`)).ok()).toBeTruthy();
  expect((await (await api.get(`/api/v1/nodes/${node}`)).json()).node.tags).toHaveLength(1);
  const missing = '01AAAAAAAAAAAAAAAAAAAAAAAA';
  expect((await api.put(`/api/v1/nodes/${missing}/tags`, { data: { tag_ids: [] } })).status()).toBe(404);
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [missing] } })).status()).toBe(404);
  expect((await (await api.get(`/api/v1/nodes/${node}`)).json()).node.tags).toHaveLength(1);
});

test('FUNC-CHAIN-RACE concurrent opposing chains never commit a cycle', async ({ api }) => {
  const [a, b] = await importNodes(api);
  for (let iteration = 0; iteration < 8; iteration++) {
    for (const node of [a, b]) expect((await api.put(`/api/v1/nodes/${node}/chain`, { data: { nodes: [] } })).status()).toBe(200);
    const responses = await Promise.all([
      api.put(`/api/v1/nodes/${a}/chain`, { data: { nodes: [b] } }),
      api.put(`/api/v1/nodes/${b}/chain`, { data: { nodes: [a] } }),
    ]);
    expect(responses.map(r => r.status()).sort()).toEqual([200, 409]);
    const nodes = await Promise.all([a, b].map(async id => (await (await api.get(`/api/v1/nodes/${id}`)).json()).node));
    expect(nodes.filter(n => n.chain.length)).toHaveLength(1);
  }
});

test('FUNC-TAG-RACE concurrent additions, replacement rollback and removal', async ({ api }) => {
  const nodes = await importNodes(api);
  const tags = await Promise.all(['A', 'B', 'C'].map(async name => {
    const r = await api.post('/api/v1/tags', { data: { name } }); expect(r.status()).toBe(201); return (await r.json()).tag;
  }));
  const assignments = (tag: string) => nodes.map(node_id => ({ node_id, tag_ids: [tag] }));
  const added = await Promise.all(tags.map(tag => api.post('/api/v1/nodes/batch-tags', { data: { mode: 'add', assignments: assignments(tag.id) } })));
  expect(added.map(r => r.status())).toEqual([204, 204, 204]);
  const expected = tags.map(t => t.id).sort();
  for (const id of nodes) expect((await (await api.get(`/api/v1/nodes/${id}`)).json()).node.tags.map((t: {id: string}) => t.id).sort()).toEqual(expected);
  const invalid = await api.post('/api/v1/nodes/batch-tags', { data: { assignments: [
    { node_id: nodes[0], tag_ids: [] }, { node_id: nodes[1], tag_ids: ['01AAAAAAAAAAAAAAAAAAAAAAAA'] },
  ] } });
  expect(invalid.status()).toBe(404);
  expect((await (await api.get(`/api/v1/nodes/${nodes[0]}`)).json()).node.tags).toHaveLength(3);
  expect((await api.post('/api/v1/nodes/batch-tags', { data: { mode: 'remove', assignments: assignments(tags[0].id) } })).status()).toBe(204);
  for (const id of nodes) expect((await (await api.get(`/api/v1/nodes/${id}`)).json()).node.tags).toHaveLength(2);
  expect((await api.post('/api/v1/nodes/batch-tags', { data: { assignments: [assignments(tags[0].id)[0], assignments(tags[1].id)[0]] } })).status()).toBe(400);
});

test('FUNC-TAG-IDENTITY concurrent names, rename and delete versus assignment', async ({ api }) => {
  const [node] = await importNodes(api);
  const created = await Promise.all(Array.from({ length: 6 }, () => api.post('/api/v1/tags', { data: { name: 'Unique' } })));
  expect(created.map(r => r.status()).sort()).toEqual([201, 409, 409, 409, 409, 409]);
  const tag = (await created.find(r => r.status() === 201)!.json()).tag;
  expect((await api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.id] } })).status()).toBe(204);
  expect((await api.patch(`/api/v1/tags/${tag.id}`, { data: { name: 'Renamed', color: '#123456' } })).status()).toBe(200);
  expect((await (await api.get(`/api/v1/nodes/${node}`)).json()).node.tags[0]).toMatchObject({ id: tag.id, name: 'Renamed', color: '#123456' });
  const [deleted, assigned] = await Promise.all([
    api.delete(`/api/v1/tags/${tag.id}`), api.put(`/api/v1/nodes/${node}/tags`, { data: { tag_ids: [tag.id] } }),
  ]);
  expect(deleted.status()).toBe(204); expect([204, 404]).toContain(assigned.status());
  expect((await (await api.get(`/api/v1/nodes/${node}`)).json()).node.tags).toEqual([]);
});

test('FUNC-NODE-FILTER concurrent enable and region changes retain both fields', async ({ api }) => {
  const [node] = await importNodes(api);
  const [enabled, region] = await Promise.all([
    api.post('/api/v1/nodes/batch-enabled', { data: { node_ids: [node, node], enabled: false } }),
    api.patch(`/api/v1/nodes/${node}/region`, { data: { region: 'JP' } }),
  ]);
  expect(enabled.status()).toBe(200); expect((await enabled.json()).updated).toBe(1); expect(region.status()).toBe(200);
  const detail = (await (await api.get(`/api/v1/nodes/${node}`)).json()).node;
  expect(detail).toMatchObject({ is_active: false, region: 'JP' });
  const active = (await (await api.get('/api/v1/nodes')).json()).nodes;
  expect(active.map((n: { id: string }) => n.id)).not.toContain(node);
  const filtered = (await (await api.get('/api/v1/nodes?region=JP&include_inactive=true')).json()).nodes;
  expect(filtered.map((n: { id: string }) => n.id)).toEqual([node]);
});

test('FUNC-IMPORT-RACE concurrent duplicate imports preserve one node identity', async ({ api }) => {
  const outcomes = await Promise.all(Array.from({ length: 4 }, () => importNodes(api, 3)));
  for (const ids of outcomes) expect([...ids].sort()).toEqual([...outcomes[0]].sort());
  expect((await (await api.get('/api/v1/nodes')).json()).nodes).toHaveLength(3);
});
