import { test, expect, setupInBrowser, navigate } from './fixture';

for (const width of [1280, 390]) {
  test(`settings visibly persist name accent and reduced motion at ${width}px`, async ({ page, instance }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await setupInBrowser(page);
    await navigate(page, /^Settings$/);
    await page.getByLabel('Product Name', { exact: true }).fill('Synthetic Proxy Console');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.locator('header h1')).toHaveText('Synthetic Proxy Console');
    await expect(page).toHaveTitle('Synthetic Proxy Console');
    await page.getByLabel('Custom Theme', { exact: true }).check();
    await page.locator('input[type=color]').fill('#2563eb');
    await page.getByLabel('Reduce Motion', { exact: true }).check();
    await navigate(page, /^Nodes$/);
    await page.mouse.move(0, 0);
    await expect(page.getByRole('button', { name: 'Import Nodes', exact: true })).toHaveCSS('background-color', 'rgb(37, 99, 235)');
    await instance.restart();
    await page.reload();
    await expect(page.locator('header h1')).toHaveText('Synthetic Proxy Console');
    await expect(page).toHaveTitle('Synthetic Proxy Console');
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--color-accent').trim())).toBe('#2563eb');
    await navigate(page, /^Settings$/);
    await expect(page.getByLabel('Reduce Motion', { exact: true })).toBeChecked();
    expect(await page.getByRole('button', { name: 'Minimal Warm', exact: true })
      .evaluate(el => parseFloat(getComputedStyle(el).transitionDuration))).toBeLessThanOrEqual(0.001);
    await expect(page.getByLabel('Product Name', { exact: true })).toHaveValue('Synthetic Proxy Console');
    await page.screenshot({ path: info.outputPath('persisted-settings.png'), fullPage: true });
    await page.getByLabel('Product Name', { exact: true }).fill('');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(page.locator('header h1')).toHaveText('Deve Sub');
    await expect(page).toHaveTitle('Deve Sub');
    await page.getByLabel('Custom Theme', { exact: true }).uncheck();
    await page.reload();
    await expect(page.locator('header h1')).toHaveText('Deve Sub');
    expect(await page.evaluate(() => localStorage.getItem('custom-accent'))).toBeNull();
  });
}
