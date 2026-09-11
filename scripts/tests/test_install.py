#!/usr/bin/env python3
"""Exercise the installer with real binaries in an owned mount/PID namespace.

Service account/systemd calls are simulated; this proves installer staging,
readiness and rollback, not a real systemd/VM deployment acceptance claim.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / 'target/debug/deve-sub'
WEB = ROOT / 'apps/web/dist'

# Filesystem writes are limited by bwrap to the test's temporary directories.
DISPATCH = r'''#!/usr/bin/python3
import hashlib, json, os, pathlib, shlex, shutil, signal, subprocess, sys, time
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
if name == 'curl':
    url = args[-1]
    if url.startswith('https://'):
        with open('/state/requests', 'a') as log:
            log.write(url + '\n')
        if url == 'https://api.github.com/repos/Develata/deve-sub/releases/latest':
            print(json.dumps({'tag_name': 'v0.1.0'}))
        elif url.startswith('https://github.com/Develata/deve-sub/releases/download/v0.1.0/'):
            asset = pathlib.Path(url).name
            shutil.copyfile('/fixture/assets/' + asset, args[args.index('-o') + 1])
        else:
            raise SystemExit('unexpected release URL')
    elif pathlib.Path('/state/restored.json').exists() and url.endswith(('/health/ready', '/health/live')):
        if os.environ.get('INSTALL_SMOKE_FAIL_ROLLBACK_READY') == '1':
            raise SystemExit(22)
        print('{"version":"0.0.9"}' if url.endswith('/health/live') else '{}')
    elif os.environ.get('INSTALL_SMOKE_MISSING_VERSION') == '1' and url.endswith('/health/live'):
        print('{}')
    else:
        os.execv('/usr/bin/curl', ['curl', *args])
elif name == 'sudo':
    assert args[:1] == ['-u']
    os.execv(args[2], args[2:])
elif name == 'systemctl':
    with open('/state/systemctl', 'a') as log:
        log.write(' '.join(args) + '\n')
    if args[0] == 'is-active':
        raise SystemExit(0 if pathlib.Path('/etc/systemd/system/deve-sub.service').exists() else 3)
    if args[0] == 'stop':
        pid_file = pathlib.Path('/state/server.pid')
        if pid_file.exists():
            pid = int(pid_file.read_text())
            try:
                os.kill(pid, signal.SIGTERM)
                for _ in range(100):
                    state = pathlib.Path(f'/proc/{pid}/stat')
                    if not state.exists() or state.read_text().split()[2] == 'Z':
                        break
                    time.sleep(0.01)
            except ProcessLookupError:
                pass
            pid_file.unlink()
    if args[0] == 'enable' and os.environ.get('INSTALL_SMOKE_FAIL_ENABLE') == '1':
        raise SystemExit(1)
    if args[0] == 'start':
        pathlib.Path('/state/restored.json').write_text(json.dumps({
            'binary': hashlib.sha256(pathlib.Path('/usr/local/bin/deve-sub').read_bytes()).hexdigest(),
            'web': pathlib.Path('/usr/local/share/deve-sub/web/index.html').read_text(),
            'unit': pathlib.Path('/etc/systemd/system/deve-sub.service').read_text(),
        }))
    if args[0] == 'restart':
        if os.environ.get('INSTALL_SMOKE_FAIL_RESTART') == '1':
            raise SystemExit(1)
        unit = pathlib.Path('/etc/systemd/system/deve-sub.service').read_text()
        command = next(line.removeprefix('ExecStart=') for line in unit.splitlines() if line.startswith('ExecStart='))
        with open('/state/server.log', 'w') as log:
            process = subprocess.Popen(shlex.split(command), cwd='/var/lib/deve-sub', stdout=log, stderr=log)
            pathlib.Path('/state/server.pid').write_text(str(process.pid))
# Account management is simulated; bwrap maps this namespace's root to the
# invoking user. No call reaches the host's systemctl or user database.
elif name not in ('groupadd', 'useradd', 'chown'):
    raise SystemExit('unexpected shim')
'''


class InstallerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which('bwrap'):
            raise unittest.SkipTest('bubblewrap is required for isolated installer smoke')
        if not BINARY.is_file() or not (WEB / 'index.html').is_file():
            raise unittest.SkipTest('build the CLI and Web dist before running installer smoke')
        subprocess.run(['bwrap', '--ro-bind', '/', '/', '--unshare-user', '--uid', '0', '--gid', '0', '/usr/bin/true'], check=True)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='deve-sub-installer-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for directory in ('assets', 'tools', 'state', 'local/bin', 'local/share/deve-sub/web', 'data', 'systemd'):
            (self.root / directory).mkdir(parents=True, exist_ok=True)
        dispatch = self.root / 'tools/dispatch.py'
        dispatch.write_text(DISPATCH)
        dispatch.chmod(0o755)
        for name in ('curl', 'sudo', 'systemctl', 'groupadd', 'useradd', 'chown'):
            (dispatch.parent / name).symlink_to('dispatch.py')
        self.old_binary = b'#!/bin/sh\necho deve-sub 0.0.9\n'
        (self.root / 'local/bin/deve-sub').write_bytes(self.old_binary)
        (self.root / 'local/bin/deve-sub').chmod(0o755)
        (self.root / 'local/share/deve-sub/web/index.html').write_text('previous frontend')
        (self.root / 'systemd/deve-sub.service').write_text('previous service unit')
        shutil.copyfile(BINARY, self.root / 'assets/deve-sub-linux-amd64')
        with tarfile.open(self.root / 'assets/deve-sub-web.tar.gz', 'w:gz') as archive:
            for path in sorted(WEB.rglob('*')):
                if path.is_file() and path.name != '.ci-artifact.json':
                    archive.add(path, arcname='./' + path.relative_to(WEB).as_posix(), recursive=False)
        self.checksums()

    def checksums(self):
        assets = self.root / 'assets'
        lines = []
        for name in ('deve-sub-linux-amd64', 'deve-sub-web.tar.gz'):
            lines.append(hashlib.sha256((assets / name).read_bytes()).hexdigest() + '  ' + name)
        (assets / 'checksums.txt').write_text('\n'.join(lines) + '\n')

    def run_installer(self, fail_restart=False, latest=False, missing_version=False, fail_enable=False, fail_rollback_ready=False):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        env = dict(os.environ, DEVE_SUB_BIND=f'127.0.0.1:{port}', DEVE_SUB_VERSION='latest' if latest else 'v0.1.0',
                   DEVE_SUB_DATA_DIR='/var/lib/deve-sub', INSTALL_SMOKE_FAIL_RESTART='1' if fail_restart else '0',
                   INSTALL_SMOKE_MISSING_VERSION='1' if missing_version else '0',
                   INSTALL_SMOKE_FAIL_ENABLE='1' if fail_enable else '0',
                   INSTALL_SMOKE_FAIL_ROLLBACK_READY='1' if fail_rollback_ready else '0',
                   NO_PROXY='localhost,127.0.0.1,::1', no_proxy='localhost,127.0.0.1,::1')
        command = ['bwrap', '--unshare-user', '--uid', '0', '--gid', '0',
                   '--unshare-pid', '--die-with-parent', '--new-session', '--proc', '/proc', '--dev', '/dev',
                   '--ro-bind', '/usr', '/usr', '--ro-bind', '/lib', '/lib',
                   '--ro-bind', '/lib64', '/lib64', '--ro-bind', '/bin', '/bin',
                   '--ro-bind', '/sbin', '/sbin', '--ro-bind', '/etc', '/etc',
                   '--ro-bind', str(ROOT), '/repo', '--ro-bind', str(self.root), '/fixture',
                   '--tmpfs', '/tmp', '--tmpfs', '/var/tmp', '--tmpfs', '/run', '--tmpfs', '/var/lib',
                   '--bind', str(self.root / 'local'), '/usr/local',
                   '--bind', str(self.root / 'data'), '/var/lib/deve-sub',
                   '--bind', str(self.root / 'state'), '/state',
                   '--bind', str(self.root / 'systemd'), '/etc/systemd/system',
                   '--setenv', 'PATH', '/fixture/tools:/usr/bin:/bin',
                   '--chdir', '/', '/bin/sh', '-c', 'umask 077; exec /bin/sh /repo/scripts/install.sh']
        result = subprocess.run(command, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=90)
        (self.root / 'state/installer.log').write_text(result.stdout)
        return result

    def assert_untouched(self):
        self.assertEqual((self.root / 'local/bin/deve-sub').read_bytes(), self.old_binary)
        self.assertEqual((self.root / 'local/share/deve-sub/web/index.html').read_text(), 'previous frontend')

    def test_install_both_assets_and_reach_readiness_from_one_resolved_tag(self):
        result = self.run_installer(latest=True)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn('installed successfully', result.stdout)
        self.assertEqual((self.root / 'local/bin/deve-sub').read_bytes(), BINARY.read_bytes())
        self.assertEqual((self.root / 'local/share/deve-sub/web/index.html').read_bytes(), (WEB / 'index.html').read_bytes())
        unit = (self.root / 'systemd/deve-sub.service').read_text()
        self.assertIn('--web-dist-dir /usr/local/share/deve-sub/web', unit)
        requests = (self.root / 'state/requests').read_text().splitlines()
        self.assertEqual(len(requests), 4)
        self.assertTrue(all('/download/v0.1.0/' in url for url in requests[1:]))

    def test_restart_failure_restores_previous_binary_and_frontend(self):
        result = self.run_installer(fail_restart=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('rolling back to previous binary', result.stdout)
        self.assert_untouched()

    def test_missing_running_version_cannot_report_success(self):
        result = self.run_installer(missing_version=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('version mismatch', result.stdout)
        self.assertNotIn('installed successfully', result.stdout)
        self.assert_untouched()

    def test_late_failure_stops_new_process_and_restores_old_service_state(self):
        result = self.run_installer(fail_enable=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assert_untouched()
        actions = (self.root / 'state/systemctl').read_text().splitlines()
        self.assertEqual(actions[-4:], ['stop deve-sub', 'daemon-reload', 'start deve-sub', 'is-active --quiet deve-sub'])
        restored = json.loads((self.root / 'state/restored.json').read_text())
        self.assertEqual(restored, {'binary': hashlib.sha256(self.old_binary).hexdigest(),
                                    'web': 'previous frontend', 'unit': 'previous service unit'})
        self.assertFalse((self.root / 'state/server.pid').exists())

    def test_started_but_unhealthy_previous_service_retains_backups(self):
        result = self.run_installer(fail_restart=True, fail_rollback_ready=True)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assert_untouched()
        self.assertIn('rollback incomplete; recovery backups retained', result.stdout)

    def test_first_install_does_not_require_a_previous_service(self):
        (self.root / 'systemd/deve-sub.service').unlink()
        (self.root / 'local/bin/deve-sub').unlink()
        shutil.rmtree(self.root / 'local/share/deve-sub')
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn('installed successfully', result.stdout)
        self.assertEqual((self.root / 'local/share/deve-sub').stat().st_mode & 0o777, 0o755)

    def test_corrupted_frontend_fails_before_stopping_service(self):
        with (self.root / 'assets/deve-sub-web.tar.gz').open('ab') as stream:
            stream.write(b'corrupted')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('checksum mismatch', result.stdout)
        self.assert_untouched()
        self.assertFalse((self.root / 'state/systemctl').exists())

    def test_symlink_archive_fails_before_install(self):
        with tarfile.open(self.root / 'assets/deve-sub-web.tar.gz', 'w:gz') as archive:
            link = tarfile.TarInfo('assets/link')
            link.type = tarfile.SYMTYPE
            link.linkname = '/etc/passwd'
            archive.addfile(link)
        self.checksums()
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('link or special file', result.stdout)
        self.assert_untouched()

    def test_missing_web_asset_fails_before_install(self):
        with tarfile.open(self.root / 'assets/deve-sub-web.tar.gz', 'w:gz') as archive:
            entry = tarfile.TarInfo('index.html')
            entry.size = 5
            archive.addfile(entry, io.BytesIO(b'index'))
        self.checksums()
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn('missing wasm assets', result.stdout)
        self.assert_untouched()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=BINARY)
    parser.add_argument('--web-dir', type=Path, default=WEB)
    args = parser.parse_args()
    BINARY, WEB = args.binary.resolve(), args.web_dir.resolve()
    unittest.main(argv=['test_install.py'])
