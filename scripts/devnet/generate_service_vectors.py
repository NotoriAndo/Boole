"""Regenerate the real Registry API's shared-subset parity vectors, offline."""
from pathlib import Path
import hashlib
import json
import platform
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from zk_registry import service as S

with tempfile.TemporaryDirectory(prefix="boole-service-vectors-") as temporary:
    RUNTIME = Path(temporary)
    W,D=64,2
    cases=[
     ('earlier-expiry-releases-later-pass',[(6,'commit',1),(7,'commit',2),(8,'commit',3),(9,'reveal',2,'PASS',True),(10,'reveal',3,'FAIL',True)],75),
     ('inclusive-last-reveal-and-expired-attempt',[(6,'commit',1),(7,'commit',2),(70,'reveal',1,'PASS',True),(72,'reveal',2,'PASS',True)],75),
     ('wrong-binding-does-not-consume-and-later-duplicate',[(6,'commit',1),(7,'commit',2),(8,'reveal',1,'PASS',False),(9,'reveal',1,'PASS',True),(10,'reveal',2,'PASS',True)],75),
    ]
    actions=[(6+i,'commit',i+1) for i in range(16)]
    for i in range(1,16):
     if i%4!=0:actions.append((30+i,'reveal',i+1,'FAIL' if i%5==0 else 'PASS',True))
    cases.append(('generated-barrier-and-verdict-sequence',sorted(actions),90))
    output=[]
    for name,actions,last in cases:
     directory=RUNTIME/name;directory.mkdir();package=directory/'package';package.mkdir();(package/'problem.json').write_text('{}\n')
     submission=directory/'submission';submission.mkdir();(submission/'proof.txt').write_text('shared-subset deterministic submission\n')
     verdict=['PASS']
     registry=S.Registry(str(directory/'registry'),lambda *_:dict(verdict=verdict[0],reason='injected-subset-verdict'))
     registry.root.mkdir()
     manifest=registry._capture(str(package),package=True)
     entry=dict(id='problem',kind='C',status='OPEN',submittable=True,root_id='problem')
     payload=dict(entries=[entry],packages={'problem':manifest},excluded={},reveal_window=W+1,challenge_window=1)
     registry._append('catalog',payload,0,[])
     by_height={}
     for action in actions:by_height.setdefault(action[0],[]).append(action)
     rows=[]
     for h in range(1,last+1):
      registry.advance(h)
      emitted=[]
      for action in by_height.get(h,[]):
       _,op,seq,*rest=action
       solver='solver-'+str(seq);salt='salt-'+str(seq)
       if op=='commit':
        commitment=S.commitment('problem',solver,S.submission_digest(str(submission)),salt)
        result=registry.commit('problem',solver,commitment,h)
        assert result['sequence']==seq
        emitted.append(dict(op=op,sequence=seq))
       else:
        verdict[0],binding=rest
        result=registry.reveal(seq,'problem',solver,str(submission),salt if binding else salt+'-wrong',h)
        matched=registry._events()[-1]['payload']['matched']
        emitted.append(dict(op=op,sequence=seq,verdict=verdict[0],correct_binding=binding,matched=matched,outcome=result['outcome']))
      state=registry.status()
      rows.append(dict(height=h,actions=emitted,expected=dict(statuses={seq:c['status'] for seq,c in state['commits'].items()},winner=state['problems']['problem'].get('winner'))))
     output.append(dict(name=name,rows=rows,event_log_sha256=hashlib.sha256(registry.log.read_bytes()).hexdigest()))
    result=dict(schema='boole-devnet-m2-service-parity-v1',provenance=dict(source='scripts/zk_registry/service.py',source_sha256=hashlib.sha256((ROOT/'scripts/zk_registry/service.py').read_bytes()).hexdigest(),generator='generate_service_vectors.py',generator_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),python=platform.python_version(),execution='Registry.commit, Registry.reveal and Registry.advance; synthetic catalog bootstrap and injected PASS/FAIL verifier; no ERROR or CANCEL'),mapping=dict(d=D,w=W,reveal_window=W+1,problem_open_height=5),cases=output)
    path = ROOT / "fixtures/devnet-m2/service-parity-v1.json"
    path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"cases":len(output), "heights":sum(len(c["rows"]) for c in output), "fixture_sha256":hashlib.sha256(path.read_bytes()).hexdigest()}))
