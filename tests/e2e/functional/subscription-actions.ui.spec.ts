import { test, expect, navigate, createTemplate, importNodes } from './fixture';

const name = 'Fixture subscription with a long readable name';
async function createSubscription(api: import('@playwright/test').APIRequestContext) {
  await importNodes(api);
  const template = await createTemplate(api);
  const response = await api.post('/api/v1/subscriptions', { data: { name, slug: 'fixture-subscription-readable-name',
    template_id: template.id, profile: 'mihomo', node_selection: { mode: 'dynamic' } } });
  expect(response.status()).toBe(201);
  return (await response.json()).subscription.id as string;
}

test('FUNC-SUB-ACTIONS primary actions fit and destructive choices require consequence confirmation', async ({ api, page }) => {
  const id = await createSubscription(api);
  const priorCode = (await (await api.post(`/api/v1/subscriptions/${id}/regenerate-short-code`)).json()).code;
  await navigate(page, /^长期订阅$|^Subscriptions$/);
  const row = page.getByRole('row').filter({ hasText: name });
  await expect(row).toBeVisible();
  await expect(row.getByRole('button')).toHaveCount(2);
  const copy = row.getByRole('button', { name: /^复制$|^Copy$/ });
  const edit = row.getByRole('button', { name: /^编辑$|^Edit$/ });
  for (const control of [copy, edit]) {
    const rect = await control.boundingBox();
    expect(rect!.height).toBeGreaterThanOrEqual(44);
    expect(rect!.x).toBeGreaterThanOrEqual(0);
    expect(rect!.x + rect!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ scale: 'css', path: test.info().outputPath('subscription-actions.png') });
  await row.locator('summary').focus();
  await page.keyboard.press('Enter');
  await expect(row.getByRole('button')).toHaveCount(6);
  let writes = 0;
  page.on('request', r => { if (r.method() === 'POST' && r.url().endsWith('/regenerate-short-code')) writes++; });
  const regenerate = row.getByRole('button', { name: /重生成短码|Regenerate Code/ });
  await regenerate.click();
  const dialog = page.getByRole('dialog');
  const cancel = dialog.getByRole('button', { name: /^取消$|^Cancel$/ });
  await expect(dialog).toContainText(name);
  await expect(dialog).toContainText(/现有短链接将立即失效|existing short link will stop working/);
  await expect(cancel).toBeFocused();
  await page.screenshot({ scale: 'css', path: test.info().outputPath('subscription-confirmation.png') });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(regenerate).toBeFocused();
  expect(writes).toBe(0);
  await regenerate.click();
  await dialog.getByRole('button', { name: /重生成短码|Regenerate Code/ }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText(/短码已更新|Short code updated/);
  expect(writes).toBe(1);
  expect((await api.get(`/s/${priorCode}/mihomo`)).status()).toBe(404);
  const current = (await (await api.get(`/api/v1/subscriptions/${id}`)).json()).subscription.short_code;
  expect((await api.get(`/s/${current}/mihomo`)).status()).toBe(200);
  await row.locator('summary').click();
  await row.getByRole('button', { name: /^删除$|^Delete$/ }).click();
  await expect(dialog).toContainText(/无法撤销|cannot be undone/);
  await expect(cancel).toBeFocused();
  await cancel.click();
  expect((await api.get(`/api/v1/subscriptions/${id}`)).status()).toBe(200);
  await row.getByRole('button', { name: /^删除$|^Delete$/ }).click();
  await dialog.getByRole('button', { name: /^删除$|^Delete$/ }).click();
  await expect(row).toHaveCount(0);
  expect((await api.get(`/api/v1/subscriptions/${id}`)).status()).toBe(404);
});

test('FUNC-SUB-DIALOG short viewport and varied focusable content retain focus inside the shared dialog', async ({ api, page }) => {
  await createSubscription(api);
  await page.setViewportSize({ width: 390, height: 420 });
  await navigate(page, /^长期订阅$|^Subscriptions$/);
  const edit = page.getByRole('button', { name: /^编辑$|^Edit$/ });
  await edit.click();
  const dialog = page.getByRole('dialog');
  const rect = await dialog.boundingBox();
  expect(rect!.y).toBeGreaterThanOrEqual(0);
  expect(rect!.y + rect!.height).toBeLessThanOrEqual(420);
  await expect(dialog.getByLabel('名称', { exact: true })).toBeFocused();
  await dialog.evaluate(el => {
    const first = document.createElement('a'); first.href = '#fixture'; first.id = 'focus-anchor'; first.textContent = 'Fixture link'; el.prepend(first);
    const extra = document.createElement('div'); extra.id = 'focus-fixtures';
    extra.innerHTML = '<details><summary id="focus-summary">Fixture details</summary><button id="closed-detail">Hidden in details</button></details>' +
      '<div contenteditable="true" id="focus-editable">Editable fixture</div><div tabindex="0" id="focus-custom">Custom focus</div>' +
      '<input type="hidden"><button hidden>Hidden</button><button style="display:none">Display none</button><button style="visibility:hidden">Invisible</button>' +
      '<fieldset disabled><input><button>Disabled fieldset</button></fieldset><button disabled>Disabled</button><div tabindex="-1">Negative</div>' +
      '<div contenteditable="false">Not editable</div><div inert><button>Inert</button></div>';
    el.append(extra);
  });
  const anchor = dialog.locator('#focus-anchor');
  const custom = dialog.locator('#focus-custom');
  await anchor.focus();
  await page.keyboard.press('Shift+Tab');
  await expect(custom).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(anchor).toBeFocused();
  await dialog.locator('#focus-summary').focus();
  await page.keyboard.press('Tab');
  await expect(dialog.locator('#focus-editable')).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(custom).toBeFocused();
  // Make each supported type the boundary in turn; hidden/disabled nodes follow it.
  await custom.evaluate(el => el.remove());
  await dialog.locator('#focus-editable').focus();
  await page.keyboard.press('Tab');
  await expect(anchor).toBeFocused();
  await dialog.locator('#focus-editable').evaluate(el => el.remove());
  await dialog.locator('#focus-summary').focus();
  await page.keyboard.press('Tab');
  await expect(anchor).toBeFocused();
  await dialog.locator('#focus-fixtures').evaluate(el => el.remove());
  await anchor.evaluate(el => el.remove());
  await page.screenshot({ scale: 'css', path: test.info().outputPath('subscription-short-dialog.png') });
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(edit).toBeFocused();
});
