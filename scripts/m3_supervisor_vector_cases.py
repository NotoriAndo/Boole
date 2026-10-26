"""Amended A1 first-failure vectors. No native run or fitted-value change."""
import copy,json,tempfile
from pathlib import Path
from unittest.mock import patch
from test_zk_registry_m3_gate import descriptor
from zk_registry.m3_gate import adapter as A,codec as C,mapping as M,front as F
from zk_registry import fuel_caps as FC

def encoded(value):return (json.dumps(value,indent=2)+'\n').encode()
def auth_value(case,statement):
 kind=case.get('authenticated','valid')
 return {'valid':statement,'no-valid':M.NO_VALID_STATEMENT,'unknown':M.UNSET_STATEMENT,'none':None}[kind]
def checked_reply(raw,d,c,caps,*,fork=False,authenticated=M.UNSET_STATEMENT):
 """Adapter decoder plus the trusted caller's published per-kernel allowances."""
 r=A.decode_reply(raw,d,c,fork_authorized=fork,authenticated_statement=authenticated)
 for k in ('lean','nanoda'):
  v=r['kernels'][k]
  if v['state']=='executed':
   expected=min(v['debit'],caps[k]) if r['row']=='resource_cap' else v['debit']
   if v['charged']!=expected or (r['row']!='resource_cap' and v['debit']>caps[k]):raise ValueError('caller package allowance')
 return r

def measured():
 a=FC.load_authority();ref,r=next((k,r) for k,r in sorted(a['references'].items()) if r['eligible'])
 cap=dict(version=1,reference=ref,problem=r['problem'],environment=r['environment'],statement=r['statement'],own_declarations=r['own_declarations'],units={k:r['units'][k]*4 for k in ('lean','nanoda')})
 p=dict(version=1,mode='certified-own',problem=r['problem'],env_id=r['environment'],target=[['str','Fixture'],['str','target']],statement=r['statement'],reference_sha256=r['reference_export'],inventory_sha256='8'*64,fuel_cap=dict(cap=cap,sha256=FC.digest(cap)))
 return a,p

def context(p,bundle=b'broken-transport'):
 return dict(chain_id=720926,problem_id='2'*64,commit_hash='3'*64,solver='4'*40,env_id=p['env_id'],export_digest=C.sha(bundle),package_sha256=C.sha(C.canonical(p)),leaf_kind=1,lane=1,slot_fuel=2**63)

def cap_vectors():
 a,pkg=measured();d=descriptor();d['fuel_authority_sha256']=C.sha((Path(__file__).parent/'zk_registry/data/fuel_cap_authority_v1.json').read_bytes());rows=[]
 cases=[('measured-reference',pkg)]
 wrong=copy.deepcopy(pkg);wrong['reference_sha256']='b'*64;cases.append(('different-reference-export',wrong))
 wrong=copy.deepcopy(pkg);wrong['env_id']='7'*64;cases.append(('cap-environment-mismatch',wrong))
 full=dict(pkg,mode='full',fuel_cap=None);cases.append(('full-without-public-counter',full))
 for name,p in cases:
  for phase in ('first','retry','replay'):
   c=context(p)
   try:budget=A.published_caps(p,d,c);row='accept';caps=dict(**budget,statement=p['statement'])
   except F.Refusal as e:row=e.row;caps=dict(lean=0,nanoda=0,statement=p['statement'])
   states={k:dict(state='executed',debit=n,charged=n) for k,n in [('lean',1),('nanoda',2)]} if row=='accept' else M.not_run()
   reply=A.reply(d,c,M.classify(M.Observation(row),d,phase),p['statement'],states,3 if row=='accept' else 0)
   checked_reply(C.canonical(reply),d,c,caps)
   rows.append(dict(name=name,phase=phase,package_hex=C.canonical(p).hex(),caps=caps,context=c,reply_hex=C.canonical(reply).hex()))
 return encoded(dict(descriptor_hex=C.encode(d).hex(),cases=rows))

