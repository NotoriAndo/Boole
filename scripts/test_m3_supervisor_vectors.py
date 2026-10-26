"""Host-independent Rust/Python real-controller contract vectors."""
import copy,importlib.util,json,sys,tempfile,unittest
from pathlib import Path
from unittest.mock import Mock,patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
from generate_m3_supervisor_vectors import ROOT,generate
from m3_supervisor_vector_cases import checked_reply,auth_value
from zk_registry.m3_gate import codec as C,adapter as A,front as F,mapping as M

class Vectors(unittest.TestCase):
 def test_shared_bytes_are_reproducible(self):
  for name,raw in generate().items():self.assertEqual((ROOT/name).read_bytes(),raw,name)
 def test_descriptor_codec_accepts_exactly_the_shared_cases(self):
  cases=json.loads((ROOT/'descriptor-cases.json').read_text());self.assertEqual(len(cases),28)
  for case in cases:
   with self.subTest(case=case['name']):
    try:d=C.decode(bytes.fromhex(case['raw_hex']));ok=True
    except (ValueError,TypeError):ok=False
    self.assertEqual(ok,case['valid'])
    if ok:self.assertEqual(C.identity(d),case['identity'])
 def test_reply_rows_and_caller_package_allowances(self):
  d=C.decode((ROOT/'descriptor-v1.json').read_bytes());v=json.loads((ROOT/'reply-cases.json').read_text());self.assertEqual(len(v['cases']),71)
  for case in v['cases']:
   with self.subTest(case=case['name']):
    try:checked_reply(bytes.fromhex(case['raw_hex']),d,v['context'],v['caps'],fork=case['fork_authorized'],authenticated=auth_value(case,v['caps']['statement']));ok=True
    except (ValueError,TypeError):ok=False
    self.assertEqual(ok,case['valid'])
 def test_certified_own_caps_and_reference_identity_in_all_phases(self):
  v=json.loads((ROOT/'cap-cases.json').read_text());d=C.decode(bytes.fromhex(v['descriptor_hex']));self.assertEqual(len(v['cases']),12)
  for case in v['cases']:
   p=C.loads(bytes.fromhex(case['package_hex']))
   try:caps=A.published_caps(p,d,case['context']);row='accept'
   except F.Refusal as e:caps=dict(lean=0,nanoda=0);row=e.row
   self.assertEqual(caps,{k:case['caps'][k] for k in ('lean','nanoda')})
   r=checked_reply(bytes.fromhex(case['reply_hex']),d,case['context'],case['caps']);self.assertEqual(r['row'],row)
   self.assertEqual(r['fuel'],3 if row=='accept' else 0)
 def test_exact_first_failure_on_plain_and_typed_transport_in_all_phases(self):
  v=json.loads((ROOT/'reconstruction-cases.json').read_text());self.assertEqual(len(v['cases']),183)
  for case in v['cases']:
   with self.subTest(case=case['name'],phase=case['phase']):
    d=C.decode(bytes.fromhex(case['descriptor_hex']));status=case['decoder_status_hex']
    with tempfile.TemporaryDirectory() as tmp,patch.object(A,'verify_tools'):
     if case['fork_authorized']:
      try:authenticated=F.package(bytes.fromhex(case['package_hex']),case['context']['package_sha256'],F.Inventory(bytes.fromhex(case['inventory_hex']),d['artifact_table_sha256']))['statement']
      except F.Refusal as e:
       if e.row!='package_invalid':raise
       authenticated=M.NO_VALID_STATEMENT
      statement=M.quarantine_statement(case['context'],authenticated,fork_authorized=True)
      r=A.reply(d,case['context'],M.classify(M.Observation('quarantine_void',fork_authorized=True),d,case['phase']),statement,M.not_run(),0,fork_authorized=True,authenticated_statement=authenticated)
     else:
      r=A.run(None if case['bundle_hex'] is None else bytes.fromhex(case['bundle_hex']),bytes.fromhex(case['package_hex']),bytes.fromhex(case['inventory_hex']),case['context'],C.encode(d),C.identity(d),None,None,None,Path(tmp)/'work',phase=case['phase'],public_counter=case['public_counter'],decoder_status=None if status is None else bytes.fromhex(status),decoder_pin=case['decoder_pin'] if status is not None else None,transport_digest=case['transport_digest'] if status is not None else None)
    self.assertEqual(C.canonical(r).hex(),case['reply_hex'])

 def test_open_layout_models_and_rejected_wire_boundaries(self):
  v=json.loads((ROOT/'open-layout-cases.json').read_text())['cases']
  self.assertEqual(len(v),56);self.assertEqual(sum(r['accepted'] for r in v),28)
  self.assertEqual({len(bytes.fromhex(r['model_hex'])) for r in v if r['accepted']},{0,1,40,64})
  for r in v:
   if not r['accepted']:continue
   d=C.decode(bytes.fromhex(r['descriptor_hex']))
   with tempfile.TemporaryDirectory() as tmp,patch.object(A,'verify_tools'):
    reply=A.run(bytes.fromhex(r['bundle_hex']),bytes.fromhex(r['package_hex']),bytes.fromhex(r['inventory_hex']),r['context'],C.encode(d),C.identity(d),None,None,None,Path(tmp)/'work',public_counter=True)
   self.assertEqual(C.canonical(reply).hex(),r['reply_hex'],r['name'])

