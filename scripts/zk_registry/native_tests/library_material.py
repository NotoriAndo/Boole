"""Honest alternate proofs using public Init material outside a pruned base."""
from pathlib import Path
import argparse
import copy
import hashlib
import json
import os
import subprocess
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from zk_registry import closure as D, lean_runner as L
p=argparse.ArgumentParser();p.add_argument('--build',required=True,type=Path);p.add_argument('--prefix',required=True,type=Path)
p.add_argument('--exporter',required=True,type=Path);p.add_argument('--nanoda',required=True,type=Path)
p.add_argument('--out',required=True,type=Path);a=p.parse_args();a.out.mkdir(parents=True)
root=Path(__file__).resolve().parents[1]
sources={
 'reference':'theorem target : True := True.intro',
 'structure':'def helper : Nat × Nat := (1, 2)\ntheorem target : True := (fun (_ : Nat × Nat) => True.intro) helper',
 'library-def':'theorem target : True := Function.comp (fun x : True => x) (fun x : True => x) True.intro',
 'wrong-statement':'theorem target : True ∧ True := ⟨Function.comp (fun x : True => x) (fun x : True => x) True.intro, True.intro⟩',
 'library-axiom':'noncomputable def helper : Nat := Classical.choice (show Nonempty Nat from ⟨0⟩)\ntheorem target : True := (fun (_ : Nat) => True.intro) helper',
 'own-inductive':'inductive OwnBox where | mk\ndef helper : OwnBox := OwnBox.mk\ntheorem target : True := (fun (_ : OwnBox) => True.intro) helper',
 'own-opaque':'opaque bad : Nat := 0\ntheorem target : True := (fun (_ : Nat) => True.intro) bad',
 'own-literal':'def helper : Nat × Nat := (340282366920938463463374607431768211456, 2)\ntheorem target : True := (fun (_ : Nat × Nat) => True.intro) helper',
 'own-axiom':'axiom bad : Nat\ntheorem target : True := (fun (_ : Nat) => True.intro) bad',
 'library-theorem':'theorem target : True := (fun (_ : ([1,2,3] : List Nat).reverse.reverse = [1,2,3]) => True.intro) (List.reverse_reverse [1,2,3])',
}
sources={key:'import Init\nnamespace PublicControl\n'+value+'\nend PublicControl\n' for key,value in sources.items()}
(a.out/'PREREGISTRATION.json').write_text(json.dumps(dict(generator_sha256=D.digest(__file__),
 sources={k:dict(utf8=v,sha256=hashlib.sha256(v.encode()).hexdigest()) for k,v in sources.items()},
 provenance='Authored generator and public pinned Init only. No local proof or export input.'),indent=2)+'\n')
env=dict(os.environ,PATH=str(a.prefix/'bin')+':/usr/bin:/bin:/usr/sbin:/sbin',LEAN_SYSROOT=str(a.prefix),LEAN_PATH=str(a.prefix/'lib/lean'),LEAN_STACK_SIZE_KB='262144',LEAN_NAT_MAX_SIZE='4096')
rows=[]
def execute(argv,label,env=env,cwd=a.out):
 result=subprocess.run(list(map(str,argv)),cwd=cwd,env=env,capture_output=True,timeout=180)
 (a.out/(label+'.stdout')).write_bytes(result.stdout);(a.out/(label+'.stderr')).write_bytes(result.stderr)
 return result
exports={}
for label,text in sources.items():
 work=a.out/label;work.mkdir();source=work/'PublicControl.lean';source.write_text(text)
 result=execute([a.prefix/'bin/lean','-o',work/'PublicControl.olean',source],label+'-compile',cwd=work)
 assert result.returncode==0,(label,result.stderr,result.stdout)
 taskenv=dict(env,LEAN_PATH=str(a.prefix/'lib/lean')+':'+str(work))
 result=execute([a.exporter,'PublicControl','--','PublicControl.target',*D.PRIMITIVES,*D.QUOTIENTS,'Classical.choice','propext','Quot.sound'],label+'-export',env=taskenv,cwd=work)
 assert result.returncode==0,(label,result.stderr)
 export=a.out/(label+'.ndjson');export.write_bytes(result.stdout);exports[label]=export

