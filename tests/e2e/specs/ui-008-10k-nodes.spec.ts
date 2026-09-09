import { test, expect } from '../fixtures/auth';

test('UI-008: 10k logical nodes retain bounded DOM across scroll, filter and select', async ({ authedPage: page }, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.locator('aside button').filter({ hasText: /节点管理|Nodes/ }).click();
  const container = page.locator('#nodes-scroll');
  await expect(container).toBeVisible();
  // Prove the whole dataset is in the logical list, not just the first page.
  await expect(container).toHaveAttribute('data-total-rows', '10000');
  const rows = container.locator('[data-node-row]');
  await expect(rows.first()).toBeVisible();
  const alignment = await container.evaluate(el => {
    const header = el.querySelector('[data-node-header]')!;
    const row = el.querySelector('[data-node-row]')!;
    return Array.from(header.children).map((cell, i) =>
      Math.abs(cell.getBoundingClientRect().x - row.children[i].getBoundingClientRect().x));
  });
  expect(alignment).toHaveLength(6);
  expect(Math.max(...alignment)).toBeLessThan(1);
  const counts: number[] = [];
  const timings: number[] = [];
  for (const position of [240000, 479000, 0, 96000, 400000, 48000, 0, 240000, 0, 479000]) {
    const started = Date.now();
    await container.evaluate(async (el, top) => {
      el.scrollTop = top;
      await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    }, position);
    await expect.poll(() => rows.count()).toBeGreaterThan(0);
    const count = await rows.count();
    counts.push(count);
    timings.push(Date.now() - started);
    expect(count).toBeLessThanOrEqual(24);
    expect(await container.locator('*').count()).toBeLessThan(500);
  }
  // Filtering while scrolled near the end must reset/clamp the window.
  // Guard against an offset exceeding the new filtered length.
  const search = page.locator('input[type="search"]');
  await search.fill('Node-0000');
  await expect(container).toHaveAttribute('data-total-rows', '1');
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText('Node-0000');
  await expect(rows.first()).toBeVisible();
  await search.fill('no-such-test-node');
  await expect(container).toHaveAttribute('data-total-rows', '0');
  await expect(rows).toHaveCount(0);
  await search.fill('');
  await expect(container).toHaveAttribute('data-total-rows', '10000');
  await expect(rows.first()).toBeVisible();
  await rows.first().locator('input[type="checkbox"]').check();
  await expect(page.locator('text=/已选|selected/')).toBeVisible();
  expect(errors).toEqual([]);
  // Observations only: CI asserts state and cardinality, not timing budgets.
  await testInfo.attach('virtualization-observations', {
    body: JSON.stringify({ logicalRows: 10000, renderedRows: counts, scrollObservationMs: timings, pageErrors: errors }),
    contentType: 'application/json',
  });
  await page.screenshot({ path: testInfo.outputPath('nodes-selected.png') });
});
