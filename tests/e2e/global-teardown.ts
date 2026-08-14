import { spawnSync } from 'child_process';
import { readFileSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';

const FRESH_PORT = 18080;
const SEEDED_PORT = 18081;

export default async function globalTeardown(): Promise<void> {
  // PIDs are written by global-setup to a temp file so teardown can kill them.
  const pidFile = join(tmpdir(), 'deve-sub-e2e-pids.json');
  let pids: number[] = [];
  try {
    pids = JSON.parse(readFileSync(pidFile, 'utf-8'));
  } catch {
    console.warn('[teardown] no PID file found — servers may linger');
    return;
  }

  for (const pid of pids) {
    try {
      process.kill(pid, 'SIGTERM');
      console.log(`[teardown] sent SIGTERM to pid ${pid}`);
    } catch {
      // already dead
    }
  }

  // Give them a moment to shut down gracefully.
  await new Promise(r => setTimeout(r, 2000));

  // Force kill any survivors.
  for (const pid of pids) {
    try {
      process.kill(pid, 'SIGKILL');
    } catch {
      // already dead
    }
  }

  console.log('[teardown] servers stopped');
}
