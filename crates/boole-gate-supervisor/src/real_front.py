"""Private controller preparation phases over the unchanged pinned A1 adapter.
Fixed guards stop immediately before reference/state access or native execution.
A requested artifact is not a verdict; final execution uses the original adapter.
"""
import json,sys
from pathlib import Path
from zk_registry.m3_gate import adapter as A,codec as C,front as F,decoder as DC

def prepare(request):
    raw=Path(request['descriptor']).read_bytes();d=C.decode(raw,request['gate'])
    c=C.loads(Path(request['context']).read_bytes());A.context_check(c)
    mode=request['mode']
    if mode not in ('reference','state'):raise ValueError('private preparation mode')
    requested=[]
    def stop_at(kind):
        def guard(*args,**kwargs):
            requested.append(kind)
            raise F.Refusal('node_fault','private materialization boundary')
        return guard
    old_reference=A.check_reference;old_bind=A.FC.bind;old_compare=A.compare_exports;old_counter=A.CI.one
    if mode=='reference':A.check_reference=stop_at('reference')
    A.FC.bind=stop_at('state');A.compare_exports=stop_at('ready');A.CI.one=stop_at('ready')
    try:
        decoded=request.get('decoder_status')
        reply=A.run(F.BundleSource(request['bundle']),F.PackageSource(request['package']),Path(request['inventory']).read_bytes(),c,raw,request['gate'],request['reference'],A.D.Tools.load(request['tools_root']),request['counter_driver'],request['work'],public_counter=request['public_counter'],decoder_status=DC.Source(decoded) if decoded else None,decoder_pin=request.get('decoder_pin'),transport_digest=request.get('transport_digest'))
    finally:
        A.check_reference=old_reference;A.FC.bind=old_bind;A.compare_exports=old_compare;A.CI.one=old_counter
    if len(requested)>1:raise ValueError('multiple preparation boundaries')
    if requested:
        need=requested[0];result=None
        if mode=='reference' and need!='reference':raise ValueError('reference boundary skipped')
    else:
        if reply['outcome']=='ACCEPT' or reply['counter_identity'] is not None or any(v['state']!='not-run' for v in reply['kernels'].values()):
            raise ValueError('preparation must not execute kernels')
        need='reply';result=C.canonical(reply).hex()
    return dict(version=1,context_sha256=C.sha(C.canonical(c)),gate_rule_digest=C.identity(d),descriptor_sha256=C.sha(raw),mode=mode,public_counter=request['public_counter'],need=need,reply=result)

if __name__=='__main__':
    request=json.loads(Path(sys.argv[1]).read_bytes())
    sys.stdout.buffer.write(C.canonical(prepare(request)))
