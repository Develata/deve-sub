// One preparation owner for both matrices. No asset acquisition or fallback.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

const packageNames = ['@playwright/test', 'playwright', 'playwright-core'];
const digest = executable => crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex');

function lockedIdentity(lock, installed, registry) {
  const version = lock.packages?.['node_modules/playwright-core']?.version;
  if (!version || packageNames.some(name =>
    lock.packages?.[`node_modules/${name}`]?.version !== version || installed[name] !== version)) {
    throw new Error('installed Playwright packages do not match the exact lockfile');
  }
  const expected = registry.browsers?.find(browser => browser.name === 'chromium');
  if (!expected || !/^\d+$/.test(expected.revision) || !/^\d+\.\d+\.\d+\.\d+$/.test(expected.browserVersion)) {
    throw new Error('full Chromium identity is absent from the locked Playwright registry');
  }
  return { ...expected, playwrightVersion: version };
}

function loadRuntime() {
  const lock = JSON.parse(fs.readFileSync(path.join(__dirname, 'package-lock.json'), 'utf8'));
  const installed = Object.fromEntries(packageNames.map(name =>
    [name, JSON.parse(fs.readFileSync(require.resolve(`${name}/package.json`), 'utf8')).version]));
  const core = path.dirname(require.resolve('playwright-core/package.json'));
  const registry = JSON.parse(fs.readFileSync(path.join(core, 'browsers.json'), 'utf8'));
  const expected = lockedIdentity(lock, installed, registry);
  // executablePath selects full Chromium. launch() without it may select shell.
  const chromium = require('playwright').chromium;
  return { expected, chromium };
}

function verifyAsset(executable, expected, run = spawnSync) {
  if (!expected?.revision || !expected?.browserVersion || !executable || /[\r\n\0]/.test(executable)) {
    throw new Error('invalid locked browser identity or executable path');
  }
  if (!fs.existsSync(executable) || !fs.lstatSync(executable).isFile()) {
    throw new Error('the existing locked browser asset is absent; no download attempted');
  }
  const result = run(executable, ['--version'], { encoding: 'utf8', timeout: 10_000 });
  const actual = result.stdout?.match(/(?:^|\s)(\d+\.\d+\.\d+\.\d+)(?:\s|$)/)?.[1];
  if (result.error || result.status !== 0 || actual !== expected.browserVersion) {
    throw new Error('the existing browser does not execute with the exact locked version');
  }
  return { status: 'ASSET_VERIFIED', revision: expected.revision, browser_version: actual,
    executable_path: executable, executable_sha256: digest(executable), browser_download_attempted: false };
}

async function prepareBrowser({ expected, chromium }, run = spawnSync) {
  if (expected.name !== 'chromium') throw new Error('only full Chromium may be prepared');
  const executable = chromium.executablePath();
  const asset = verifyAsset(executable, expected, run);
  let browser;
  try {
    browser = await chromium.launch({ executablePath: executable, headless: true, timeout: 10_000 });
    if (!browser.isConnected() || browser.version() !== expected.browserVersion) {
      throw new Error('launched full Chromium does not match the locked version');
    }
  } finally {
    if (browser) await browser.close();
  }
  if (digest(executable) !== asset.executable_sha256) {
    throw new Error('browser executable changed during preparation');
  }
  return { ...asset, status: 'READY', playwright_version: expected.playwrightVersion,
    browser_kind: 'chromium', asset_provider: 'existing-playwright-cache', launch_verified: true };
}

function publishReady(result, envFile) {
  if (result.status !== 'READY' || result.launch_verified !== true || result.browser_kind !== 'chromium'
      || !result.executable_path || /[\r\n\0]/.test(result.executable_path)) {
    throw new Error('only fully prepared Chromium may be exported');
  }
  if (envFile) fs.appendFileSync(envFile, `DEVE_SUB_BROWSER=${result.executable_path}\n`);
}

module.exports = { lockedIdentity, loadRuntime, verifyAsset, prepareBrowser, publishReady };
