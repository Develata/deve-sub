"""Run/verify source-bound local browser evidence; no acquisition or publishing."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys

sys.dont_write_bytecode = True
from common import ROOT, digest, identity, write_json, install_interrupt_handlers
import artifact
import suite
from normal import execute


def fingerprint(source):
    return {key: source[key] for key in ['commit', 'dirty', 'source_digest', 'toolchain', 'lockfile_digest']}


def inventory(directory):
    if not directory.is_dir() or directory.is_symlink():
        raise ValueError('evidence input must be an existing regular directory')
    result = {}
    for path in sorted(directory.rglob('*')):
        if path.is_symlink():
            raise ValueError('evidence symlinks forbidden')
        if not path.is_file() and not path.is_dir():
            raise ValueError('evidence contains nonregular files')
        if path.is_file() and path != directory / 'manifest.json':
            result[str(path.relative_to(directory))] = {'size': path.stat().st_size, 'sha256': digest(path)}
    return result


def validate_browser(ready):
    plan = json.loads((ROOT / 'tests/acceptance/local-browser.json').read_text())
    required = plan['browser']
    if (ready.get('status') != 'READY' or ready.get('launch_verified') is not True
            or ready.get('browser_kind') != 'chromium'
            or ready.get('browser_download_attempted') is not False
            or ready.get('playwright_version') != required['playwright_version']
            or ready.get('browser_version') != required['version']
            or ready.get('revision') != required['revision']
            or not isinstance(ready.get('executable_path'), str) or not ready['executable_path']
            or any(c in ready['executable_path'] for c in '\r\n\0')
            or not re.fullmatch(r'[0-9a-f]{64}', ready.get('executable_sha256', ''))):
        raise ValueError('real full locked Chromium startup evidence missing')


def verify(directory, expected_sha, source, release=False):
    manifest = directory / 'manifest.json'
    if manifest.is_symlink() or not manifest.is_file() or not re.fullmatch(r'[0-9a-f]{64}', expected_sha or ''):
        raise ValueError('missing local evidence or independent approved digest')
    if digest(manifest) != expected_sha:
        raise ValueError('local evidence differs from independently approved digest')
    envelope = json.loads(manifest.read_text())
    if envelope.get('schema_version') != 1 or envelope.get('files') != inventory(directory):
        raise ValueError('local evidence file inventory/digests differ')
    report = json.loads((directory / 'local.json').read_text())
    if (report.get('profile') != 'local-browser' or report.get('status') != 'PASS'
            or type(report.get('preparation_exit')) is not int or report['preparation_exit'] != 0
            or not (directory / 'preparation.log').is_file()):
        raise ValueError('full local browser execution missing/failed')
    if fingerprint(report['source']) != fingerprint(source) or report.get('source_after') != report['source']:
        raise ValueError('local evidence source/commit/lock/toolchain differs')
    if release and (source['dirty'] or report['source']['dirty']):
        raise ValueError('release requires final clean committed source')
    ready = json.loads((directory / 'browser.json').read_text())
    validate_browser(ready)
    producers = {}
    producer_run = None
    for kind in ['binary', 'wasm']:
        path = directory / f'{kind}-producer.json'
        producer = json.loads(path.read_text())
        run = (producer.get('run_id'), producer.get('run_attempt'))
        if not all(isinstance(v, str) and v for v in run) or (producer_run is not None and producer_run != run):
            raise ValueError('producer execution contexts missing/different')
        producer_run = run
        expected = artifact.context(kind, source)
        expected.update(run_id=run[0], run_attempt=run[1])
        artifact.validate_manifest(producer, expected)
        if (digest(path) != report['producer_digests'][kind]
                or fingerprint(producer) != fingerprint(source)
                or producer.get('kind') != kind or producer.get('profile') != 'release'
                or producer.get('target') != ('x86_64-unknown-linux-gnu' if kind == 'binary' else 'wasm32-unknown-unknown')
                or not producer.get('files')):
            raise ValueError('producer/source provenance differs')
        producers[kind] = producer
    results = suite.validate_results(directory / 'suites', 'local-browser')
    if results['source'] != report['source']:
        raise ValueError('raw suites belong to another source')
    runtime = results.get('runtime', {})
    if (runtime.get('binary_sha256') != producers['binary']['files']['deve-sub']['sha256']
            or runtime.get('web_files') != producers['wasm']['files']
            or runtime.get('browser_path') != ready.get('executable_path')):
        raise ValueError('executed runtime differs from prepared browser or producer files')
    return {'profile': 'full-local-browser-evidence', 'status': 'PASS',
            'source': report['source'], 'primary_browser_cases': 151,
            'platform': report['platform'], 'full_native_device_acceptance': 'NOT_ASSERTED'}


def run(args):
    directory = args.root.resolve()
    if directory == ROOT or ROOT in directory.parents:
        raise ValueError('local evidence output must be outside the source tree')
    directory.mkdir(parents=True, exist_ok=False)
    source = identity()
    report = {'schema_version': 1, 'profile': 'local-browser', 'source': source,
              'status': 'FAIL', 'errors': [], 'platform': platform.platform(),
              'started_at': datetime.now(timezone.utc).isoformat(), 'producer_digests': {}}
    try:
        if sys.platform != 'linux' or platform.machine() != 'x86_64':
            raise ValueError('existing producer contract is Linux x86_64; other native platforms NOT_RUN')
        for kind, root, expected in [('binary', args.binary_root, args.binary_manifest_sha256),
                                     ('wasm', args.web_root, args.web_manifest_sha256)]:
            if root is None:
                raise ValueError('both source-bound producer roots are required')
            artifact.verify(root, kind, source, expected)
            path = root / artifact.MANIFEST
            report['producer_digests'][kind] = digest(path)
            (directory / f'{kind}-producer.json').write_bytes(path.read_bytes())
        script = "const fs=require('node:fs');const m=require('./browser-preparation.cjs');m.prepareBrowser(m.loadRuntime()).then(r=>fs.writeFileSync(process.argv[1],JSON.stringify(r))).catch(e=>{console.error(e.message);process.exitCode=1})"
        command = ['node', '-e', script, str(directory / 'browser.json')]
        report['preparation_command'] = command
        code = execute(command, suite.E2E, os.environ.copy(), directory / 'preparation.log', timeout=60)
        report['preparation_exit'] = code
        if code != 0:
            raise ValueError('real locked browser preparation failed; dependent cases NOT_RUN')
        ready = json.loads((directory / 'browser.json').read_text())
        validate_browser(ready)
        env = {'DEVE_SUB_BROWSER': ready['executable_path'],
               'DEVE_SUB_BINARY': str(args.binary_root.resolve() / 'deve-sub'),
               'DEVE_SUB_WEB_DIST': str(args.web_root.resolve())}
        results = suite.run('local-browser', directory / 'suites', env)
        if results['status'] != 'PASS':
            raise ValueError('local browser suites did not all pass')
        for kind, root, expected in [('binary', args.binary_root, args.binary_manifest_sha256),
                                     ('wasm', args.web_root, args.web_manifest_sha256)]:
            artifact.verify(root, kind, source, expected)
        report['source_after'] = identity()
        if report['source_after'] != source:
            raise ValueError('source mutated during local verification')
        report['status'] = 'PASS'
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, InterruptedError) as error:
        report['errors'].append(f'{type(error).__name__}: {error}')
    finally:
        report['finished_at'] = datetime.now(timezone.utc).isoformat()
        write_json(directory / 'local.json', report)
        write_json(directory / 'manifest.json', {'schema_version': 1, 'files': inventory(directory)})
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['run', 'verify'])
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--manifest-sha256')
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--binary-root', type=Path)
    parser.add_argument('--web-root', type=Path)
    parser.add_argument('--binary-manifest-sha256')
    parser.add_argument('--web-manifest-sha256')
    args = parser.parse_args()
    try:
        result = run(args) if args.operation == 'run' else verify(args.root, args.manifest_sha256, identity(), args.release)
        print(json.dumps(result))
        return 0 if result['status'] == 'PASS' else 1
    except (ValueError, KeyError, TypeError, OSError) as error:
        print(f'full local acceptance rejected: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    install_interrupt_handlers()
    raise SystemExit(main())
