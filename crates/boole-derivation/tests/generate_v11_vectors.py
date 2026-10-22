"""Independent byte formulas for fixed M2 event and tie vectors (stdlib only)."""
from pathlib import Path
import hashlib,json
H=lambda b:hashlib.sha256(b).digest()
u64=lambda x:x.to_bytes(8,'big');u32=lambda x:x.to_bytes(4,'big');u16=lambda x:x.to_bytes(2,'big')
chain=u64(720926)
root=H(b'\0BOOLE-M2/POOL-LEAF/v2'+chain+b'\2\0\0'+u32(len(b'generator-v1'))+H(b'generator-v1')+bytes([9])*32)
mix=H(u64(4)+b'randao')
seed=H(b'BOOLE-M2/DRAW/v2'+chain+b'\0'+u32(1)+mix)
p=H(b'BOOLE-M2/PROBLEM/v2'+chain+root+u32(0)+seed+u32(0))
leaf=H(b'\0BOOLE-M2/PART/v1'+bytes([6])*20+bytes([7])*32+u16(0)+H(b'*'))
b=b'\1'+p+bytes([6])*20+bytes([7])*32+b'\1'+H(b'EENV2-canonical-closure-v2')+H(b'BOOLE-DEVNET/BASE/v1')+b'\0\1\1'+u64(1)+H(b'*')+u32(1)+u16(1)+leaf+bytes(4)+u16(40)+bytes([1,0,1,0])+bytes(36)
assert len(b)==277
ch=H(b'BOOLE-M2/COMMIT/v2'+chain+p+H(b'BOOLE-M2/CANCEL/v1'+bytes([8])*32)+H(b'BOOLE-M2/BODY/v1'+b))
slot=lambda code,pid=bytes(32): b'\3\0'+u32(1)+bytes([code])+u32(1)+u32(0)+pid
hashes={x:H(x.encode()) for x in ['a','b','c']}
order=lambda mix:sorted(hashes,key=lambda x:H(b'BOOLE-M2/TIE/v1'+hashes[x]+mix))
result=dict(schema='boole-devnet-m2-event-golden-v1.1',problem_id=p.hex(),commit_hash=ch.hex(),events=dict(draw=slot(0xc1).hex(),withheld=slot(0xc2).hex(),deadline=slot(0xc3,p).hex(),expiry=(b'\2'+p+ch+b'\4').hex(),prune=(b'\4'+p+u32(1)).hex()),tie=dict(commit_height=6,next_mix=(bytes([0x42])*32).hex(),expected_order=order(bytes([0x42])*32),commit_mix_order=order(H(u64(6)+b'randao'))))
repo=Path(__file__).resolve().parents[3]
provenance=dict(generator='crates/boole-derivation/tests/generate_v11_vectors.py',generator_sha256=H(Path(__file__).read_bytes()).hex(),method='independent stdlib byte formulas; configured devnet reset')
result['draw_seed']=seed.hex()
result['union_draws']=[]
for desired,pair in [(2,[1,5]),(3,[3,1])]:
 for i in range(10000):
  m=H(u64(i));seed_u=H(b'BOOLE-M2/DRAW/v2'+chain+b'\0'+u32(1)+m)
  if int.from_bytes(seed_u[:8],'big')%4==desired:
   result['union_draws'].append(dict(mix=m.hex(),seed=seed_u.hex(),j=desired,pair=pair));break
