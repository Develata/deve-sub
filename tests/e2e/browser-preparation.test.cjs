// Synthetic preparation tests: injected launch objects never start a browser.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { lockedIdentity, prepareBrowser, publishReady } = require('./browser-preparation.cjs');

const names = ['@playwright/test', 'playwright', 'playwright-core'];
const expected = { name: 'chromium', revision: '1234', browserVersion: '151.0.7922.34', playwrightVersion: '1.62.1' };
const lock = { packages: Object.fromEntries(names.map(name => [`node_modules/${name}`, { version: '1.62.1' }])) };
const installed = Object.fromEntries(names.map(name => [name, '1.62.1']));
const registry = { browsers: [expected, { ...expected, name: 'chromium-headless-shell' }] };
const versionProbe = () => ({ status: 0, stdout: 'Google Chrome for Testing 151.0.7922.34\n' });

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deve-preparation-unit-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const executable = path.join(root, 'synthetic-full-chromium');
  fs.writeFileSync(executable, 'synthetic unit bytes; not executable browser code');
  const envFile = path.join(root, 'github-env');
  fs.writeFileSync(envFile, 'UNCHANGED=1\n');
  const calls = [];
  const browser = {
    isConnected: () => true,
    version: () => expected.browserVersion,
    close: async () => { calls.push('close'); },
  };
  const chromium = {
    executablePath: () => executable,
    launch: async options => { calls.push(options); return browser; },
  };
  return { executable, envFile, calls, browser, runtime: { chromium, expected } };
}

test('lock owns package and full Chromium identities', () => {
  const identity = lockedIdentity(lock, installed, registry);
  assert.equal(identity.name, 'chromium');
  assert.equal(identity.playwrightVersion, '1.62.1');
  assert.equal(identity.revision, '1234');
  assert.equal(identity.browserVersion, '151.0.7922.34');
});

test('each installed package and lock entry must agree exactly', () => {
  for (const name of names) {
    assert.throws(() => lockedIdentity(lock, { ...installed, [name]: '1.65.0' }, registry), /exact lockfile/);
    const changed = structuredClone(lock);
    changed.packages[`node_modules/${name}`].version = '1.65.0';
    assert.throws(() => lockedIdentity(changed, installed, registry), /exact lockfile/);
  }
});

test('headless shell and incomplete registry metadata cannot become the target', () => {
  for (const browsers of [[{ ...expected, name: 'chromium-headless-shell' }],
    [{ ...expected, revision: '' }], [{ ...expected, browserVersion: '151' }]]) {
    assert.throws(() => lockedIdentity(lock, installed, { browsers }), /full Chromium identity/);
  }
});

test('READY and export follow explicit full-browser launch and successful close', async t => {
  const f = fixture(t);
  f.browser.close = async () => {
    assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
    f.calls.push('close');
  };
  const result = await prepareBrowser(f.runtime, versionProbe);
  assert.deepEqual(f.calls, [{ executablePath: f.executable, headless: true, timeout: 10_000 }, 'close']);
  assert.equal(result.status, 'READY');
  assert.equal(result.launch_verified, true);
  assert.equal(result.asset_provider, 'existing-playwright-cache');
  assert.equal(result.browser_download_attempted, false);
  publishReady(result, f.envFile);
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), `UNCHANGED=1\nDEVE_SUB_BROWSER=${f.executable}\n`);
});

test('missing asset stops before any version command or browser launch', async t => {
  const f = fixture(t);
  fs.unlinkSync(f.executable);
  await assert.rejects(prepareBrowser(f.runtime, () => assert.fail('no command permitted')), /absent/);
  assert.deepEqual(f.calls, []);
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
});

test('wrong asset version stops before launch or export', async t => {
  const f = fixture(t);
  await assert.rejects(prepareBrowser(f.runtime, () => ({ status: 0, stdout: 'Chromium 154.0.8037.57\n' })), /exact locked version/);
  assert.deepEqual(f.calls, []);
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
});

test('a shell identity is refused before inspecting an executable', async t => {
  const f = fixture(t);
  f.runtime.expected = { ...expected, name: 'chromium-headless-shell' };
  f.runtime.chromium.executablePath = () => assert.fail('no path lookup permitted');
  await assert.rejects(prepareBrowser(f.runtime), /only full Chromium/);
});

test('launch failure leaves no exported path', async t => {
  const f = fixture(t);
  f.runtime.chromium.launch = async () => { throw new Error('synthetic launch failure'); };
  await assert.rejects(prepareBrowser(f.runtime, versionProbe), /synthetic launch failure/);
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
});

test('launched version mismatch and disconnection both close the owned browser', async t => {
  for (const override of [{ version: () => '154.0.8037.57' }, { isConnected: () => false }]) {
    const f = fixture(t);
    Object.assign(f.browser, override);
    await assert.rejects(prepareBrowser(f.runtime, versionProbe), /launched full Chromium/);
    assert.equal(f.calls.at(-1), 'close');
    assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
  }
});

test('cleanup failure cannot report READY or export', async t => {
  const f = fixture(t);
  f.browser.close = async () => { throw new Error('synthetic close failure'); };
  await assert.rejects(prepareBrowser(f.runtime, versionProbe), /synthetic close failure/);
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
});

test('changed executable bytes invalidate an otherwise successful launch', async t => {
  const f = fixture(t);
  f.browser.close = async () => { fs.appendFileSync(f.executable, 'changed'); };
  await assert.rejects(prepareBrowser(f.runtime, versionProbe), /changed during preparation/);
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
});

test('asset-only evidence, shell selectors and environment injection are not exportable', t => {
  const f = fixture(t);
  const valid = { status: 'READY', launch_verified: true, browser_kind: 'chromium', executable_path: f.executable };
  for (const change of [{ status: 'ASSET_VERIFIED' }, { launch_verified: false },
    { browser_kind: 'chromium-headless-shell' }, { executable_path: f.executable + '\nOTHER=1' }]) {
    assert.throws(() => publishReady({ ...valid, ...change }, f.envFile), /fully prepared/);
  }
  assert.equal(fs.readFileSync(f.envFile, 'utf8'), 'UNCHANGED=1\n');
});
