#!/usr/bin/env python3
"""Verify signed release assets with real CLI/server processes in bubblewrap.

The production trust root is unchanged. Assets must come from a signed Release
or preflight artifact. systemctl is simulated; this is not a systemd VM test.
Only temporary files are writable, and the PID namespace owns every process.
"""
import argparse
import functools
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request


def version(binary):
    return subprocess.check_output([str(binary), '--version'], text=True, timeout=5).split()[-1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def healthy(url, expected):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    for _ in range(100):
        try:
            with opener.open(url, timeout=1) as response:
                if json.load(response)['version'] == expected:
                    return
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.1)
    raise AssertionError(f'expected running version {expected} at {url}')


def systemctl():
    """Namespace-only shim: stop/start real binaries and inject one stale restart."""
    state = Path('/state')
    count_file = state / 'restarts'
    count = int(count_file.read_text()) + 1 if count_file.exists() else 1
    count_file.write_text(str(count))
    assert os.sys.argv[2:] == ['restart', 'deve-sub']
    # Leave the original process alive on the first update restart: HTTP 200
    # alone must not count as success when its running version is still old.
    if os.environ['UPDATE_CASE'] == 'rollback' and count == 2:
        assert digest(state / 'deve-sub') == os.environ['UPDATE_HASH']
        return
    pid_file = state / 'server.pid'
    if pid_file.exists():
        pid = int(pid_file.read_text())
        try:
            os.kill(pid, signal.SIGTERM)
            for _ in range(100):
                status = Path(f'/proc/{pid}/stat')
                if not status.exists() or status.read_text().split()[2] == 'Z':
                    break
                time.sleep(0.05)
            else:
                raise AssertionError('server did not terminate')
        except ProcessLookupError:
            pass
    with (state / 'server.log').open('ab') as log:
        process = subprocess.Popen(['/state/deve-sub', 'serve', '--config', '/state/config.json'],
                                   stdout=log, stderr=log)
    pid_file.write_text(str(process.pid))


