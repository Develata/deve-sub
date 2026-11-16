// Shared CLI adapter; acquisition and runner authorization are outside this job.
const { loadRuntime, prepareBrowser, publishReady, verifyAsset } = require('./browser-preparation.cjs');

async function main() {
  const result = await prepareBrowser(loadRuntime());
  publishReady(result, process.env.GITHUB_ENV);
  console.log(JSON.stringify(result));
}

// Keep the existing asset-verification regression entrypoint unchanged.
module.exports = { verifyAsset };
if (require.main === module) {
  main().catch(error => {
    console.error(JSON.stringify({ status: 'NOT_RUN', reason: error.message,
      browser_download_attempted: false, launch_verified: false }));
    process.exitCode = 1;
  });
}
