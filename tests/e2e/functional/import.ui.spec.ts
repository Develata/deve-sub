import { test, expect, navigate } from './fixture';
import type { Route } from '@playwright/test';

const content = 'trojan://fixture@import.example:443#Imported-node';
const importButton = /^导入节点$|^Import Nodes$/;

test('FUNC-IMPORT-PENDING preserves the draft and blocks dismissal until failure or completion', async ({ api, page }, info) => {
  await navigate(page, /^节点管理$|^Nodes$/);
  await page.getByRole('button', { name: importButton }).click();
  const modal = page.getByRole('dialog');
  await modal.locator('textarea').fill(content);
  let held: Route | undefined;
  let requests = 0;
  await page.route('**/api/v1/nodes/import', route => { requests++; held = route; });
  await modal.getByRole('button', { name: importButton }).click();
  await expect.poll(() => !!held).toBeTruthy();
  await expect(modal.locator('textarea')).toBeDisabled();
  await expect(modal.locator('select')).toBeDisabled();
  await expect(modal.getByRole('button', { name: /^取消$|^Cancel$/ })).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(modal).toBeVisible();
  // A second submit event must also be rejected while the first request is held.
  await modal.locator('form').evaluate(form => (form as HTMLFormElement).requestSubmit());
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(requests).toBe(1);
  await held!.fulfill({ status: 503, json: { error: 'unavailable', message: 'fixture import unavailable' } });
  await expect(modal.getByRole('alert')).toContainText('fixture import unavailable');
  await expect(modal.getByRole('alert')).toBeFocused();
  await expect(modal.locator('textarea')).toHaveValue(content);
  await expect(modal.locator('textarea')).toBeEnabled();
  await page.unroute('**/api/v1/nodes/import');
  const saved = page.waitForResponse(r => r.url().endsWith('/nodes/import') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: importButton }).click();
  expect((await saved).status()).toBe(200);
  await expect(modal.getByRole('button', { name: /^完成$|^Done$/ })).toBeFocused();
  await modal.getByRole('button', { name: /^完成$|^Done$/ }).click();
  await expect(page.locator('main')).toContainText('Imported-node');
  expect((await (await api.get('/api/v1/nodes')).json()).nodes).toHaveLength(1);
  await page.screenshot({ path: info.outputPath('import-complete.png'), fullPage: false });
});

test('FUNC-IMPORT-RETURN closes success with Escape and keeps the node list current', async ({ page }) => {
  await navigate(page, /^节点管理$|^Nodes$/);
  const opener = page.getByRole('button', { name: importButton });
  await opener.click();
  const modal = page.getByRole('dialog');
  await modal.locator('textarea').fill(content);
  await modal.getByRole('button', { name: importButton }).click();
  await expect(modal.getByRole('button', { name: /^完成$|^Done$/ })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(modal).toHaveCount(0);
  await expect(page.locator('main')).toContainText('Imported-node');
  await expect(opener).toBeFocused();
});

test('FUNC-IMPORT-LAYOUT fits a short viewport and supports keyboard cancellation and empty input', async ({ page }, info) => {
  await page.setViewportSize({ width: 390, height: 420 });
  await navigate(page, /^节点管理$|^Nodes$/);
  const opener = page.getByRole('button', { name: importButton });
  await opener.click();
  const panel = page.getByRole('dialog');
  await page.screenshot({ path: info.outputPath('import-short-viewport.png'), fullPage: false });
  const box = await panel.boundingBox();
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height).toBeLessThanOrEqual(420);
  const modal = page.getByRole('dialog');
  await expect(modal).toHaveAccessibleName(/手动导入节点|Import Nodes Manually/);
  await modal.getByLabel(/订阅内容|Content/i).fill(' \n ');
  await modal.getByRole('button', { name: importButton }).click();
  await expect(modal.getByRole('alert')).toContainText(/内容不能为空|Content cannot be empty/);
  await page.screenshot({ path: info.outputPath('import-empty-feedback.png'), fullPage: false });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBeTruthy();
  const submit = modal.getByRole('button', { name: importButton });
  await submit.focus();
  await page.keyboard.press('Tab');
  await expect(modal.locator('select')).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(submit).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(modal).toHaveCount(0);
  await expect(opener).toBeFocused();
});
