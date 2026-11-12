import { writeFileSync } from 'fs';
import { test, expect, setupInBrowser, navigate, importInBrowser, templateInBrowser } from './fixture';

const pages = [
  { name: 'Dashboard', nav: /^仪表盘$|^Dashboard$/ },
  { name: 'Nodes', nav: /^节点管理$|^Nodes$/, dialog: /^导入节点$|^Import Nodes$/ },
  { name: 'Sources', nav: /^订阅源$|^Sources$/, dialog: /^添加订阅源$|^Add Source$/ },
  { name: 'Templates', nav: /^模板$|^Templates$/, dialog: /^新建模板$|^New Template$/ },
  { name: 'Subscriptions', nav: /^长期订阅$|^Subscriptions$/, dialog: /^添加订阅$|^Add Subscription$/ },
  { name: 'Users', nav: /^用户$|^Users$/, dialog: /^新建用户$|^New User$/ },
  { name: 'Audit', nav: /^审计日志$|^Audit Log$/ },
  { name: 'Settings', nav: /^设置$|^Settings$/ },
];
const themes = [
  { name: 'Light', select: /^Light$|^浅色$/, stored: 'light', canvas: 'rgb(250, 250, 249)', surface: 'rgb(255, 255, 255)' },
  { name: 'Dark', select: /^Dark$|^深色$/, stored: 'dark', canvas: 'rgb(12, 10, 9)', surface: 'rgb(28, 25, 23)' },
  { name: 'Minimal Warm', select: /^Minimal Warm$/, stored: 'warm', canvas: 'rgb(255, 244, 230)', surface: 'rgb(255, 251, 245)' },
  { name: 'Fantasy Violet', select: /^Fantasy Violet$/, stored: 'violet', canvas: 'rgb(242, 237, 255)', surface: 'rgb(250, 247, 255)' },
];