result['provenance']=provenance
out=repo/'fixtures/devnet-m2'
(out/'event-golden-v1.1.json').write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
# Canonical config JSON field order is normative and independently encoded here.
pkgs=sorted([H(b'BOOLE-DEVNET/EVM/v1'),H(b'BOOLE-DEVNET/RISCV/v1')])
arr=lambda b:list(b)
row=lambda lane,sd,gap,grace:dict(lane=lane,solve_deadline=sd,announce_gap=gap,reveal_grace=grace)
c=dict(chain_id=720926,d=9,w=64,w_slack_min=16,p_blk=6,ratio_max=16,k_max=4,c_commit=937,q_max=8,v_per_deposit=128,verdict_gas=1000000,l1_gas_limit=30000000,pool_limit=131072,slot_table=[row(2,48,4,4),row(1,96,8,4)],statement_raw_max=1048576,slot_leaves_max=1048576,pool_lead=2,pool_max=65536,statement_max=122880,trace_embargo_max=2628000,schema_id=arr(H(b'EENV2-canonical-closure-v2')),base_root=arr(H(b'BOOLE-DEVNET/BASE/v1')),pkg_table=[arr(p) for p in pkgs],env_allowed=[[],[arr(pkgs[0])],[arr(pkgs[1])]],incompatible=[[arr(pkgs[0]),arr(pkgs[1])]],terms_table={'1':arr(H(b'Option A placeholder: proofs CC0-1.0; granted trace CC-BY-4.0'))},pool_key=list(bytes.fromhex('1a642f0e3c3af545e7acbd38b07251b3990914f1')))
profiles=[]
for name in ['devnet','testnet','production']:
 if name=='testnet':c.update(chain_id=720927,d=96,w=300,w_slack_min=64,c_commit=1875,v_per_deposit=256,l1_gas_limit=60000000,pool_lead=7200,slot_table=[row(2,1800,300,25),row(1,7200,3600,25),row(2,1800,300,25)])
 if name=='production':c.update(chain_id=720928,pool_lead=50400)
 encoded=json.dumps(c,separators=(',',':')).encode();slots=bytes([len(c['slot_table'])])+b''.join(bytes([s['lane']])+u64(s['solve_deadline'])+u64(s['announce_gap'])+u64(s['reveal_grace']) for s in c['slot_table'])
 profiles.append(dict(name=name,config=json.loads(encoded),canonical_json_hex=encoded.hex(),slot_encoding_hex=slots.hex(),slot_table_digest=H(b'BOOLE-M2/SLOTS/v1'+slots).hex(),config_digest=H(b'BOOLE-M2/CONFIG/v1'+encoded).hex()))
e=b''.join(b'\1'+u64(tx)+u64(log)+bytes([kind,code]) for tx,log,kind,code in [(1,2,2,1),(2,4,5,63)])
for p in profiles:
 p['root_height1_probe']=H(b'BOOLE-M2/ROOT/v2'+bytes.fromhex(p['config_digest'])+u64(1)+H(u64(1))+H(e)).hex()
 p['probe_event_hex']=e.hex()
(out/'config-golden-v1.1.json').write_text(json.dumps(dict(schema='boole-devnet-m2-config-v1.1',provenance=provenance,profiles=profiles),indent=2,sort_keys=True)+'\n')
import os,subprocess,tempfile
binary=Path(os.environ['BOOLE_DEVNET_ZSTD']);pin=json.loads((out/'zstd-pin-v1.json').read_text());assert H(binary.read_bytes()).hex()==pin['sha256']
raw=b'a'*1048576
(repo/'target').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='m2-v11-package-',dir=repo/'target') as scratch:
 input_path=Path(scratch)/'package.raw';input_path.write_bytes(raw)
 carried=subprocess.check_output([str(binary),'-q','-1','--check','--no-dictID','--stdout',str(input_path)])
 compressed=Path(scratch)/'package.zst';compressed.write_bytes(carried)
 listed=subprocess.check_output([str(binary),'--list','--verbose',str(compressed)],text=True)
 assert any('Frames' in line and line.split()[-1]=='1' for line in listed.splitlines())
assert carried[:4]==bytes.fromhex('28b52ffd')
d=carried[4];assert d&3==0 and d&4 and d&0x18==0
single=bool(d&32);at=5
if single:window=None
else:
 w=carried[at];at+=1;base=1<<(10+(w>>3));window=base+(base//8)*(w&7)
fcs_bytes=[int(single),2,4,8][d>>6];assert fcs_bytes,'R32 requires Frame_Content_Size'
content=int.from_bytes(carried[at:at+fcs_bytes],'little')+(256 if fcs_bytes==2 else 0)
assert content==len(raw) and (window or content)<=content
frame=dict(content_size=content,window=window or content,checksum=True,no_dictionary=True,frames=1)
assert subprocess.check_output([str(binary),'-q','-d','--stdout'],input=carried)==raw
(out/'package-codec-v1.1.json').write_text(json.dumps(dict(schema='boole-devnet-m2-package-v1.1',provenance={**provenance,'codec_binary':pin},codec=1,frame=frame,raw_len=len(raw),raw_sha256=H(raw).hex(),carried_hex=carried.hex()),indent=2,sort_keys=True)+'\n')
print('generated event/config/package v1.1 vectors')
