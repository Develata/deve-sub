import { test, expect, navigate } from './fixture';

test.use({ auditRetentionDays: 0, oldAuditCount: 3 });

test('audit004_ui_preview_cancel_confirm_and_receipt', async ({ page, api }, info) => {
  await navigate(page, /审计日志|Audit Log/i);
  await expect(page.getByText(/自动回收已关闭|Automatic retention is disabled/)).toBeVisible();
  await page.getByRole('button', { name: /预览清理|Preview cleanup/ }).click();
  await expect(page.getByText(/本批待清理：3|Entries in this batch: 3/)).toBeVisible();
  await page.getByRole('button', { name: /^取消$|^Cancel$/ }).click();
  expect((await (await api.get('/api/v1/audit-logs?action=fixture.old')).json()).entries).toHaveLength(3);
  await page.getByRole('button', { name: /预览清理|Preview cleanup/ }).click();
  await expect(page.getByRole('button', { name: /确认清理本批|Confirm batch cleanup/ })).toBeVisible();
  await page.getByLabel(/手动保留天数|Days to keep manually/).selectOption('30');
  await expect(page.getByRole('button', { name: /确认清理本批|Confirm batch cleanup/ })).toHaveCount(0);
  await page.getByRole('button', { name: /预览清理|Preview cleanup/ }).click();
  await page.getByRole('button', { name: /确认清理本批|Confirm batch cleanup/ }).click();
  await expect(page.getByRole('status').filter({ hasText: /已清理 3|Deleted 3/ })).toBeVisible();
  await expect(page.locator('table').getByText(/audit.cleanup/)).toBeVisible();
  expect((await (await api.get('/api/v1/audit-logs?action=fixture.old')).json()).entries).toHaveLength(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: info.outputPath('audit-cleanup.png'), fullPage: true });
  await navigate(page, /审计日志|Audit Log/i);
  await expect(page.locator('table').getByText(/audit.cleanup/)).toBeVisible();
});

test('audit004_ui_concurrent_cleanup_requires_new_preview', async ({ page, api }) => {
  await navigate(page, /审计日志|Audit Log/i);
  await page.getByRole('button', { name: /预览清理|Preview cleanup/ }).click();
  await expect(page.getByRole('button', { name: /确认清理本批|Confirm batch cleanup/ })).toBeVisible();
  const preview = await (await api.post('/api/v1/audit-logs/cleanup/preview', { data: { keep_days: 90 } })).json();
  expect((await api.post('/api/v1/audit-logs/cleanup', { data: { before_unix_ms: preview.before_unix_ms, entry_ids: preview.entry_ids } })).status()).toBe(200);
  await page.getByRole('button', { name: /确认清理本批|Confirm batch cleanup/ }).click();
  await expect(page.getByRole('alert')).toHaveText(/日志已变化|History changed/);
  await expect(page.getByRole('button', { name: /确认清理本批|Confirm batch cleanup/ })).toHaveCount(0);
  await page.getByRole('button', { name: /预览清理|Preview cleanup/ }).click();
  await expect(page.getByText(/本批待清理：0|Entries in this batch: 0/)).toBeVisible();
  expect((await (await api.get('/api/v1/audit-logs?action=audit.cleanup')).json()).entries).toHaveLength(1);
});

test('audit001_ui_old_response_cannot_replace_new_filter', async ({ page }) => {
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let first = true;
  await page.route('**/api/v1/audit-logs?**', async route => {
    if (first) {
      first = false;
      const response = await route.fetch();
      await held;
      await route.fulfill({ response });
    } else await route.continue();
  });
  try {
    await navigate(page, /审计日志|Audit Log/i);
    await expect.poll(() => first).toBe(false);
    await page.getByRole('combobox', { name: /^操作$|^Action$/ }).selectOption('auth.login');
    await page.getByRole('button', { name: /^筛选$|^Apply$/i }).click();
    await expect(page.locator('tbody tr')).toHaveCount(1);
    const oldResponse = page.waitForResponse(response => response.url().endsWith('/api/v1/audit-logs?limit=50'));
    release();
    await oldResponse;
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(page.locator('tbody tr')).toHaveCount(1);
    await expect(page.locator('table')).not.toContainText('fixture.old');
  } finally { release(); }
});
