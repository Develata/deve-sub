import { spawn, spawnSync, type ChildProcess } from 'child_process';
import { closeSync, mkdirSync, mkdtempSync, openSync, rmSync, writeFileSync } from 'fs';
import { createServer } from 'net';
import { tmpdir } from 'os';
import { join } from 'path';

interface Handle { proc: ChildProcess; directory: string; stopped: Promise<void> }

/** Own only children and temporary state created by this invocation. */
export class Servers {
  private handles: Handle[] = [];

  constructor(private binary: string, private webDist: string, private logDirectory: string) {}

  async start(port: number): Promise<void> {
    // Port allocation and child bind cannot be atomic without changing the
    // server API. Reject an occupied port and require our child's startup log
    // plus liveness; never accept an unrelated healthy server as our fixture.
    await new Promise<void>((resolve, reject) => {
      const reservation = createServer();
      reservation.once('error', reject);
      reservation.listen(port, '127.0.0.1', () => reservation.close(error => error ? reject(error) : resolve()));
    });
    const directory = mkdtempSync(join(tmpdir(), 'deve-sub-e2e-'));
    let log: number | undefined;
    let child: ChildProcess | undefined;
    try {
      const config = join(directory, 'config.json');
      writeFileSync(config, JSON.stringify({
        product_name: 'Deve Sub',
        server: { bind: `127.0.0.1:${port}`, serve_web: true, web_dist_dir: this.webDist },
        database: { path: join(directory, 'deve-sub.db') },
        security: {
          master_key_path: join(directory, 'master.key'), allow_master_key_generation: true,
          session_ttl_secs: 86400, cookie_secure: false, max_login_attempts: 100,
          lockout_duration_secs: 1, trust_proxy_headers: false,
        },
        geoip: { mmdb_path: null },
      }));
      mkdirSync(this.logDirectory, { recursive: true });
      log = openSync(join(this.logDirectory, `server-${port}.log`), 'wx', 0o600);
      const logFd = log;
      const migration = spawnSync(this.binary, ['migrate', '--db-path', join(directory, 'deve-sub.db')], {
        stdio: ['ignore', log, log], timeout: 30000,
      });
      if (migration.status !== 0) throw new Error(`migration failed for E2E port ${port}`);
      child = spawn(this.binary, ['serve', '--config', config], {
        stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, RUST_LOG: 'info' },
      });
      const stopped = new Promise<void>(resolve => child!.once('close', () => resolve()));
      this.handles.push({ proc: child, directory, stopped });
      let listening = false;
      let spawnError: Error | undefined;
      let startup = '';
      child.once('error', error => { spawnError = error; });
      const consume = (data: Buffer) => {
        // Keep bounded startup text only; all diagnostics stream to this run's log.
        if (!listening) {
          startup = (startup + data.toString()).slice(-8192);
          listening = startup.includes(`HTTP server listening on 127.0.0.1:${port}`);
        }
        writeFileSync(logFd, data);
      };
      child.stdout!.on('data', consume);
      child.stderr!.on('data', consume);
      child.once('close', () => closeSync(logFd));
      const deadline = Date.now() + 30000;
      while (Date.now() < deadline) {
        if (spawnError || child.exitCode !== null || child.signalCode !== null) {
          throw new Error(`E2E server ${port} exited before readiness: ${spawnError ?? child.exitCode}`);
        }
        if (listening) {
          const response = await fetch(`http://127.0.0.1:${port}/health/live`, {
            signal: AbortSignal.timeout(1000),
          }).catch(() => undefined);
          if (response?.ok) return;
        }
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      throw new Error(`E2E server ${port} did not become ready`);
    } catch (error) {
      if (!child) {
        if (log !== undefined) closeSync(log);
        rmSync(directory, { recursive: true, force: true });
      }
      throw error;
    }
  }

  async stop(): Promise<void> {
    const handles = this.handles.splice(0);
    await Promise.all(handles.map(async ({ proc, directory, stopped }) => {
      if (proc.exitCode === null && proc.signalCode === null) {
        proc.kill('SIGTERM');
        const deadline = setTimeout(() => proc.kill('SIGKILL'), 3000);
        await stopped;
        clearTimeout(deadline);
      } else {
        await stopped;
      }
      rmSync(directory, { recursive: true, force: true });
    }));
  }
}
