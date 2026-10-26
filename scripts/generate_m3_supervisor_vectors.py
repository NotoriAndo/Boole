"""Shared development vectors checked by the Python and Rust gate consumers."""
import copy,json
from pathlib import Path
from test_zk_registry_m3_gate import descriptor
from zk_registry.m3_gate import adapter as A,codec as C,mapping as M
from zk_registry import fuel_caps as FC
ROOT=Path(__file__).resolve().parents[1]/'fixtures/m3-real-gate'

from m3_supervisor_vector_cases import cap_vectors,reconstruction_vectors,checked_reply
from m3_open_layout_vectors import open_layout_vectors

def generate():
 d=descriptor();raw=C.encode(d);dc=[]
 def add(name,b):
  try:identity=C.identity(C.decode(b));ok=True
  except (ValueError,TypeError):identity=None;ok=False
  dc.append(dict(name=name,raw_hex=b.hex(),valid=ok,identity=identity))
 add('valid',raw);add('reordered',C.canonical(dict(reversed(list(d.items())))))
 add('duplicate',raw.replace(b'"version":1',b'"version":1,"version":1',1))
 for name,b in [('newline',raw+b'\n'),('bom',b'\xef\xbb\xbf'+raw),('invalid_utf8',raw[:-1]+b'\xff}'),('surrogate',raw.replace(b'boole-m3-real',b'boole-\\ud800-real')),('unicode_escape',raw.replace(b'boole-m3-real',b'boole-m3-rea\\u006c'))]:add(name,b)
 for key,value in [('version',True),('grammar',1.0),('json_nesting_limit',128),('fuel_authority_sha256','0'*64),('source_sha256','A'*64),('kernel_set_digest','9'*64),('version',2**64),('version',-1),('version',None)]:
  x=copy.deepcopy(d);x[key]=value;add('field-'+key+'-'+str(value),C.canonical(x))
 for name,fn in [('unknown',lambda x:x.update(path='/synthetic')),('linux-missing',lambda x:x['profiles'].pop('Linux-aarch64')),('linux-assurance',lambda x:x['profiles']['Linux-aarch64'].update(assurance='development')),('zero-weight',lambda x:x['fuel'].update(lean_weight=0)),('wrong-policy',lambda x:x['policy'].update(rules_sha256='1'*64)),('extra-profile',lambda x:x['profiles'].update({'Linux-x86_64':x['profiles']['Linux-aarch64']})),('tool-zero',lambda x:x['profiles']['Darwin-arm64']['tools'].update(codec='0'*64)),('tools-order',lambda x:x['profiles']['Darwin-arm64'].update(tools=dict(reversed(list(x['profiles']['Darwin-arm64']['tools'].items()))))),('profiles-order',lambda x:x.update(profiles=dict(reversed(list(x['profiles'].items()))))),('interpreter',lambda x:x['reference_interpreter'].update(version='3.13')),('charge-policy',lambda x:x['fuel'].update(cap_charge='v1'))]:
  x=copy.deepcopy(d);fn(x);add(name,C.canonical(x))
 context=dict(chain_id=720926,problem_id='2'*64,commit_hash='3'*64,solver='4'*40,env_id='5'*64,export_digest='6'*64,package_sha256='7'*64,leaf_kind=1,lane=1,slot_fuel=30)
 caps=dict(lean=10,nanoda=20,statement='a'*64)
 rc=[]
 def reply_case(name,v,fork=False):
  b=C.canonical(v)
  try:checked_reply(b,d,context,caps,fork=fork,authenticated=caps['statement']);ok=True
  except (ValueError,TypeError):ok=False
  rc.append(dict(name=name,raw_hex=b.hex(),valid=ok,fork_authorized=fork,authenticated='valid'))
 replies={}
 for name in M.ROWS:
  mapped=M.classify(M.Observation(name,fork_authorized=name=='quarantine_void'),d)
  states=M.not_run();fuel=0;statement=caps['statement']
  if name in ('accept','kernel_reject','kernel_disagreement'):
   states={k:dict(state='executed',debit=n,charged=n) for k,n in [('lean',5),('nanoda',7)]};fuel=12
  elif name=='resource_cap':states=dict(lean=dict(state='not-run',debit=None,charged=0),nanoda=dict(state='executed',debit=21,charged=20));fuel=20
  elif name=='output_terminal':states=M.output_limit()
  elif mapped['outcome']=='DEFER':states=M.unknown();fuel=None
  if name=='package_invalid':statement=M.void_statement(context)
  v=A.reply(d,context,mapped,statement,states,fuel,fork_authorized=name=='quarantine_void',authenticated_statement=caps['statement']);replies[name]=v
  reply_case(name,v,name=='quarantine_void')
 good=replies['accept']
 for key,value in [('reason',0x0307),('version',True),('row','unknown'),('outcome','VOID'),('action','retry'),('failure_class','I'),('detail','unknown'),('gate_error','oops'),('native_followup','x'),('gate_rule_digest','1'*64),('descriptor_sha256','1'*64),('input_digest','1'*64),('fuel',31),('statement_id','0'*64),('statement_id',M.void_statement(context)),('ext',[1]),('assurance','production')]:
  v=copy.deepcopy(good);v[key]=value;reply_case('bad-'+key+'-'+str(value),v)
 for name,row,k,value in [('accept-one-kernel','accept','lean',dict(state='not-run',debit=None,charged=0)),('unknown-fabricated','node_fault','lean',dict(state='unknown',debit=0,charged=0)),('not-run-fabricated','front_invalid','lean',dict(state='not-run',debit=0,charged=0)),('wrong-charge','accept','lean',dict(state='executed',debit=5,charged=4)),('state-enum','node_fault','lean',dict(state='finished',debit=None,charged=None)),('void-executed','generator_unavailable','lean',dict(state='executed',debit=0,charged=0)),('terminal-fabricated','output_terminal','lean',dict(state='not-run',debit=None,charged=0))]:
  v=copy.deepcopy(replies[row]);v['kernels'][k]=value;reply_case(name,v)
 v=copy.deepcopy(replies['package_invalid']);v['statement_id']='a'*64;reply_case('void-wrong-fingerprint',v)
 reply_case('fork-not-authorized',replies['quarantine_void'])
 v=copy.deepcopy(good);v['counter_identity']={};reply_case('counter-missing-fields',v)
 extra=copy.deepcopy(good);extra['unknown']=1;reply_case('unknown-field',extra)
 # The caller also authenticates the package allowance; both consumers check it.
 v=copy.deepcopy(good);v['kernels']['lean']=dict(state='executed',debit=11,charged=11);v['fuel']=18
 A.decode_reply(C.canonical(v),d,context)
 rc.append(dict(name='package-cap-plus-one',raw_hex=C.canonical(v).hex(),valid=False,fork_authorized=False,authenticated='valid'))
 v=copy.deepcopy(replies['resource_cap']);v['kernels']['nanoda']['charged']=19;v['fuel']=19
 A.decode_reply(C.canonical(v),d,context)
 rc.append(dict(name='cap-charge-not-exact-min',raw_hex=C.canonical(v).hex(),valid=False,fork_authorized=False,authenticated='valid'))
 for auth in ('no-valid','unknown','none','valid'):
  fallback=M.quarantine_fingerprint(context)
  v=copy.deepcopy(replies['quarantine_void']);v['statement_id']=fallback
  b=C.canonical(v)
  authenticated={'no-valid':M.NO_VALID_STATEMENT,'unknown':M.UNSET_STATEMENT,'none':None,'valid':caps['statement']}[auth]
  try:checked_reply(b,d,context,caps,fork=True,authenticated=authenticated);ok=True
  except (ValueError,TypeError):ok=False
  rc.append(dict(name='quarantine-fallback-'+auth,raw_hex=b.hex(),valid=ok,fork_authorized=True,authenticated=auth))
 for statement in ('0'*64,M.void_statement(context),'b'*64):
  v=copy.deepcopy(replies['quarantine_void']);v['statement_id']=statement
  rc.append(dict(name='bad-quarantine-statement-'+statement,raw_hex=C.canonical(v).hex(),valid=False,fork_authorized=True,authenticated='no-valid'))
 foreign=dict(context,commit_hash='e'*64)
 v=copy.deepcopy(replies['quarantine_void']);v['statement_id']=M.quarantine_fingerprint(foreign)
 rc.append(dict(name='foreign-context-quarantine-fallback',raw_hex=C.canonical(v).hex(),valid=False,fork_authorized=True,authenticated='no-valid'))
 for name in ('accept','front_invalid','package_invalid','fuel_binding_invalid','generator_unavailable'):
  v=copy.deepcopy(replies[name]);v['statement_id']=M.quarantine_fingerprint(context)
  rc.append(dict(name='quarantine-fingerprint-in-'+name,raw_hex=C.canonical(v).hex(),valid=False,fork_authorized=False,authenticated='no-valid'))
 return {'open-layout-cases.json':open_layout_vectors(),'reconstruction-cases.json':reconstruction_vectors(),'cap-cases.json':cap_vectors(),'descriptor-v1.json':raw,'descriptor-cases.json':(json.dumps(dc,indent=2)+'\n').encode(),'reply-cases.json':(json.dumps(dict(context=context,caps=caps,cases=rc),indent=2)+'\n').encode()}
if __name__=='__main__':
 for name,data in generate().items():(ROOT/name).write_bytes(data)
