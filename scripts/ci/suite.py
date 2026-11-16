"""Exact registered Playwright-runner groups; this module never acquires browsers."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import platform

sys.dont_write_bytecode = True
from common import ROOT, digest, identity, write_json, install_interrupt_handlers
from normal import execute
from normal_results import catalogue, exact, playwright_results, specs
import artifact

E2E = ROOT / 'tests/e2e'
CLI = ['node', 'node_modules/@playwright/test/cli.js', 'test']


def groups(profile):
    filename = {'ci-non-browser': 'non-browser.json', 'local-browser': 'local-browser.json'}[profile]
    plan = json.loads((ROOT / 'tests/acceptance' / filename).read_text())
    if plan.get('schema_version') != 1 or plan.get('profile') != profile:
        raise ValueError('suite profile/schema differs')
    result = plan['groups']
    if profile == 'local-browser':
        result['normal']['cases'] = [c for lane in catalogue()['lanes'].values()
                                     if lane['kind'] == 'playwright' for c in lane['cases']]
    all_cases = [c for group in result.values() for c in group['cases']]
    expected_count = 18 if profile == 'ci-non-browser' else 151
    if len(all_cases) != expected_count or len(set(all_cases)) != expected_count:
        raise ValueError('suite registered case inventory changed')
    return result


def discover(group):
    result = subprocess.run([*CLI, *group['args'], '--list', '--reporter=json'],
                            cwd=E2E, capture_output=True, text=True, timeout=60, check=True)
    raw = json.loads(result.stdout)
    if raw.get('errors'):
        raise ValueError('suite discovery errors')
    exact([r[0] for r in specs(raw)], group['cases'], 'discovered suite')


def validate_results(directory, profile):
    summary = json.loads((directory / 'summary.json').read_text())
    plan = groups(profile)
    if summary.get('profile') != profile or summary.get('status') != 'PASS':
        raise ValueError('suite profile/execution did not pass')
    exact(list(summary.get('groups', {})), list(plan), 'suite group inventory')
    for name, group in plan.items():
        result = summary['groups'][name]
        path = directory / name / 'results.json'
        if result.get('command') != [*CLI, *group['args'], '--reporter=json']:
            raise ValueError('suite command differs')
        if type(result.get('exit_code')) is not int or result['exit_code'] != 0:
            raise ValueError('suite exit failed')
        if digest(path) != result.get('raw_sha256') or not (directory / name / 'command.log').is_file():
            raise ValueError('missing or changed suite raw evidence')
        actual = playwright_results(json.loads(path.read_text()), group['cases'])
        if any(c['status'] != 'PASS' for c in actual) or actual != result.get('cases'):
            raise ValueError('suite case failure or inconsistent raw result')
    return summary


def run(profile, directory, environment=None):
    directory.mkdir(parents=True, exist_ok=False)
    source = identity()
    runtime_env = {**os.environ, **(environment or {})}
    binary = Path(runtime_env.get('DEVE_SUB_BINARY', ROOT / 'target/release/deve-sub'))
    web = Path(runtime_env.get('DEVE_SUB_WEB_DIST', ROOT / 'apps/web/dist'))
    def runtime():
        return {'platform': platform.platform(), 'binary_sha256': digest(binary),
                'web_files': artifact.files(web, 'wasm') if web.is_dir() else None,
                'browser_path': runtime_env.get('DEVE_SUB_BROWSER')}
    summary = {'schema_version': 1, 'profile': profile, 'source': source,
               'status': 'FAIL', 'groups': {}, 'errors': [],
               'started_at': datetime.now(timezone.utc).isoformat(),
               'full_acceptance_readiness': 'NOT_ASSERTED'}
    try:
        summary['runtime'] = runtime()
        for name, group in groups(profile).items():
            discover(group)
            target = directory / name
            target.mkdir()
            env = {**os.environ, **(environment or {}),
                   'PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD': '1',
                   'PLAYWRIGHT_JSON_OUTPUT_FILE': str(target / 'results.json')}
            command = [*CLI, *group['args'], '--reporter=json']
            code = execute(command, E2E, env, target / 'command.log')
            raw = target / 'results.json'
            result = {'command': command, 'exit_code': code, 'raw_sha256': digest(raw),
                      'cases': playwright_results(json.loads(raw.read_text()), group['cases'])}
            summary['groups'][name] = result
            if code != 0 or any(c['status'] != 'PASS' for c in result['cases']):
                raise ValueError(f'{name}: failed/partial suite')
        if source != identity():
            raise ValueError('source changed during execution')
        if summary['runtime'] != runtime():
            raise ValueError('executed binary or Web changed during execution')
        summary['status'] = 'PASS'
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, InterruptedError) as error:
        summary['errors'].append(f'{type(error).__name__}: {error}')
    finally:
        summary['finished_at'] = datetime.now(timezone.utc).isoformat()
        write_json(directory / 'summary.json', summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['ci-non-browser'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = run(args.profile, args.output.resolve())
    print(f"{args.profile}: {result['status']}")
    return 0 if result['status'] == 'PASS' else 1


if __name__ == '__main__':
    install_interrupt_handlers()
    raise SystemExit(main())