def compare(label,proof,base=False):
 work=a.out/('compare-'+label);work.mkdir();status=work/'status';status.touch()
 config=dict(challenge_module='Challenge',solution_module='ProofMod',theorem_names=['PublicControl.target'],permitted_axioms=['Classical.choice','propext','Quot.sound'],enable_nanoda=True,max_exported_constants=1000000,lean_kernel=copy.deepcopy(L.testnet_caps()['lean']),nanoda_kernel=copy.deepcopy(L.testnet_caps()['nanoda']))
 if base:
  config.update(certified_base_olean=str(a.out/'state.olean'),candidate_declarations=['PublicControl.target','PublicControl.helper','PublicControl.OwnBox','PublicControl.bad'])
  if '-own-lean-cap-' in label:config['lean_kernel']['aggregate_raw']=63
  if '-own-nanoda-cap-' in label:config['nanoda_kernel']['aggregate_work']=63
  if '-own-literal-' in label:config['lean_kernel']['literal_bytes']=1
  if '-own-count-' in label:config['max_exported_constants']=1
  if '-target-not-own-' in label:config['candidate_declarations']=['PublicControl.helper']
  if '-disallowed-axiom-' in label:config['permitted_axioms']=['propext','Quot.sound']
 path=work/'config';path.write_text(json.dumps(config))
 taskenv=dict(env,COMPARATOR_NANODA=str(a.nanoda),COMPARATOR_LEAN4EXPORT=str(a.exporter),COMPARATOR_STATUS_FILE=str(status),COMPARATOR_LANDRUN=str(root/'data/development-landrun.sh'))
 challenge=exports['reference']
 if '-challenge-base-' in label or '-base-payload-' in label:
  source=challenge if '-challenge-base-' in label else exports[proof]
  changed=[];mutated=False
  for line in source.read_text().splitlines():
   row=json.loads(line)
   if not mutated and 'inductive' in row:
    row['inductive']['types'][0]['type']=1;mutated=True
   changed.append(json.dumps(row))
  assert mutated
  modified=work/'altered-base.ndjson';modified.write_text('\n'.join(changed)+'\n')
  if '-challenge-base-' in label:challenge=modified
  else:proof='modified';exports[proof]=modified
 result=execute([a.build/'comparator/.lake/build/bin/comparator',path,'--exports',challenge,exports[proof]],label,env=taskenv,cwd=work)
 record=dict(label=label,rc=result.returncode,events=[json.loads(s) for s in status.read_text().splitlines()])
 rows.append(record);(a.out/'RECORDS.json').write_text(json.dumps(rows,indent=2)+'\n');return record
# The legacy full two-kernel path establishes these are honest public proofs.
for label in ('reference','structure','library-def','library-theorem','library-axiom'):
 row=compare('legacy-'+label,label);assert row['rc']==0,row
writer=root/'native_tests/StateFromExport.lean'
taskenv=dict(env,LEAN_PATH=':'.join(map(str,[a.prefix/'lib/lean',a.build/'lean4export/.lake/build/lib/lean'])))
result=execute([a.prefix/'bin/lean','--run',writer,exports['reference'],a.out/'state.olean','PublicControl.target'],'state',env=taskenv)
assert result.returncode==0,(result.stdout,result.stderr)
probeenv=dict(env,LEAN_PATH=':'.join(map(str,[a.prefix/'lib/lean',a.build/'comparator/.lake/build/lib/lean',a.build/'lean4export/.lake/build/lib/lean'])))
result=execute([a.prefix/'bin/lean','--run',root/'native_tests/BaseIdentityOrder.lean',a.out/'state.olean'],'base-identity-order',env=probeenv)
assert result.returncode==0,(result.stdout,result.stderr)
failures=[]
alternatives=[]
for label in ('structure','library-def','library-theorem','library-axiom'):
 row=compare('capped-alternative-'+label,label,True);alternatives.append(row)
 if row['rc']!=70 or not any(e.get('stage')=='error' and e.get('reason')=='library-outside-certified-base' for e in row['events']) or any(e.get('stage') in ('fuel','nanoda-fuel') for e in row['events']):failures.append(row)
# Unsupported library material must not mask independently provable own INVALID.
for guard,proof,expected,reason in [('profile','own-inductive',2,'certified-base-mismatch'),('profile','own-opaque',2,'certified-base-mismatch'),('profile','own-axiom',2,'certified-base-mismatch'),('own-lean-cap','library-def',3,'resource-cap'),('own-nanoda-cap','library-def',3,'resource-cap'),('own-literal','own-literal',3,'resource-cap'),('own-count','structure',2,'resource-cap'),('target-not-own','library-def',2,'certified-base-mismatch')]:
 row=compare('capped-'+guard+'-'+proof,proof,True)
 if row['rc']!=expected or not any(e.get('stage')=='policy' and e.get('reason')==reason for e in row['events']):failures.append(row)
for guard,proof,expected,reason in [('statement','wrong-statement',2,'comparator-statement-mismatch'),('disallowed-axiom','library-axiom',2,'disallowed-axiom'),('base-payload','library-def',2,'certified-base-mismatch'),('challenge-base','library-def',70,'kernel-unavailable')]:
 row=compare('capped-'+guard+'-'+proof,proof,True)
 stage='error' if expected==70 else 'policy'
 if row['rc']!=expected or not any(e.get('stage')==stage and e.get('reason')==reason for e in row['events']):failures.append(row)
measurement=dict(public_alternatives=len(alternatives),library_scope_hits=sum(any(e.get('reason')=='library-outside-certified-base' for e in r['events']) for r in alternatives),provenance='All alternate proof sources above are committed public generated Init material; no private catalog sample.')
(a.out/'MEASUREMENT.json').write_text(json.dumps(measurement,indent=2)+'\n')
(a.out/'RESULT.json').write_text(json.dumps(dict(status='FAIL' if failures else 'PASS',rows=rows,failures=failures,measurement=measurement,scope='Public alternatives: typed terminal library ERROR after own profile, target, structural and mandatory declaration-charge caps; legacy full checks PASS.'),indent=2)+'\n')
assert not failures,failures
