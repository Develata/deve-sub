import { test, expect } from '@playwright/test';
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from 'fs';
import { createServer } from 'net';
import { tmpdir } from 'os';
import { join } from 'path';
import { Servers } from '../server-lifecycle';

let root: string;
let previousTmp: string | undefined;
test.beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'deve-sub-lifecycle-test-'));
  previousTmp = process.env.TMPDIR;
  process.env.TMPDIR = root;
});
test.afterEach(() => {
  if (previousTmp === undefined) delete process.env.TMPDIR;
  else process.env.TMPDIR = previousTmp;
  rmSync(root, { recursive: true, force: true });
});

async function availablePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = (server.address() as { port: number }).port;
  await new Promise<void>(resolve => server.close(() => resolve()));
  return port;
}

test('log setup failure removes state before a child exists', async () => {
  const logPath = join(root, 'not-a-directory');
  writeFileSync(logPath, 'fixture');
  const servers = new Servers('/unused-binary', '/unused-dist', logPath);
  await expect(servers.start(await availablePort())).rejects.toThrow();
  await servers.stop();
  expect(readdirSync(root).filter(name => name.startsWith('deve-sub-e2e-'))).toEqual([]);
});

test('migration spawn failure removes state and closes its log', async () => {
  const servers = new Servers(join(root, 'missing-binary'), '/unused-dist', join(root, 'logs'));
  await expect(servers.start(await availablePort())).rejects.toThrow('migration failed');
  await servers.stop();
  expect(readdirSync(root).filter(name => name.startsWith('deve-sub-e2e-'))).toEqual([]);
});

test('occupied port is rejected without adopting or stopping its owner', async () => {
  const listener = createServer();
  await new Promise<void>(resolve => listener.listen(0, '127.0.0.1', resolve));
  const port = (listener.address() as { port: number }).port;
  const servers = new Servers('/unused-binary', '/unused-dist', join(root, 'logs'));
  try {
    await expect(servers.start(port)).rejects.toThrow();
    await servers.stop();
    expect(listener.listening).toBe(true);
    expect(readdirSync(root)).toEqual([]);
  } finally {
    await new Promise<void>(resolve => listener.close(() => resolve()));
  }
});
