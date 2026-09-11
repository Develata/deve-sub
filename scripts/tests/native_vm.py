#!/usr/bin/env python3
"""Destructive acceptance INSIDE a disposable VM reached over SSH.

The supplied guest must be dedicated to this test, with passwordless sudo,
Python 3, curl and systemd as PID 1. No host service/account command is issued.
Release downloads use guest-local fixtures; systemctl is always the real binary.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, required=True)
    parser.add_argument('--key', type=Path, required=True)
    parser.add_argument('--known-hosts', type=Path, required=True)
    parser.add_argument('--binary-a', type=Path, required=True)
    parser.add_argument('--binary-b', type=Path, required=True)
    parser.add_argument('--web', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    ssh_options = ['-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=yes',
                   '-o', f'UserKnownHostsFile={args.known_hosts.resolve()}',
                   '-o', 'ConnectTimeout=10', '-i', str(args.key.resolve())]
    ssh = ['ssh', *ssh_options, '-p', str(args.port), 'vmtester@127.0.0.1']
    report = {'status': 'FAIL', 'cases': {}, 'transport': 'fixture curl download shim; real systemd, accounts, filesystem, HTTP readiness and reboot'}

    def guest(command, timeout=180, check=True):
        result = subprocess.run([*ssh, "bash -euo pipefail -c " + shlex.quote(command)], capture_output=True, text=True, timeout=timeout)
        if check and result.returncode:
            raise RuntimeError(f'guest command failed ({result.returncode}): {result.stdout[-2000:]} {result.stderr[-2000:]}')
        return result

    def probe(version, marker):
        guest("sudo systemctl is-active deve-sub && sudo systemctl is-enabled deve-sub && "
              "curl --fail --max-time 5 -s http://127.0.0.1:8080/health/ready")
        live = json.loads(guest("curl --fail --max-time 5 -s http://127.0.0.1:8080/health/live").stdout)
        frontend = guest("curl --fail --max-time 5 -s http://127.0.0.1:8080/acceptance-version.txt").stdout
        assert live["version"] == version, live
        assert frontend.strip() == marker, frontend
        return live

    def install(version, fixture, expect=True):
        command = ('sudo env PATH=/opt/fixture/tools:/usr/sbin:/usr/bin:/sbin:/bin '
                   f'DEVE_SUB_VERSION=v{version} DEVE_SUB_BIND=127.0.0.1:8080 '
                   f'FIXTURE={fixture} sh /opt/fixture/install.sh')
        result = guest(command, timeout=240, check=False)
        report.setdefault('install_logs', []).append(result.stdout[-5000:] + result.stderr[-2000:])
        assert (result.returncode == 0) == expect, result.stdout + result.stderr
        return result

    try:
        assert guest('cat /proc/1/comm').stdout.strip() == 'systemd'
        report['guest'] = guest('cat /etc/os-release; systemctl --version | head -1; uname -m').stdout
        assert guest('test ! -e /usr/local/bin/deve-sub && test ! -e /var/lib/deve-sub', check=False).returncode == 0, 'guest must be fresh'
        with tempfile.TemporaryDirectory(prefix='deve-sub-vm-input-') as directory:
            root = Path(directory)
            versions = []
            for name, binary in [('a', args.binary_a), ('b', args.binary_b)]:
                version = subprocess.check_output([str(binary.resolve()), '--version'], text=True, timeout=10).split()[-1]
                versions.append(version)
                folder = root / name
                folder.mkdir()
                (folder / 'deve-sub-linux-amd64').write_bytes(binary.read_bytes())
                (folder / 'deve-sub-linux-amd64').chmod(0o755)
                with tarfile.open(folder / 'deve-sub-web.tar.gz', 'w:gz') as archive:
                    for path in sorted(args.web.rglob('*')):
                        if path.is_file() and path.name != '.ci-artifact.json':
                            archive.add(path, arcname='./' + path.relative_to(args.web).as_posix(), recursive=False)
                    marker = folder / 'acceptance-version.txt'
                    marker.write_text(name + '\n')
                    archive.add(marker, arcname='./acceptance-version.txt')
                (folder / 'checksums.txt').write_text(''.join(
                    hashlib.sha256((folder / asset).read_bytes()).hexdigest() + '  ' + asset + '\n'
                    for asset in ('deve-sub-linux-amd64', 'deve-sub-web.tar.gz')))
            (root / 'install.sh').write_bytes((ROOT / 'scripts/install.sh').read_bytes())
            (root / 'tools').mkdir()
            # Only the release transport is replaced. All readiness requests and
            # system/account commands use actual guest tools and processes.
            (root / 'tools/curl').write_text('''#!/usr/bin/python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
if args[-1].startswith('https://github.com/Develata/deve-sub/releases/download/'):
    asset = args[-1].rsplit('/', 1)[-1]
    shutil.copyfile('/opt/fixture/' + os.environ['FIXTURE'] + '/' + asset, args[args.index('-o') + 1])
else:
    os.execv('/usr/bin/curl', ['curl', *args])
''')
            (root / 'tools/curl').chmod(0o755)
            bundle = root / 'fixture.tar'
            with tarfile.open(bundle, 'w') as archive:
                for name in ('a', 'b', 'tools', 'install.sh'):
                    archive.add(root / name, arcname=name)
            subprocess.run(['scp', '-q', *ssh_options, '-P', str(args.port), str(bundle),
                            'vmtester@127.0.0.1:/tmp/fixture.tar'], check=True, timeout=240)
        guest('sudo mkdir -p /opt/fixture; sudo tar -xf /tmp/fixture.tar -C /opt/fixture; rm /tmp/fixture.tar')
        va, vb = versions
        assert va != vb, 'cross-version evidence requires two different real binary versions'
        report['versions'] = {'a': va, 'b': vb}
        report['binary_sha256'] = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [('a', args.binary_a), ('b', args.binary_b)]}
        report['installer_sha256'] = hashlib.sha256((ROOT / 'scripts/install.sh').read_bytes()).hexdigest()
        install(va, 'a')
        probe(va, 'a')
        permissions = guest("sudo stat -c '%a %U:%G' /var/lib/deve-sub /var/lib/deve-sub/master.key; "
                            "sudo systemctl show deve-sub -p User -p Group -p UMask -p KillMode; "
                            "sudo getent passwd deve-sub").stdout
        assert '700 deve-sub:deve-sub' in permissions and '600 deve-sub:deve-sub' in permissions
        report['permissions'] = permissions
        report['cases']['fresh_install'] = 'PASS'
        before = guest('cat /proc/sys/kernel/random/boot_id').stdout.strip()
        guest('sudo systemctl reboot', timeout=20, check=False)
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            time.sleep(2)
            result = guest('cat /proc/sys/kernel/random/boot_id', timeout=15, check=False)
            if result.returncode == 0 and result.stdout.strip() != before:
                try:
                    probe(va, 'a')
                    break
                except (RuntimeError, AssertionError):
                    pass
        else:
            raise AssertionError('reboot did not recover readiness within 180s')
        report['cases']['reboot'] = 'PASS'
        install(vb, 'b')
        probe(vb, 'b')
        report['cases']['upgrade' if va != vb else 'same_version_reinstall'] = 'PASS'
        # Reject only candidate A using its executable digest; rollback B is
        # accepted by the same real systemd ExecStartPre path.
        digest_b = hashlib.sha256(args.binary_b.read_bytes()).hexdigest()
        script = '#!/bin/sh\n[ "$(sha256sum /usr/local/bin/deve-sub | cut -d \' \' -f 1)" = "' + digest_b + '" ]\n'
        guest("sudo mkdir -p /etc/systemd/system/deve-sub.service.d; "
              f"printf %s {shlex.quote(script)} | sudo tee /opt/fixture/accept-b.sh >/dev/null; "
              "sudo chmod 755 /opt/fixture/accept-b.sh; "
              "printf '[Service]\\nExecStartPre=/opt/fixture/accept-b.sh\\n' | sudo tee /etc/systemd/system/deve-sub.service.d/fail.conf >/dev/null; "
              "sudo systemctl daemon-reload")
        install(va, 'a', expect=False)
        deadline = time.monotonic() + 45
        while True:
            try:
                probe(vb, 'b')
                break
            except RuntimeError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(1)
        report['cases']['restart_failure_automatic_rollback'] = 'PASS'
        # A real systemd override blocks every attempted new start. Rollback
        # must retain artifacts because it cannot truthfully restart the old service.
        guest("sudo mkdir -p /etc/systemd/system/deve-sub.service.d; "
              "printf '[Service]\\nExecStartPre=/bin/false\\n' | sudo tee /etc/systemd/system/deve-sub.service.d/fail.conf >/dev/null; "
              "sudo systemctl daemon-reload")
        install(va, 'a', expect=False)
        assert guest('sudo test -s /var/tmp/deve-sub-install.pending', check=False).returncode == 0
        report['cases']['rollback_failure_retains_recovery'] = 'PASS'
        # Remove only this deliberate override, recover with retained previous
        # files, then explicitly clear the pending checkpoint after health passes.
        guest('sudo rm /etc/systemd/system/deve-sub.service.d/fail.conf; sudo systemctl daemon-reload; sudo systemctl reset-failed deve-sub; sudo systemctl start deve-sub')
        deadline = time.monotonic() + 45
        while True:
            try:
                probe(vb, 'b')
                break
            except RuntimeError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(1)
        report['cases']['manual_recovery_of_previous_service'] = 'PASS'
        guest('sudo rm /var/tmp/deve-sub-install.pending')
        # Kill a real installer after its durable checkpoint appears. The next
        # invocation must refuse before touching the potentially partial pair.
        interruption = r'''import hashlib, json, os, pathlib, signal, subprocess, time
pending = pathlib.Path('/var/tmp/deve-sub-install.pending')
env = dict(os.environ, PATH='/opt/fixture/tools:/usr/sbin:/usr/bin:/sbin:/bin',
           DEVE_SUB_VERSION='vVERSION_A', DEVE_SUB_BIND='127.0.0.1:8080', FIXTURE='a')
with open('/opt/fixture/interrupted.log', 'w') as log:
    process = subprocess.Popen(['sh', '/opt/fixture/install.sh'], env=env,
                               stdout=log, stderr=log, start_new_session=True)
    try:
        deadline = time.monotonic() + 90
        while not pending.exists():
            assert process.poll() is None, 'installer exited before checkpoint'
            assert time.monotonic() < deadline, 'checkpoint timeout'
            time.sleep(0.001)
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=10)
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=10)
assert pending.exists()
def state():
    return {name: hashlib.sha256(pathlib.Path(name).read_bytes()).hexdigest()
            if pathlib.Path(name).is_file() else None for name in
            ['/usr/local/bin/deve-sub', '/usr/local/share/deve-sub/web/index.html',
             '/etc/systemd/system/deve-sub.service']}
before = state()
retry = subprocess.run(['sh', '/opt/fixture/install.sh'], env=env,
                       capture_output=True, text=True, timeout=30)
assert retry.returncode != 0 and 'interrupted installation' in retry.stderr
assert state() == before
print(json.dumps({'checkpoint': pending.read_text().strip(), 'next_run_refused': True}))
'''.replace('VERSION_A', va)
        result = guest('sudo python3 -c ' + shlex.quote(interruption), timeout=120)
        checkpoint = json.loads(result.stdout)['checkpoint']
        assert checkpoint.startswith('/var/tmp/deve-sub-install.')
        report['cases']['sigkill_then_clear_refusal'] = 'PASS'
        backup = shlex.quote(checkpoint)
        guest(f'sudo systemctl stop deve-sub; sudo cp -a {backup}/deve-sub.bak /usr/local/bin/deve-sub; '
              'sudo rm -rf /usr/local/share/deve-sub/web; '
              f'sudo cp -a {backup}/web.bak /usr/local/share/deve-sub/web; '
              f'sudo cp -a {backup}/service.bak /etc/systemd/system/deve-sub.service; '
              'sudo systemctl daemon-reload; sudo systemctl start deve-sub')
        deadline = time.monotonic() + 45
        while True:
            try:
                probe(vb, 'b')
                break
            except RuntimeError:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(1)
        report['cases']['interruption_manual_recovery'] = 'PASS'
        guest('sudo rm /var/tmp/deve-sub-install.pending')
        report['status'] = 'PASS'
    except BaseException as error:
        report['error'] = str(error)
        raise
    finally:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({key: value for key, value in report.items() if key != 'install_logs'}, indent=2))


if __name__ == '__main__':
    main()
