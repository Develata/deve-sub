import { spawn, spawnSync, type ChildProcess } from 'child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, createWriteStream } from 'fs';
import { createServer } from 'net';
import { randomInt } from 'crypto';
import { join } from 'path';
import { tmpdir } from 'os';

/** Blank, isolated instance. Restart retains this instance's DB and master key. */
export class NormalServer {
  readonly directory = mkdtempSync(join(tmpdir(), 'deve-sub-normal-'));
  private child?: ChildProcess;
  private stopped?: Promise<void>;
  private port = 0;
  private startNumber = 0;
  url = '';

  constructor(private binary: string, private webDist: string, private logs: string) {}

  async start() {
    this.port = await new Promise<number>((resolve, reject) => {
      const socket = createServer();
      let attempts = 0;
      socket.on('error', (error: NodeJS.ErrnoException) => {
        if (error.code === 'EADDRINUSE' && ++attempts < 20) socket.listen(randomInt(20000, 30000), '127.0.0.1');
        else reject(error);
      });
      socket.once('listening', () => {
        const port = (socket.address() as { port: number }).port;
        socket.close(error => error ? reject(error) : resolve(port));
      });
      socket.listen(randomInt(20000, 30000), '127.0.0.1');
    });
    this.url = `http://127.0.0.1:${this.port}`;
    writeFileSync(join(this.directory, 'config.json'), JSON.stringify({
      product_name: 'Deve Sub',
      server: { bind: `127.0.0.1:${this.port}`, serve_web: true, web_dist_dir: this.webDist },
      database: { path: join(this.directory, 'deve-sub.db') },
      security: { master_key_path: join(this.directory, 'master.key'), allow_master_key_generation: true,
        session_ttl_secs: 86400, cookie_secure: false, max_login_attempts: 100,
        lockout_duration_secs: 1, trust_proxy_headers: false },
      geoip: { mmdb_path: null },
    }));
    const migration = spawnSync(this.binary, ['migrate', '--db-path', join(this.directory, 'deve-sub.db')], { timeout: 30_000 });
    if (migration.status !== 0) throw new Error(`migration failed: ${migration.stderr}`);
    mkdirSync(this.logs, { recursive: true });
    writeFileSync(join(this.logs, 'migration.log'), migration.stdout.toString() + migration.stderr.toString());
    await this.launch();
  }

  private async launch() {
    const output = createWriteStream(join(this.logs, `server-${++this.startNumber}.log`), { mode: 0o600 });
    const child = spawn(this.binary, ['serve', '--config', join(this.directory, 'config.json')], {
      stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, RUST_LOG: 'info' },
    });
    this.child = child;
    this.stopped = new Promise(resolve => child.once('close', () => { output.end(); resolve(); }));
    let startup = '', listening = false, spawnError: Error | undefined;
    child.once('error', error => { spawnError = error; });
    const consume = (data: Buffer) => {
      output.write(data);
      if (!listening) {
        startup = (startup + data.toString()).slice(-8192);
        listening = startup.includes(`HTTP server listening on 127.0.0.1:${this.port}`);
      }
    };
    child.stdout!.on('data', consume);
    child.stderr!.on('data', consume);
    for (const deadline = Date.now() + 30_000; Date.now() < deadline;) {
      if (spawnError || child.exitCode !== null || child.signalCode !== null) throw new Error(`server startup failed: ${spawnError ?? child.exitCode}`);
      if (listening && (await fetch(`${this.url}/health/live`, { signal: AbortSignal.timeout(1000) }).catch(() => undefined))?.ok) return;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw new Error('server readiness timed out');
  }

  private async stopProcess() {
    if (!this.child || !this.stopped) return;
    if (this.child.exitCode === null && this.child.signalCode === null) {
      this.child.kill('SIGTERM');
      const deadline = setTimeout(() => this.child?.kill('SIGKILL'), 3000);
      await this.stopped;
      clearTimeout(deadline);
    } else await this.stopped;
  }

  async restart() { await this.stopProcess(); await this.launch(); }
  async close() { await this.stopProcess(); rmSync(this.directory, { recursive: true, force: true }); }
}
