// Capability check only: browser acquisition is outside this workflow.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

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
  return { status: 'READY', revision: expected.revision, browser_version: actual,
    executable_path: executable,
    executable_sha256: crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex'),
    browser_download_attempted: false };
}

function main() {
  const core = path.dirname(require.resolve('playwright-core/package.json'));
  const expected = JSON.parse(fs.readFileSync(path.join(core, 'browsers.json'), 'utf8'))
    .browsers.find(browser => browser.name === 'chromium');
  const executable = require('playwright').chromium.executablePath();
  const result = verifyAsset(executable, expected);
  // All three configs consume this verified path; no system-browser fallback.
  if (process.env.GITHUB_ENV) fs.appendFileSync(process.env.GITHUB_ENV, `DEVE_SUB_BROWSER=${executable}\n`);
  console.log(JSON.stringify(result));
}

module.exports = { verifyAsset };
if (require.main === module) {
  try { main(); }
  catch (error) {
    console.error(JSON.stringify({ status: 'NOT_RUN', reason: error.message, browser_download_attempted: false }));
    process.exitCode = 1;
  }
}
