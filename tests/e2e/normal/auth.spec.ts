import { createHmac } from 'crypto';
import { writeFileSync } from 'fs';
import { test, expect, setupInBrowser, navigate } from './fixture';

// Enrollment secrets and recovery codes remain in memory, outside traces/screenshots.
test.use({ trace: 'off', screenshot: 'off' });
const credentials = { username: 'normal-admin', password: 'SyntheticPassword2026' };
function totp(base32: string) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const char of base32.replace(/=+$/, '')) bits += alphabet.indexOf(char).toString(2).padStart(5, '0');
  const secret = Buffer.from(bits.match(/.{8}/g)!.map(byte => parseInt(byte, 2)));
  const counter = Buffer.alloc(8); counter.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30_000)));
  const hash = createHmac('sha1', secret).update(counter).digest(), offset = hash[19] & 15;
  return String((hash.readUInt32BE(offset) & 0x7fffffff) % 1_000_000).padStart(6, '0');
}

for (const entry of ['UI', 'API']) {
  test(`normal ${entry} TOTP recovery regeneration disable and restart`, async ({ page, playwright, instance }, info) => {
    const api = await playwright.request.newContext({ baseURL: instance.url, extraHTTPHeaders: { Origin: instance.url } });
    const reader = entry === 'UI' ? page.request : api;
    try {
      if (entry === 'UI') await setupInBrowser(page);
      else {
        expect((await api.get('/api/v1/auth/status')).status()).toBe(200);
        expect((await api.post('/api/v1/auth/setup', { data: credentials })).status()).toBe(201);
        expect((await api.post('/api/v1/auth/login', { data: credentials })).status()).toBe(200);
      }
      let secret: string, recovery: string[];
      if (entry === 'UI') {
        await navigate(page, /^Settings$/);
        await page.getByRole('button', { name: 'Enable 2FA', exact: true }).click();
        secret = await page.locator('main code').first().innerText();
        await page.locator('main input[maxlength="6"]').fill(totp(secret));
        await page.getByRole('button', { name: /^Verify$/ }).click();
        await expect(page.locator('main ul li')).toHaveCount(10);
        recovery = await page.locator('main ul li').allTextContents();
        await page.getByRole('button', { name: 'Close', exact: true }).click();
      } else {
        const setup = await api.post('/api/v1/auth/2fa/setup'); expect(setup.status()).toBe(200);
        secret = (await setup.json()).secret;
        const verified = await api.post('/api/v1/auth/2fa/verify', { data: { code: totp(secret) } });
        expect(verified.status()).toBe(200); recovery = (await verified.json()).recovery_codes;
      }
      expect(recovery.length).toBe(10);
      expect((await (await reader.get('/api/v1/auth/me')).json()).user.two_factor_enabled).toBe(true);
      await instance.restart();
      expect((await (await reader.get('/api/v1/auth/me')).json()).user.two_factor_enabled).toBe(true);
      const login = async (code: string) => {
        if (entry === 'UI') {
          await navigate(page, /^Settings$/);
          await page.locator('aside').getByRole('button', { name: /^Logout$|^Sign Out$|^退出登录$/ }).click();
          await page.locator('input[autocomplete=username]').fill(credentials.username);
          await page.locator('input[autocomplete=current-password]').fill(credentials.password);
          await page.getByRole('button', { name: /^Sign In$|^Login$|^登录$/ }).click();
          const input = page.locator('input[autocomplete=one-time-code]');
          await input.fill(code);
          // Length assertion avoids printing a recovery secret on failure.
          expect((await input.inputValue()).length).toBe(code.length);
          await page.getByRole('button', { name: /^Verify$/ }).click();
          await expect(page.locator('main')).toBeVisible();
        } else {
          expect((await api.post('/api/v1/auth/logout')).status()).toBe(200);
          const challenge = await api.post('/api/v1/auth/login', { data: credentials });
          expect(challenge.status()).toBe(200);
          const result = await challenge.json(); expect(result.requires_2fa).toBe(true);
          expect((await api.post('/api/v1/auth/login/2fa', { data: { challenge_token: result.challenge_token, code } })).status()).toBe(200);
        }
      };
      await login(totp(secret));
      await login(recovery[0]);
      let replacement: string[];
      if (entry === 'UI') {
        await navigate(page, /^Settings$/);
        await page.getByRole('button', { name: 'Regenerate Recovery Codes', exact: true }).click();
        await page.locator('main input[type=password]').fill(credentials.password);
        await page.getByRole('button', { name: 'Regenerate Recovery Codes', exact: true }).click();
        await expect(page.locator('main ul li')).toHaveCount(10);
        replacement = await page.locator('main ul li').allTextContents();
        await page.getByRole('button', { name: 'Close', exact: true }).click();
      } else {
        const response = await api.post('/api/v1/auth/2fa/recovery-codes', { data: { password: credentials.password } });
        expect(response.status()).toBe(200); replacement = (await response.json()).recovery_codes;
      }
      expect(replacement.length).toBe(10);
      expect(replacement.some(code => recovery.includes(code))).toBe(false);
      await login(replacement[0]);
      if (entry === 'UI') {
        await navigate(page, /^Settings$/);
        await page.getByRole('button', { name: 'Disable 2FA', exact: true }).click();
        await page.locator('main input[type=password]').fill(credentials.password);
        await page.getByRole('button', { name: 'Disable 2FA', exact: true }).click();
        await expect(page.getByRole('button', { name: 'Enable 2FA', exact: true })).toBeVisible();
      } else expect((await api.post('/api/v1/auth/2fa/disable', { data: { password: credentials.password } })).status()).toBe(200);
      await instance.restart();
      expect((await (await reader.get('/api/v1/auth/me')).json()).user.two_factor_enabled).toBe(false);
      writeFileSync(info.outputPath('auth-proof.json'), JSON.stringify({
        entry, at: new Date().toISOString(), status: 'PASS', api_preseed: false,
        operations: ['setup', 'login', 'logout', '2fa/setup', '2fa/verify', 'login/2fa:totp', 'login/2fa:recovery', '2fa/recovery-codes', '2fa/disable', 'me', 'restart'],
        recovery_code_count: recovery.length, replacement_count: replacement.length,
      }, null, 2));
    } finally { await api.dispose(); }
  });
}
