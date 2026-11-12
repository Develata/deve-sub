import { type Page, expect } from '@playwright/test';
import { navigate } from './fixture';

export async function openSubscription(page: Page, name: string, template: string, profile = 'json') {
  await navigate(page, /^长期订阅$|^Subscriptions$/);
  await page.getByRole('button', { name: /^添加订阅$|^Add Subscription$/ }).click();
  const modal = page.getByRole('dialog');
  await modal.locator('#subscription-name').fill(name);
  await modal.locator('#subscription-slug').fill(name);
  await modal.locator('#subscription-template').selectOption(template);
  await modal.locator('#subscription-profile').selectOption(profile);
  return modal;
}

export async function saveAndCopy(page: Page) {
  const modal = page.getByRole('dialog');
  const response = page.waitForResponse(r => r.url().endsWith('/subscriptions') && r.request().method() === 'POST');
  await modal.getByRole('button', { name: /^保存$|^Save$/ }).click();
  expect((await response).status()).toBe(201);
  const url = await modal.locator('#subscription-url').inputValue();
  await modal.getByRole('button', { name: /^复制$|^Copy$/ }).click();
  await expect(modal.getByRole('status')).toContainText(/已复制|Copied/);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(url);
  await modal.getByRole('button', { name: 'OK', exact: true }).click();
  return url;
}