def reconstruction_vectors():
 _,cert=measured();d0=descriptor();d0['fuel_authority_sha256']=C.sha((Path(__file__).parent/'zk_registry/data/fuel_cap_authority_v1.json').read_bytes())
 base=dict(version=1,mode='full',problem='fixture',env_id='7'*64,target=[['str','Fixture']],statement='a'*64,reference_sha256='b'*64,inventory_sha256='8'*64,fuel_cap=None)
 specs=[]
 def add(name,**kw):specs.append(dict(name=name,**kw))
 add('malformed-package-and-broken-transport',invalid=b'')
 add('target-false-absent-reference',target=False)
 add('target-num-bool',target=[['num',False]])
 add('target-empty-component',target=[['str','']])
 add('package-duplicate-field',mutation='duplicate')
 add('package-fields-reordered',mutation='reordered')
 add('package-missing-reference-field',mutation='missing-reference')
 add('package-noncanonical',mutation='newline')
 add('target-too-many-components',target=[['str','x']]*257)
 add('target-u64-maximum',target=[['num',2**64-1]])
 add('package-boolean-version',mutation='version-bool')
 add('package-float',mutation='float')
 add('package-depth-127',mutation='depth-127')
 add('package-depth-128',mutation='depth-128')
 add('valid-package-broken-transport')
 add('open-environment-mismatch',open_env='8'*64,bundle=F.pack(b'not reached'))
 for suffix,kwargs in [('absent-reference',{}),('open-env',dict(open_env='8'*64)),('corrupt-bound-bundle',dict(corrupt=True))]:
  add('full-without-public-counter-'+suffix,public_counter=False,**kwargs)
  p=copy.deepcopy(cert);p['reference_sha256']='b'*64
  add('reference-mismatch-'+suffix,package=p,**kwargs)
 p=copy.deepcopy(cert);p['env_id']='7'*64;add('cap-environment-mismatch',package=p)
 for kind,lane in [(2,1),(1,2),(2,2)]:
  for typed in (False,True):add('kind%d-lane%d-%s'%(kind,lane,'typed' if typed else 'plain'),kind=kind,lane=lane,decoder='invalid-data' if typed else None,no_output=typed)
 add('decoder-invalid-data-no-output',decoder='invalid-data',no_output=True)
 add('decoder-digest-no-output',decoder='invalid-data',cause='digest',corrupt=True,no_output=True)
 add('decoder-ok-corrupt-output',decoder='ok',corrupt=True)
 add('decoder-invalid-data-present-output',decoder='invalid-data')
 add('decoder-invalid-data-corrupt-output',decoder='invalid-data',corrupt=True)
 add('decoder-write-fault',decoder='unavailable',cause='write',no_output=True)
 add('malformed-package-and-decoder-write-fault',invalid=b'',decoder='unavailable',cause='write',no_output=True)
 add('decoder-missing-evidence',decoder='missing',no_output=True)
 add('decoder-untyped-exit1',decoder='exit1',no_output=True)
 add('decoder-ok-bound-output',decoder='ok')
 add('decoder-ok-no-output',decoder='ok',no_output=True)
 for field in ('version','context_sha256','gate_rule_digest','descriptor_sha256','decoder_sha256','transport_sha256','cause','raw_sha256','raw_bytes'):
  add('decoder-foreign-'+field,decoder='ok',status_mutation=field)
 add('decoder-noncanonical',decoder='ok',status_mutation='newline')
 for mutation in ('cap','duplicate','unknown','bytes-bool','bytes-zero','malformed'):
  add('decoder-status-'+mutation,decoder='ok',status_mutation=mutation)
 add('decoder-invalid-data-fabricated-output',decoder='invalid-data',status_mutation='fabricated',no_output=True)
 add('decoder-unavailable-wrong-cause',decoder='unavailable',cause='frame',no_output=True)
 add('quarantine-with-no-statement',invalid=b'',fork=True)
 add('quarantine-malformed-target-with-nonzero-field',target=False,fork=True)
 add('quarantine-with-valid-authenticated-statement',fork=True)
 rows=[];pin='c'*64;transport=C.sha(b'authenticated reconstructed compressed transport')
 for spec in specs:
  p=copy.deepcopy(spec.get('package',base));p['target']=spec.get('target',p['target'])
  inv=C.canonical(dict(version=1,environment=p['env_id'],base={},library={}))
  d=copy.deepcopy(d0);d['artifact_table_sha256']=C.sha(inv);p['inventory_sha256']=C.sha(inv)
  raw=C.canonical(p)
  if 'invalid' in spec:raw=spec['invalid']
  mutation=spec.get('mutation')
  if mutation=='duplicate':raw=raw.replace(b'"version":1',b'"version":1,"version":1',1)
  elif mutation=='reordered':raw=C.canonical(dict(reversed(list(p.items()))))
  elif mutation=='missing-reference':v=copy.deepcopy(p);del v['reference_sha256'];raw=C.canonical(v)
  elif mutation=='newline':raw+=b'\n'
  elif mutation=='version-bool':raw=raw.replace(b'"version":1',b'"version":true',1)
  elif mutation=='float':raw=raw.replace(b'"fuel_cap":null',b'"fuel_cap":1.0',1)
  elif mutation in ('depth-127','depth-128'):
   depth=int(mutation.split('-')[1])-1
   raw=raw.replace(b'"fuel_cap":null',b'"fuel_cap":'+b'['*depth+b'0'+b']'*depth,1)
  bundle=spec.get('bundle',b'broken-transport');c=context(p,bundle);c.update(package_sha256=C.sha(raw),env_id=spec.get('open_env',p['env_id']),leaf_kind=spec.get('kind',1),lane=spec.get('lane',1))
  if spec.get('corrupt'):c['export_digest']='6'*64
  output=None if spec.get('no_output') else bundle
  decoder=spec.get('decoder');status=None
  if decoder:
   v=dict(version=2,context_sha256=C.sha(C.canonical(c)),gate_rule_digest=C.identity(d),descriptor_sha256=C.sha(C.encode(d)),decoder_sha256=pin,transport_sha256=transport,status=decoder,cause=spec.get('cause','none' if decoder=='ok' else 'frame'),raw_sha256=C.sha(bundle) if decoder=='ok' else None,raw_bytes=len(bundle) if decoder=='ok' else None)
   field=spec.get('status_mutation')
   if field=='version':v[field]=1
   elif field=='raw_bytes':v[field]+=1
   elif field=='cause':v[field]='stderr-error'
   elif field=='bytes-bool':v['raw_bytes']=True
   elif field=='bytes-zero':v['raw_bytes']=0
   elif field=='unknown':v['extra']=1
   elif field=='fabricated':v['raw_sha256']=C.sha(bundle);v['raw_bytes']=len(bundle)
   elif field and field not in ('newline','cap','duplicate','malformed'):v[field]='d'*64
   status=C.canonical(v)+(b'\n' if field=='newline' else b'')
   if field=='cap':status=b'x'*8193
   elif field=='duplicate':status=status.replace(b'"version":2',b'"version":2,"version":2',1)
   elif field=='malformed':status=b'{broken-json'
   if decoder=='missing':status=b''
   elif decoder=='exit1':status=b'{"exit_code":1,"stderr":"data error"}'
  for phase in ('first','retry','replay'):
   with tempfile.TemporaryDirectory() as tmp,patch.object(A,'verify_tools'):
    if spec.get('fork'):
     try:valid=F.package(raw,c['package_sha256'],F.Inventory(inv,C.sha(inv)));authenticated=valid['statement']
     except F.Refusal as e:
      if e.row!='package_invalid':raise
      authenticated=M.NO_VALID_STATEMENT
     statement=M.quarantine_statement(c,authenticated,fork_authorized=True)
     reply=A.reply(d,c,M.classify(M.Observation('quarantine_void',fork_authorized=True),d,phase),statement,M.not_run(),0,fork_authorized=True,authenticated_statement=authenticated)
    else:
     reply=A.run(output,raw,inv,c,C.encode(d),C.identity(d),None,None,None,Path(tmp)/'work',phase=phase,public_counter=spec.get('public_counter',True),decoder_status=status,decoder_pin=pin if status is not None else None,transport_digest=transport if status is not None else None)
   A.decode_reply(C.canonical(reply),d,c,fork_authorized=spec.get('fork',False),authenticated_statement=authenticated if spec.get('fork') else M.UNSET_STATEMENT)
   rows.append(dict(name=spec['name'],phase=phase,fork_authorized=spec.get('fork',False),descriptor_hex=C.encode(d).hex(),inventory_hex=inv.hex(),context=c,package_hex=raw.hex(),bundle_hex=None if output is None else output.hex(),decoder_status_hex=None if status is None else status.hex(),decoder_pin=pin,transport_digest=transport,public_counter=spec.get('public_counter',True),reply_hex=C.canonical(reply).hex(),row=reply['row']))
 return encoded(dict(cases=rows))