def inside():
    state = Path('/state')
    old_version = version(state / 'deve-sub')
    target = os.environ['UPDATE_VERSION']
    assert old_version != target, 'different versions required to detect a stale running process'
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    url = f'http://127.0.0.1:{port}/health/live'
    (state / 'config.json').write_text(json.dumps({
        'server': {'bind': f'127.0.0.1:{port}', 'serve_web': False},
        'database': {'path': '/state/data.db'},
        'security': {'master_key_path': '/state/master.key', 'allow_master_key_generation': True},
    }))
    subprocess.run(['/state/deve-sub', 'migrate', '--db-path', '/state/data.db'],
                   check=True, stdout=subprocess.DEVNULL, timeout=20)
    original_hash = digest(state / 'deve-sub')
    subprocess.run(['systemctl', 'restart', 'deve-sub'], check=True, timeout=10)
    healthy(url, old_version)
    case = os.environ['UPDATE_CASE']
    command = ['/updater', 'update', '--binary-only', '--force', '--allow-downgrade',
               '--manifest-url', os.environ['UPDATE_MANIFEST'], '--binary-path', '/state/deve-sub',
               '--health-url', url, '--timeout', '3', '--config', '/state/config.json']
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=40)
    (state / 'update.log').write_text(result.stdout)
    print(result.stdout, end='')
    if case == 'tamper':
        assert result.returncode != 0 and 'signature verification failed' in result.stdout
        assert digest(state / 'deve-sub') == original_hash
        assert (state / 'restarts').read_text() == '1'
        healthy(url, old_version)
    elif case == 'rollback':
        assert result.returncode != 0 and 'Rolled back' in result.stdout
        assert digest(state / 'deve-sub.failed') == os.environ['UPDATE_HASH']
        assert 'signed manifest verified' in result.stdout
        assert digest(state / 'deve-sub') == original_hash
        assert (state / 'restarts').read_text() == '3'
        healthy(url, old_version)
    else:
        assert result.returncode == 0 and 'update successful' in result.stdout
        assert 'signed manifest verified' in result.stdout
        assert digest(state / 'deve-sub') == os.environ['UPDATE_HASH']
        assert not (state / 'deve-sub.bak').exists()
        healthy(url, target)
    print(f'PASS {case}: previous={old_version} target={target}')


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True, help='signed manifest, signature and native binary directory')
    parser.add_argument('--previous-binary', type=Path, required=True, help='real older release binary')
    parser.add_argument('--updater', type=Path, help='CLI under test; defaults to previous binary for actual upgrade path')
    args = parser.parse_args()
    assert shutil.which('bwrap'), 'bubblewrap is required'
    asset_name = {'x86_64': 'deve-sub-linux-amd64', 'aarch64': 'deve-sub-linux-arm64'}[os.uname().machine]
    assets = args.assets.resolve()
    previous = args.previous_binary.resolve()
    updater = (args.updater or previous).resolve()
    manifest = json.loads((assets / 'deve-sub-manifest.json').read_text())
    target = manifest['version']
    binary = assets / asset_name
    assert version(previous) != target, 'previous and target versions must differ'
    assert len((assets / 'deve-sub-manifest.json.sig').read_bytes()) == 64
    for case in ('success', 'rollback', 'tamper'):
        with tempfile.TemporaryDirectory(prefix='deve-sub-signed-update-') as temporary:
            root = Path(temporary)
            for directory in ('assets', 'state', 'systemd', 'tools'):
                (root / directory).mkdir()
            shutil.copy2(previous, root / 'state/deve-sub')
            for name in (asset_name, 'deve-sub-manifest.json', 'deve-sub-manifest.json.sig'):
                shutil.copyfile(assets / name, root / 'assets' / name)
            if case == 'tamper':
                signature = bytearray((root / 'assets/deve-sub-manifest.json.sig').read_bytes())
                signature[0] ^= 1
                (root / 'assets/deve-sub-manifest.json.sig').write_bytes(signature)
            server = http.server.ThreadingHTTPServer(('127.0.0.1', 0),
                functools.partial(QuietHandler, directory=str(root / 'assets')))
            base = f'http://127.0.0.1:{server.server_port}'
            release = {'tag_name': f'v{target}', 'assets': [
                {'name': name, 'browser_download_url': f'{base}/{name}'}
                for name in (asset_name, 'deve-sub-manifest.json', 'deve-sub-manifest.json.sig')]}
            (root / 'assets/release.json').write_text(json.dumps(release))
            (root / 'systemd/deve-sub.service').write_text('namespace-only service fixture\n')
            shim = root / 'tools/systemctl'
            shim.write_text('#!/bin/sh\nexec /usr/bin/python3 /smoke.py systemctl "$@"\n')
            shim.chmod(0o755)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            command = ['bwrap', '--unshare-user', '--uid', '0', '--gid', '0', '--unshare-pid',
                       '--die-with-parent', '--new-session', '--ro-bind', '/usr', '/usr',
                       '--ro-bind', '/lib', '/lib', '--ro-bind', '/lib64', '/lib64',
                       '--ro-bind', '/bin', '/bin', '--ro-bind', '/etc', '/etc', '--proc', '/proc',
                       '--dev', '/dev', '--tmpfs', '/tmp', '--bind', str(root / 'state'), '/state',
                       '--ro-bind', str(root / 'systemd'), '/etc/systemd/system',
                       '--ro-bind', str(root / 'tools'), '/tools', '--ro-bind', str(updater), '/updater',
                       '--ro-bind', str(Path(__file__).resolve()), '/smoke.py',
                       '--setenv', 'PATH', '/tools:/usr/bin:/bin', '--chdir', '/state',
                       '/usr/bin/python3', '/smoke.py', 'inside']
            env = dict(os.environ, UPDATE_CASE=case, UPDATE_VERSION=target, UPDATE_HASH=digest(binary),
                       UPDATE_MANIFEST=f'{base}/release.json', NO_PROXY='127.0.0.1', no_proxy='127.0.0.1')
            # Prevent user configuration overrides from entering the test.
            env = {key: value for key, value in env.items() if not key.startswith('DEVE_SUB_')}
            try:
                subprocess.run(command, env=env, check=True, timeout=70)
            finally:
                server.shutdown()
                server.server_close()
                thread.join(timeout=2)


if __name__ == '__main__':
    if os.sys.argv[1:2] == ['systemctl']:
        systemctl()
    elif os.sys.argv[1:2] == ['inside']:
        inside()
    else:
        main()
