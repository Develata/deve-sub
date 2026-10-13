import { test, expect } from './fixture';
import type { Route } from '@playwright/test';

const summary = { total_upload: 1048576, total_download: 3145728, by_source_kind: [], by_probe_source: [] };
const history = { scoped_to_subscription: false, points: Array.from({ length: 30 }, (_, i) => ({
  date: `2026-09-${String(i + 1).padStart(2, '0')}`, total_upload: (i % 4 + 1) * 131072,
  total_download: (i % 7 + 1) * 262144, source_breakdown: [],
})) };
const failure = { status: 503, json: { error: 'fixture_unavailable', message: 'Fixture traffic unavailable' } };

test('FUNC-DASHBOARD-STATES independent loading, failure and retry never fabricate zero traffic', async ({ page }) => {
  let heldSummary: Route | undefined;
  let heldHistory: Route | undefined;
  let summaryRequests = 0, historyRequests = 0;
  await page.route('**/api/v1/dashboard/traffic', route => { summaryRequests++; heldSummary = route; });
  await page.route('**/api/v1/dashboard/traffic/history?days=30', route => { historyRequests++; heldHistory = route; });
  await page.goto('/');
  await expect(page).toHaveTitle('Deve Sub');
  await expect(page.locator('main').getByRole('heading', { name: '仪表盘' })).toBeVisible();
  const totals = page.getByRole('region', { name: '流量汇总', exact: true });
  const chart = page.getByRole('region', { name: '30 天流量趋势', exact: true }).first();
  await expect(totals.getByRole('status')).toContainText('正在加载');
  await expect(chart.getByRole('status')).toContainText('正在加载');
  await expect(page.locator('main')).not.toContainText('0 B');
  await expect(page.locator('main')).not.toContainText('没有流量历史');
  await expect.poll(() => !!heldSummary && !!heldHistory).toBe(true);
  await heldSummary!.fulfill(failure);
  await heldHistory!.fulfill({ json: history });
  await expect(totals.getByRole('alert')).toContainText('流量汇总加载失败');
  await expect(chart.locator('figure')).toBeVisible();
  await expect(page.locator('main')).not.toContainText('0 B');
  await page.screenshot({ scale: 'css', path: test.info().outputPath('dashboard-partial-error.png') });
  await totals.getByRole('button', { name: '重试' }).click();
  await expect(totals.getByRole('status')).toContainText('正在加载');
  await expect(chart.locator('figure')).toBeVisible();
  await expect.poll(() => summaryRequests).toBe(2);
  expect(historyRequests).toBe(1);
  await heldSummary!.fulfill({ json: { ...summary, total_upload: 0, total_download: 0 } });
  await expect(totals.getByText('0 B', { exact: true })).toHaveCount(3);
  await expect(totals.getByRole('alert')).toHaveCount(0);
});

test('FUNC-DASHBOARD-HISTORY failed history stays distinct from an empty successful response', async ({ page }) => {
  await page.addInitScript(() => { localStorage.setItem('lang', 'en'); });
  let historyRequests = 0, summaryRequests = 0;
  await page.route('**/api/v1/dashboard/traffic', route => { summaryRequests++; return route.fulfill({ json: summary }); });
  await page.route('**/api/v1/dashboard/traffic/history?days=30', route => ++historyRequests === 1
    ? route.fulfill(failure) : route.fulfill({ json: { scoped_to_subscription: false, points: [] } }));
  await page.goto('/');
  const totals = page.getByRole('region', { name: 'Traffic summary', exact: true });
  const chart = page.getByRole('region', { name: '30-day traffic history', exact: true });
  await expect(totals).toContainText('4.00 MiB');
  await expect(chart.getByRole('alert')).toContainText('Could not load traffic history');
  await expect(chart).not.toContainText('No traffic history');
  await chart.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(chart).toContainText('No traffic history for this period');
  await expect(totals).toContainText('4.00 MiB');
  expect(summaryRequests).toBe(1);
  await page.screenshot({ scale: 'css', path: test.info().outputPath('dashboard-empty-history.png') });
});

test('FUNC-DASHBOARD-CHART dates, IEC units and non-color legend remain readable and translated', async ({ page }) => {
  await page.route('**/api/v1/dashboard/traffic', route => route.fulfill({ json: summary }));
  await page.route('**/api/v1/dashboard/traffic/history?days=30', route => route.fulfill({ json: history }));
  await page.goto('/');
  const figure = page.locator('figure');
  await expect(figure).toBeVisible();
  await expect(figure.locator('figcaption')).toContainText('2026-09-01 – 2026-09-30 · MiB');
  await expect(figure.locator('figcaption')).toContainText('上传');
  expect(await figure.locator('tbody tr').count()).toBe(30);
  await page.getByRole('button', { name: 'EN', exact: true }).click();
  await expect(figure.locator('figcaption')).toContainText('Upload');
  await expect(figure.locator('figcaption')).toContainText('Download');
  await expect(figure.locator('thead')).toContainText('Date (UTC)');
  await expect(page.locator('main')).not.toContainText('总上传');
  const noOverflow = await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
  expect(noOverflow).toBe(true);
  // Hidden daily values must not add a blank scroll range below visible content.
  const extent = await page.evaluate(() => ({
    documentHeight: document.documentElement.scrollHeight,
    visibleBottom: Math.ceil(document.querySelector('main')!.getBoundingClientRect().bottom + scrollY),
    viewportHeight: innerHeight,
  }));
  expect(extent.documentHeight).toBeLessThanOrEqual(Math.max(extent.visibleBottom, extent.viewportHeight));
  await page.screenshot({ scale: 'css', path: test.info().outputPath('dashboard-chart.png'), fullPage: true });
  await page.evaluate(() => document.documentElement.classList.add('dark'));
  await page.screenshot({ scale: 'css', path: test.info().outputPath('dashboard-chart-dark.png'), fullPage: true });
  await page.evaluate(() => document.documentElement.style.setProperty('--color-accent', '#7c3aed'));
  await expect(figure.locator('.traffic-download')).toHaveCSS('background-color', 'rgb(124, 58, 237)');
  await expect(figure.locator('pattern')).toHaveCount(1);
});