class TypedDecoder(unittest.TestCase):
 def test_typed_api_distinguishes_data_and_host_faults_without_exit_or_stderr(self):
  path=Path(__file__).resolve().parents[1]/'crates/boole-gate-supervisor/src/real_decoder.py'
  spec=importlib.util.spec_from_file_location('private_real_decoder',path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
  library=Mock();library.ZSTD_decompress.return_value=4;library.ZSTD_isError.return_value=0
  with tempfile.TemporaryDirectory() as tmp:
   source=Path(tmp)/'frame';source.write_bytes(b'bound frame');dest=Path(tmp)/'output'
   with patch.object(m.ctypes,'CDLL',return_value=library):
    self.assertEqual(m.decode('pinned',source,dest,4)[:2],('ok','none'))
    # The identical successful typed decode encounters EEXIST during publication.
    self.assertEqual(m.decode('pinned',source,dest,4),('unavailable','write',None,None))
    dest.unlink()
    self.assertEqual(m.decode('pinned',source,dest,4,'f'*64),('invalid-data','digest',None,None));self.assertFalse(dest.exists())
    self.assertEqual(m.decode('pinned',source,dest,4,C.sha(b'\0'*4))[:2],('ok','none'));dest.unlink()
    library.ZSTD_isError.return_value=1;library.ZSTD_getErrorCode.return_value=22
    self.assertEqual(m.decode('pinned',source,dest,4),('invalid-data','checksum',None,None));self.assertFalse(dest.exists())
   self.assertEqual(m.decode('pinned',source.with_name('missing'),dest,4),('unavailable','read',None,None))
   with patch.object(m.ctypes,'CDLL',side_effect=OSError('host failure')):self.assertEqual(m.decode('pinned',source,dest,4),('unavailable','tool',None,None))
class FrontPreparation(unittest.TestCase):
 def setUp(self):
  from test_zk_registry_m3_gate import Adapter
  self.fixture=Adapter('test_checked_own_set_comes_from_authenticated_inventory');self.fixture.setUp();self.addCleanup(self.fixture.doCleanups)
  path=Path(__file__).resolve().parents[1]/'crates/boole-gate-supervisor/src/real_front.py'
  spec=importlib.util.spec_from_file_location('private_real_front',path);self.front=importlib.util.module_from_spec(spec);spec.loader.exec_module(self.front)
  self.root=Path(self.fixture.tmp.name)/'controller';self.root.mkdir()
 def request(self,name,mode,bundle=None):
  f=self.fixture;root=self.root/name;root.mkdir();bundle=f.bundle if bundle is None else bundle
  c=dict(f.context,export_digest=C.sha(bundle))
  for n,raw in [('descriptor',C.encode(f.desc)),('context',C.canonical(c)),('package',C.canonical(f.pkg)),('inventory',f.inventory.raw),('bundle',bundle)]: (root/n).write_bytes(raw)
  return dict(descriptor=str(root/'descriptor'),gate=C.identity(f.desc),context=str(root/'context'),package=str(root/'package'),inventory=str(root/'inventory'),bundle=str(root/'bundle'),reference=str(f.path) if mode=='state' else str(root/'absent-reference'),tools_root='synthetic',counter_driver='synthetic',work=str(root/'work'),mode=mode,public_counter=False)
 def invoke(self,request):
  with patch.object(A,'verify_tools'),patch.object(A.D.Tools,'load',return_value=object()),patch.object(A.FC,'load_authority',return_value=self.fixture.authority):
   return self.front.prepare(request)
 def test_private_phases_request_artifacts_only_at_A1_boundaries(self):
  before=(A.check_reference,A.FC.bind,A.compare_exports,A.CI.one)
  first=self.invoke(self.request('first','reference'));self.assertEqual((first['need'],first['reply']),('reference',None))
  second=self.invoke(self.request('second','state'));self.assertEqual((second['need'],second['reply']),('state',None))
  self.assertEqual((A.check_reference,A.FC.bind,A.compare_exports,A.CI.one),before)
 def test_prior_submission_faults_precede_missing_reference_and_state(self):
  first=self.invoke(self.request('bad-export','reference',F.pack(b'bad NDJSON')))
  self.assertEqual(first['need'],'reply');self.assertEqual(C.loads(bytes.fromhex(first['reply']))['row'],'front_invalid')
  with patch.object(A,'axioms',side_effect=F.Refusal('axiom_rejected')):
   second=self.invoke(self.request('bad-axioms','state'))
  self.assertEqual(second['need'],'reply');self.assertEqual(C.loads(bytes.fromhex(second['reply']))['row'],'axiom_rejected')
if __name__=='__main__':unittest.main()
