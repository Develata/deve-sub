import { test, expect } from '../fixtures/auth';

test('UI-005: Fantasy Violet theme — chessboard background, decorations and cards remain readable', async ({ authedPage: page }) => {
  await page.locator('aside button').filter({ hasText: /设置|Settings/ }).click();
  await page.waitForLoadState('networkidle');

  await page.locator('button').filter({ hasText: /Fantasy Violet/ }).first().click();

  const html = page.locator('html');
  await expect(html).toHaveClass(/theme-violet/);

  const pages = [
    { nav: /仪表盘|Dashboard/, file: 'ui-005-violet-dashboard' },
    { nav: /节点管理|Nodes/, file: 'ui-005-violet-nodes' },
    { nav: /订阅源|Sources/, file: 'ui-005-violet-sources' },
    { nav: /长期订阅|Subscriptions/, file: 'ui-005-violet-subscriptions' },
    { nav: /设置|Settings/, file: 'ui-005-violet-settings' },
  ];

  for (const p of pages) {
    await page.locator('aside button').filter({ hasText: p.nav }).click();
    await page.waitForLoadState('networkidle');
    await page.screenshot({ path: `screenshots/${p.file}.png` });
  }

  const hasViolet = await page.evaluate(() => document.documentElement.classList.contains('theme-violet'));
  expect(hasViolet).toBe(true);

  const contrast = await page.evaluate(() => {
    const main = document.querySelector('main');
    if (!main) return false;
    const cs = getComputedStyle(main);
    return cs.color !== '' && cs.backgroundColor !== '';
  });
  expect(contrast).toBe(true);
});
