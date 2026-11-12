const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { verifyAsset } = require('./verify-locked-browser.cjs');
const expected = { revision: '1234', browserVersion: '151.0.7922.34' };

function fixture(callback) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deve-browser-guard-'));
  const executable = path.join(root, 'synthetic-unit-fixture');
  fs.writeFileSync(executable, 'synthetic fixture, not a browser');
  try { callback(executable); }
  finally { fs.rmSync(root, { recursive: true, force: true }); }
}

test('missing browser fails without attempting a command or acquisition', () => {
  let called = false;
  assert.throws(() => verifyAsset('/absent-locked-browser', expected, () => { called = true; }), /absent/);
  assert.equal(called, false);
});
test('different installed browser version is rejected', () => fixture(executable => {
  assert.throws(() => verifyAsset(executable, expected, () => ({ status: 0, stdout: 'Chromium 154.0.8037.57\n' })), /exact locked version/);
}));
test('failed or timed out version command is rejected', () => fixture(executable => {
  for (const result of [{ status: 1, stdout: 'Chromium 151.0.7922.34\n' }, { status: null, error: new Error('timeout') }]) {
    assert.throws(() => verifyAsset(executable, expected, () => result), /does not execute/);
  }
}));
test('unit fixture records the verified bytes and a bounded version check', () => fixture(executable => {
  const result = verifyAsset(executable, expected, (file, args, options) => {
    assert.equal(file, executable); assert.deepEqual(args, ['--version']); assert.equal(options.timeout, 10_000);
    return { status: 0, stdout: 'Google Chrome for Testing 151.0.7922.34\n' };
  });
  assert.equal(result.executable_sha256, crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex'));
  assert.equal(result.browser_download_attempted, false);
}));
test('symlink and environment-file newline injection are rejected', () => fixture(executable => {
  const link = executable + '-link'; fs.symlinkSync(executable, link);
  assert.throws(() => verifyAsset(link, expected), /absent/);
  assert.throws(() => verifyAsset(executable + '\nDEVE_SUB_BROWSER=other', expected), /invalid/);
}));
