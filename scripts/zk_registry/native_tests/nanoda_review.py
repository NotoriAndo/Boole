"""Generated native checker inputs; no reference proof or persisted fixture shapes."""
from pathlib import Path
import argparse
import json
import os
import resource
import subprocess
import sys
import unittest

options=argparse.ArgumentParser()
options.add_argument('--binary',required=True,type=Path)
options.add_argument('--caps',required=True,type=Path)
options.add_argument('--out',required=True,type=Path)
args,remaining=options.parse_known_args()
args.out.mkdir(parents=True)
limits=json.loads(args.caps.read_text())['nanoda']
records=[]


def generated(kind):
    metadata=dict(meta=dict(exporter=dict(name='lean4export',version='3.1.0'),format=dict(version='3.1.0'),lean=dict(githash='5045d0056413266e57c625dcd7c365b10e377c52',version='4.34.1')))
    rows=[metadata,{'in':1,'str':dict(pre=0,str='Generated0')},{'ie':0,'sort':0}]
    target=0;parameters=[]
    if kind=='sort':
        rows += [{'ie':1,'bvar':0},{'ie':2,'lam':dict(binderInfo='default',body=1,name=0,type=0)}];target=2
    elif kind=='pi':
        rows += [{'ie':1,'app':dict(fn=0,arg=0)}];target=1
    elif kind=='unknown':
        rows += [{'in':2,'str':dict(pre=0,str='Missing')},{'ie':1,'const':dict(name=2,us=[])}];target=1
    elif kind=='universes':
        rows += [{'in':2,'str':dict(pre=0,str='u')},{'il':1,'param':2},{'ie':1,'sort':1},
                 {'axiom':dict(name=1,type=1,levelParams=[2],isUnsafe=False)},
                 {'in':3,'str':dict(pre=0,str='Generated1')},{'ie':2,'const':dict(name=1,us=[])}]
        rows += [{'axiom':dict(name=3,type=2,levelParams=[],isUnsafe=False)}]
        return rows,['Generated0','Generated1']
    rows += [{'axiom':dict(name=1,type=target,levelParams=parameters,isUnsafe=False)}]
    return rows,['Generated0']


def invoke(label,kind='valid',*,spawn_failure=False,status_link=False):
    directory=args.out/label;directory.mkdir()
    rows,permitted=generated(kind)
    export=directory/'generated.ndjson';export.write_text(''.join(json.dumps(row)+'\n' for row in rows))
    config=directory/'config.json';config.write_text(json.dumps(dict(export_file_path=str(export),num_threads=4,nat_extension=True,string_extension=True,permitted_axioms=permitted,gate_limits=limits)))
    status=directory/'status.json';canary=directory/'canary'
    before=None
    if status_link:
        canary.write_bytes(b'');before=(canary.read_bytes(),canary.stat().st_mtime_ns);status.symlink_to(canary)
        config=directory/'absent-config.json'  # availability outcome still publishes status
    else:status.touch()
    def restrict():resource.setrlimit(resource.RLIMIT_NPROC,(0,0))
    result=subprocess.run([str(args.binary),str(config)],env=dict(os.environ,NANODA_STATUS_FILE=str(status)),capture_output=True,text=True,timeout=20,preexec_fn=restrict if spawn_failure else None)
    record=dict(label=label,kind=kind,exit_code=result.returncode,stdout=result.stdout,stderr=result.stderr,
                status=json.loads(status.read_text()) if status.is_file() and status.stat().st_size else None,
                canary_unchanged=(canary.read_bytes(),canary.stat().st_mtime_ns)==before if status_link else None)
    records.append(record);(args.out/'RECORDS.json').write_text(json.dumps(records,indent=2)+'\n')
    return record


def decode_case(label, payload=None, **settings):
    directory=args.out/label;directory.mkdir()
    rows,permitted=generated('valid')
    if label.startswith('semver-'):
        rows[0]['meta']['format']['version']={'semver-low':'3.0.0','semver-high':'3.2.0','semver-invalid':'bad'}[label]
    if label=='nat-setting': rows.insert(-1,{'ie':1,'natVal':'1'})
    if label=='string-setting': rows.insert(-1,{'ie':1,'strVal':'public generated literal'})
    export=directory/'generated.ndjson'
    export.write_bytes(payload if payload is not None else ''.join(json.dumps(row)+'\n' for row in rows).encode())
    config=dict(export_file_path=str(export),num_threads=4,nat_extension=True,string_extension=True,
        permitted_axioms=permitted,unpermitted_axiom_hard_error=True,gate_limits=limits)
    config.update(settings)
    path=directory/'config.json';path.write_text(json.dumps(config));status=directory/'status.json';status.touch()
    result=subprocess.run([str(args.binary),str(path)],env=dict(os.environ,NANODA_STATUS_FILE=str(status)),
        capture_output=True,text=True,timeout=20)
    record=dict(label=label,exit_code=result.returncode,status=json.loads(status.read_text()),stdout=result.stdout,stderr=result.stderr)
    records.append(record);(args.out/'RECORDS.json').write_text(json.dumps(records,indent=2)+'\n')
    return record


class NativeReview(unittest.TestCase):
    def test_generated_valid_declaration(self):
        self.assertEqual(invoke('valid')['exit_code'],0)

    def test_semantic_rejections_are_typed(self):
        for kind in ('sort','pi','unknown','universes'):
            with self.subTest(kind=kind):self.assertEqual(invoke(kind,kind)['exit_code'],2)

    def test_generated_decoder_faults_are_unavailable(self):
        cases=[('semver-low',None,{}),('semver-high',None,{}),('semver-invalid',None,{}),
            ('axiom-config-hard',None,dict(permitted_axioms=[])),
            ('axiom-config-skipped',None,dict(permitted_axioms=[],unpermitted_axiom_hard_error=False)),
            ('nat-setting',None,dict(nat_extension=False)),('string-setting',None,dict(string_extension=False)),
            ('serde-eof',b'{"meta":',{})]
        for label,payload,settings in cases:
            with self.subTest(case=label):
                record=decode_case(label,payload,**settings)
                self.assertEqual(record['exit_code'],70,record)
                self.assertEqual(record['status']['code'],70)

    def test_generated_decoder_syntax_is_unavailable(self):
        # A mismatched closing bracket is Syntax; an unfinished object is EOF.
        record=decode_case('serde-syntax',b'{]')
        self.assertEqual(record['exit_code'],70,record)
        self.assertEqual(record['status']['code'],70)

    def test_status_symlink_never_changes_canary(self):
        record=invoke('status-link',status_link=True)
        self.assertTrue(record['canary_unchanged'])
        self.assertEqual(record['exit_code'],70)

    @unittest.skipUnless(sys.platform=='linux' and os.getuid()!=0,'Linux non-root RLIMIT_NPROC physical-spawn control; both OSes also run Rust injected-spawn unit tests')
    def test_physical_load_spawn_failure_is_unavailable(self):
        self.assertEqual(invoke('load-spawn',spawn_failure=True)['exit_code'],70)


if __name__=='__main__':
    unittest.main(argv=[sys.argv[0],*remaining])
