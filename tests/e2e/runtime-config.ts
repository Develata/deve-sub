import { spawnSync } from 'child_process';
import { randomUUID } from 'crypto';

// Workers inherit these values; independent invocations never share report
// directories or default ports. Explicit ports remain useful for debugging.
process.env.DEVE_SUB_E2E_RUN_ID ??= randomUUID();
export const RUN_ID = process.env.DEVE_SUB_E2E_RUN_ID;
if (!/^[a-zA-Z0-9_-]+$/.test(RUN_ID)) throw new Error('invalid E2E run identifier');

if (!process.env.DEVE_SUB_E2E_PORTS) {
  const probe = spawnSync(process.execPath, ['-e', `
    const net = require('net');
    const servers = [net.createServer(), net.createServer()];
    Promise.all(servers.map(s => new Promise((resolve, reject) => {
      s.on('error', reject);
      s.listen(0, '127.0.0.1', () => resolve(s.address().port));
    }))).then(ports => {
      console.log(JSON.stringify(ports));
      servers.forEach(s => s.close());
    }).catch(() => process.exit(1));
  `], { encoding: 'utf8', timeout: 5000 });
  if (probe.status !== 0) throw new Error('could not allocate E2E ports');
  process.env.DEVE_SUB_E2E_PORTS = probe.stdout.trim();
}
const ports: number[] = JSON.parse(process.env.DEVE_SUB_E2E_PORTS);
export const FRESH_PORT = Number(process.env.DEVE_SUB_E2E_FRESH_PORT ?? ports[0]);
export const SEEDED_PORT = Number(process.env.DEVE_SUB_E2E_SEEDED_PORT ?? ports[1]);
for (const port of [FRESH_PORT, SEEDED_PORT]) {
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('invalid E2E port');
}
if (FRESH_PORT === SEEDED_PORT) throw new Error('E2E servers require distinct ports');
