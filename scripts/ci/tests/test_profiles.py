"""Synthetic proof controls: a small green profile never becomes full acceptance."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import ROOT, digest, write_json
import local_browser
import suite
from normal_results import catalogue, evaluate

SOURCE = dict(commit='synthetic-commit', dirty=False, source_digest='a'*64,
              toolchain='1.97.1', lockfile_digest='b'*64)


def raw(cases):
    rows=[]
    for key in cases:
        file, title, project = key.split('::')
        rows.append(dict(file=file, title=title, ok=True, tests=[dict(
            projectName=project, expectedStatus='passed', status='expected',
            results=[dict(status='passed', retry=0, errors=[])])]))
    return dict(errors=[], stats=dict(unexpected=0, flaky=0, skipped=0), suites=[dict(specs=rows)])


def fixture(root):
    groups={}
    for name, group in suite.groups('local-browser').items():
        target=root/'suites'/name;target.mkdir(parents=True)
        write_json(target/'results.json', raw(group['cases']))
        (target/'command.log').write_text('synthetic fixture, no actual browser\n')
        groups[name]=dict(command=[*suite.CLI,*group['args'],'--reporter=json'], exit_code=0,
                          raw_sha256=digest(target/'results.json'),
                          cases=[dict(id=k,status='PASS',observed=['passed']) for k in group['cases']])
    web={n:dict(size=1,sha256='d'*64) for n in ['index.html','assets/a.js','assets/a.wasm','assets/a.css']}
    write_json(root/'suites/summary.json',dict(profile='local-browser',status='PASS',source=SOURCE,groups=groups,
        runtime=dict(binary_sha256='d'*64,web_files=web,browser_path='/synthetic/chrome')))
    write_json(root/'browser.json',dict(status='READY',launch_verified=True,browser_kind='chromium',
        browser_download_attempted=False,playwright_version='1.62.1',browser_version='151.0.7922.34',
        revision='1234',executable_sha256='c'*64,executable_path='/synthetic/chrome'))
    (root/'preparation.log').write_text('synthetic startup control\n')
    producers={}
    for kind in ['binary','wasm']:
        names=['deve-sub'] if kind=='binary' else ['index.html','assets/a.js','assets/a.wasm','assets/a.css']
        write_json(root/f'{kind}-producer.json',{**SOURCE,'schema_version':1,'kind':kind,'profile':'release',
            'compiler':'rustc 1.97.1 synthetic','features':[] if kind=='binary' else ['frontend'],
            'run_id':'local','run_attempt':'local',
            'target':'x86_64-unknown-linux-gnu' if kind=='binary' else 'wasm32-unknown-unknown',
            'files':{n:dict(size=1,sha256='d'*64) for n in names}})
        producers[kind]=digest(root/f'{kind}-producer.json')
    write_json(root/'local.json',dict(profile='local-browser',status='PASS',source=SOURCE,
        source_after=SOURCE,platform='synthetic Linux',preparation_exit=0,producer_digests=producers))


def seal(root):
    write_json(root/'manifest.json',dict(schema_version=1,files=local_browser.inventory(root)))
    return digest(root/'manifest.json')


class Profiles(unittest.TestCase):
    def test_dns_only_is_explicit_and_fails_default_full_or_cross_profile(self):
        plan=catalogue();context={'source':'synthetic'}
        report=dict(schema_version=1,lane='dns',profile='ci-non-browser',context=context,
                    status='PASS',exit_code=0,errors=[],cases=[dict(id=plan['lanes']['dns']['cases'][0],status='PASS')])
        needs={k:dict(result='success') for k in ['normal-non-browser','build-release','web-wasm','rust-gate']}
        self.assertEqual(evaluate([report],plan,context,needs,'ci-non-browser')['status'],'PASS')
        self.assertEqual(evaluate([report],plan,context,needs)['status'],'FAIL')
        report['profile']='full'
        self.assertEqual(evaluate([report],plan,context,needs,'ci-non-browser')['status'],'FAIL')
        with self.assertRaises(ValueError):evaluate([],plan,context,needs,'other')
        changed=copy.deepcopy(plan);changed['lanes']['dns']['kind']='playwright'
        with self.assertRaises(ValueError):evaluate([],changed,context,needs,'ci-non-browser')

    def test_every_registered_primary_case_retains_one_execution_owner(self):
        ci=[c for g in suite.groups('ci-non-browser').values() for c in g['cases']]
        local=[c for g in suite.groups('local-browser').values() for c in g['cases']]
        self.assertEqual((len(ci),len(local)),(18,151))
        self.assertFalse(set(ci)&set(local))
        self.assertEqual(sum(len(g['cases']) for g in catalogue()['lanes'].values()),40)

    def test_complete_synthetic_bundle_and_all_semantic_corruptions(self):
        for mutation in ['none','source','dirty','fake-ready','missing-path','producer','producer-schema','missing-case','skip','retry','missing-raw','missing-preparation','profile','raw-digest']:
            with self.subTest(mutation=mutation),tempfile.TemporaryDirectory() as tmp:
                root=Path(tmp);fixture(root)
                if mutation in ['source','dirty','profile']:
                    p=root/'local.json';j=json.loads(p.read_text())
                    if mutation=='source':j['source']['source_digest']='e'*64
                    if mutation=='dirty':j['source']['dirty']=True
                    if mutation=='profile':j['profile']='ci-non-browser'
                    write_json(p,j)
                elif mutation in ['fake-ready','missing-path']:
                    p=root/'browser.json';j=json.loads(p.read_text())
                    if mutation=='fake-ready':j['launch_verified']=False
                    else:j.pop('executable_path')
                    write_json(p,j)
                elif mutation in ['producer','producer-schema']:
                    p=root/'binary-producer.json';j=json.loads(p.read_text())
                    if mutation=='producer':j['lockfile_digest']='e'*64
                    else:j.pop('compiler')
                    write_json(p,j)
                    if mutation=='producer-schema':
                        p2=root/'local.json';j2=json.loads(p2.read_text());j2['producer_digests']['binary']=digest(p);write_json(p2,j2)
                elif mutation=='missing-preparation':(root/'preparation.log').unlink()
                elif mutation=='missing-raw':(root/'suites/legacy/results.json').unlink()
                elif mutation in ['missing-case','skip','retry','raw-digest']:
                    p=root/'suites/legacy/results.json';j=json.loads(p.read_text())
                    if mutation=='missing-case':j['suites'][0]['specs'].pop()
                    if mutation=='skip':j['suites'][0]['specs'][0]['tests'][0]['results'][0]['status']='skipped'
                    if mutation=='retry':j['suites'][0]['specs'][0]['tests'][0]['results'][0]['retry']=1
                    if mutation=='raw-digest':j['errors']=['changed']
                    write_json(p,j)
                    if mutation!='raw-digest':
                        p2=root/'suites/summary.json';j2=json.loads(p2.read_text());j2['groups']['legacy']['raw_sha256']=digest(p);write_json(p2,j2)
                expected=seal(root)
                if mutation=='none':
                    self.assertEqual(local_browser.verify(root,expected,SOURCE,True)['status'],'PASS')
                    with self.assertRaises(ValueError):local_browser.verify(root,'f'*64,SOURCE,True)
                else:
                    with self.assertRaises((ValueError,OSError)):local_browser.verify(root,expected,SOURCE,True)

    def test_release_dag_cannot_bypass_full_gate_for_either_trigger(self):
        jobs=yaml.safe_load((ROOT/'.github/workflows/release.yml').read_text())['jobs']
        gate=jobs['acceptance-gate'];self.assertNotIn('if',gate)
        self.assertEqual(gate['needs'],'verify')
        for job in ['build','release']:self.assertIn('acceptance-gate',jobs[job]['needs'])
        step=next(s for s in gate['steps'] if s.get('name')=='Require reviewed local-browser evidence for this clean source')
        self.assertNotIn('if',step);self.assertNotIn('continue-on-error',step)
        self.assertIn('--release',step['run'])
        for event in ['push','workflow_dispatch']:
            with self.subTest(event=event),tempfile.TemporaryDirectory() as tmp:
                with self.assertRaises(ValueError):local_browser.verify(Path(tmp),'',SOURCE,True)


if __name__=='__main__':unittest.main()
