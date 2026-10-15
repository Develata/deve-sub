import { test, expect, importNodes, navigate } from './fixture';

test('FUNC-NODE-TABLE 10k virtual rows expose total/index semantics and a keyboard scroll surface', async ({ api, page }) => {
  await importNodes(api, 10000);
  await navigate(page, /^节点管理$|^Nodes$/);
  const table = page.getByRole('table', { name: /节点管理|Node Management/ });
  await expect(table).toHaveAttribute('data-total-rows', '10000');
  await expect(table).toHaveAttribute('aria-rowcount', '10001');
  await expect(table).toHaveAttribute('aria-colcount', '6');
  await expect(table).toHaveAttribute('tabindex', '0');
  await expect(table.getByRole('columnheader')).toHaveCount(6);
  const rows = table.locator('[data-node-row]');
  await expect(rows.first()).toHaveAttribute('aria-rowindex', '2');
  expect(await rows.count()).toBeLessThanOrEqual(24);
  await expect(rows.first().getByRole('rowheader')).toContainText('Node-');
  await rows.first().getByRole('checkbox').check();
  await expect(rows.first().getByRole('checkbox')).toBeChecked();
  const alignment = await table.evaluate(el => {
    const header = el.querySelector('[data-node-header]')!;
    const row = el.querySelector('[data-node-row]')!;
    return Array.from(header.children).map((cell, i) => Math.abs(cell.getBoundingClientRect().x - row.children[i].getBoundingClientRect().x));
  });
  expect(Math.max(...alignment)).toBeLessThan(1);
  if (page.viewportSize()!.width < 768) {
    await table.focus();
    await page.keyboard.press('ArrowRight');
    await expect.poll(() => table.evaluate(el => el.scrollLeft)).toBeGreaterThan(0);
    await table.evaluate(el => { el.scrollLeft = el.scrollWidth; });
    const action = rows.first().getByRole('button', { name: /^覆盖$|^Override$/ });
    await expect(action).toBeInViewport();
    await action.focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('div.fixed')).toBeVisible();
    await page.locator('div.fixed').getByRole('button', { name: /^取消$|^Cancel$/ }).click();
  }
  await table.evaluate(el => { el.scrollTop = el.scrollHeight; });
  await expect(rows.last()).toHaveAttribute('aria-rowindex', '10001');
  expect(await rows.count()).toBeLessThanOrEqual(24);
  expect(await table.locator('*').count()).toBeLessThan(500);
  await page.getByRole('searchbox').fill('Node-9999');
  await expect(table).toHaveAttribute('aria-rowcount', '2');
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute('aria-rowindex', '2');
  await expect(rows.first()).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await table.evaluate(el => { el.scrollLeft = 0; });
  await page.screenshot({ scale: 'css', path: test.info().outputPath('node-table-filtered.png') });
  await test.info().attach('node-table-accessibility', { body: await table.ariaSnapshot(), contentType: 'text/plain' });
});