for (const [themeIndex, theme] of themes.entries()) {
  for (const [langIndex, lang] of ['zh', 'en'].entries()) {
    for (const [viewIndex, width] of [1280, 390].entries()) {
      test(`${theme.name} ${lang} ${width}px eight real pages and dialogs`, async ({ page }, info) => {
        await page.setViewportSize({ width, height: 900 });
        await setupInBrowser(page);
        await importInBrowser(page, 'http://theme.example:8080#Theme-visible-node', 'uri_list');
        await templateInBrowser(page, 'Theme-visible-template', { mode: 'dynamic' });
        await navigate(page, /^设置$|^Settings$/);
        await page.getByRole('button', { name: theme.select, exact: true }).click();
        await page.locator('header').getByRole('button', { name: lang === 'zh' ? '中文' : 'EN', exact: true }).click();
        await expect.poll(() => page.evaluate(() => localStorage.getItem('theme'))).toBe(theme.stored);
        await page.reload();
        await expect(page.locator('main')).toBeVisible();
        expect(await page.evaluate(() => localStorage.getItem('lang'))).toBe(lang);
        const evidence: object[] = [], failures: string[] = [];
        for (const [pageIndex, target] of pages.entries()) {
          const id = `VIEW-${String(pageIndex * 16 + themeIndex * 4 + langIndex * 2 + viewIndex + 1).padStart(3, '0')}`;
          await navigate(page, target.nav);
          await expect(page.locator('main')).toBeVisible();
          await expect(page.locator('main')).not.toBeEmpty();
          const styles = await page.evaluate(() => {
            const css = (element: Element) => {
              const style = getComputedStyle(element), rect = element.getBoundingClientRect();
              return { background: style.backgroundColor, color: style.color, border: style.borderColor,
                width: rect.width, height: rect.height, x: rect.x, y: rect.y };
            };
            const canvas = document.createElement('canvas').getContext('2d')!;
            const luminance = (color: string) => {
              canvas.fillStyle = color; canvas.fillRect(0, 0, 1, 1);
              const rgb = [...canvas.getImageData(0, 0, 1, 1).data].slice(0, 3)
                .map(v => v / 255).map(v => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
              return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
            };
            const contrast = (a: string, b: string) => {
              const values = [luminance(a), luminance(b)].sort((a, b) => a - b);
              return (values[1] + 0.05) / (values[0] + 0.05);
            };
            const body = css(document.body), header = css(document.querySelector('header')!);
            const active = document.querySelector('aside button.bg-accent-soft')!;
            const activeStyle = css(active);
            const controls = [...document.querySelectorAll('main button, main input, main select, main textarea')]
              .filter(el => el.getBoundingClientRect().height > 0);
            return {
              body, header, sidebar: css(document.querySelector('aside')!), main: css(document.querySelector('main')!),
              active: activeStyle, bodyContrast: contrast(body.color, body.background),
              activeContrast: contrast(activeStyle.color, activeStyle.background),
              previewContrasts: [...document.querySelectorAll('main button[class*="theme-preview-"]')]
                .map(el => contrast(css(el.querySelector('span')!).color, css(el).background)),
              documentWidth: document.documentElement.scrollWidth, viewport: innerWidth,
              controlCount: controls.length, title: document.title,
              visibleText: document.querySelector('main')!.textContent?.slice(0, 120),
              theme: localStorage.getItem('theme'), language: localStorage.getItem('lang'),
            };
          });
          const issues: string[] = [];
          if (styles.body.background !== theme.canvas) issues.push('canvas color differs');
          if (styles.header.background !== theme.surface || styles.sidebar.background !== theme.surface) issues.push('shell surface differs');
          if (styles.documentWidth > width) issues.push('document overflows viewport');
          if (styles.bodyContrast < 4.5) issues.push(`body contrast ${styles.bodyContrast}`);
          if (styles.activeContrast < 4.5) issues.push(`active navigation contrast ${styles.activeContrast}`);
          if (styles.previewContrasts.some(value => value < 4.5)) issues.push('theme preview text contrast');
          await page.screenshot({ path: info.outputPath(`${id}-page.png`), fullPage: true });
          let dialogStyles: object | undefined;
          if (target.dialog) {
            await page.getByRole('button', { name: target.dialog }).click();
            const dialog = page.locator('dialog[open], .node-dialog-panel, div.fixed > div.bg-surface').last();
            await expect(dialog).toBeVisible();
            dialogStyles = await dialog.evaluate(element => {
              const style = getComputedStyle(element), rect = element.getBoundingClientRect();
              const input = element.querySelector('input,select,textarea');
              return { background: style.backgroundColor, color: style.color, x: rect.x, y: rect.y,
                width: rect.width, height: rect.height, viewportHeight: innerHeight,
                inputBackground: input ? getComputedStyle(input).backgroundColor : null };
            });
            const ds = dialogStyles as any;
            if (ds.background !== theme.surface) issues.push('dialog surface differs');
            if (ds.x < -1 || ds.x + ds.width > width + 1 || ds.y < -1 || ds.y + ds.height > 901) issues.push('dialog exceeds viewport');
            if (ds.inputBackground && ds.inputBackground !== theme.surface) issues.push('input surface differs');
            await page.screenshot({ path: info.outputPath(`${id}-dialog.png`) });
            await dialog.getByRole('button', { name: /^取消$|^Cancel$/ }).click();
          }
          evidence.push({ id, page: target.name, theme: theme.name, language: lang, width,
            at: new Date().toISOString(), status: issues.length ? 'FAIL' : 'AUTOMATED_PASS_PENDING_VISUAL_REVIEW',
            styles, dialogStyles, issues });
          failures.push(...issues.map(issue => `${id}: ${issue}`));
          writeFileSync(info.outputPath('computed-styles.json'), JSON.stringify(evidence, null, 2));
        }
        expect(failures, 'actual computed styles and layout').toEqual([]);
      });
    }
  }
}
