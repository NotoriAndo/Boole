"""Generate counter inputs solely from these public templates and pinned Init.

No reference proof, catalog, local-docs export, receipt or previous golden is an
input. Source bytes are preregistered before Lean runs; exported bytes are sealed
in manifest.json before either counter runs. This is architecture identity
coverage, not evidence of real-workload time prediction or package eligibility.
"""
from pathlib import Path
import argparse
import hashlib
import io
import json
import os
import subprocess
import tarfile
from .build_fuel_tools import digest
from .closure import PRIMITIVES, QUOTIENTS
from .counter_identity import canonical

LEAN = '5045d0056413266e57c625dcd7c365b10e377c52'
EXPORT = '076e8e57707e813375e8f9da8bf989799ace9680'


def sources():
    """Public, parameterized core-library programs; no persisted proof shapes."""
    programs = {
        'identity': 'theorem target : True := (fun (x : True) => x) True.intro',
        'universe': 'universe u\ntheorem target {α : Sort u} (x : α) : (fun y => y) x = x := rfl',
        'pair': 'theorem target : (Prod.mk (VALUE : Nat) true).1 = VALUE := rfl',
        'and': 'theorem target (p q : Prop) (h : p ∧ q) : q ∧ p := And.intro h.right h.left',
        'or': 'theorem target (p q : Prop) (h : p ∨ q) : q ∨ p := Or.elim h Or.inr Or.inl',
        'nat-rec': 'theorem target : Nat.rec 0 (fun _ x => x + 1) VALUE = VALUE := rfl',
        'nat-theorem': 'theorem target (n : Nat) : n + VALUE = VALUE + n := Nat.add_comm n VALUE',
        'list': 'theorem target (a b c : List Nat) : (a ++ b) ++ c = a ++ (b ++ c) := List.append_assoc a b c',
        'quotient': 'theorem target (r : Nat → Nat → Prop) (x y : Nat) (h : r x y) : Quot.mk r x = Quot.mk r y := Quot.sound h',
        'funext': 'theorem target (f : Nat → Nat) : (fun x => f x) = f := funext (fun _ => rfl)',
        'inductive': 'inductive PublicTree where\n | leaf : Nat → PublicTree\n | node : PublicTree → PublicTree → PublicTree\ntheorem target : PublicTree.node (.leaf VALUE) (.leaf 0) = PublicTree.node (.leaf VALUE) (.leaf 0) := rfl',
        'literal': 'theorem target : (LARGE : Nat) = LARGE := rfl',
        'string': 'theorem target : "TEXT" = "TEXT" := rfl',
        'primitive': 'theorem target : (VALUE : Nat) * 7 + 3 = PRODUCT := rfl',
        'bitwise': 'theorem target : Nat.land VALUE VALUE = VALUE := rfl',
        'nested': 'def helper (x : Nat) := NESTED\ntheorem target : helper VALUE = VALUE := rfl',
    }
    result = []
    for family, template in programs.items():
        for size in (8, 32):
            text = template.replace('LARGE', str(2**(size*16)+1)).replace('PRODUCT', str(size*7+3))
            text = text.replace('VALUE', str(size)).replace('TEXT', 'λ-public-'*size)
            text = text.replace('NESTED', '(fun y => y) ('*size + 'x' + ')'*size)
            text = 'import Init\nnamespace PublicCounter\n'+text+'\nend PublicCounter\n'
            result.append(dict(family=family, size=size, source=text))
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',required=True,type=Path);p.add_argument('--prefix',required=True,type=Path)
    p.add_argument('--exporter',required=True,type=Path);a=p.parse_args()
    a.out.mkdir(parents=True)
    work=a.out/'generation';work.mkdir();fixture=a.out/'fixture';fixture.mkdir()
    programs=sources()
    version=subprocess.check_output([str(a.prefix/'bin/lean'),'--version'],text=True)
    if '4.34.1' not in version or LEAN[:12] not in version:
        raise ValueError('pinned public Lean source build required')
    prereg=dict(version=1,scope='public-generated-Init/v1',generator_sha256=digest(Path(__file__)),
        lean_source_commit=LEAN,export_source_commit=EXPORT,
        generation_tools=dict(lean_sha256=digest(a.prefix/'bin/lean'),exporter_sha256=digest(a.exporter),lean_version=version.strip()),
        sources=[dict(index=i, family=r['family'], size=r['size'], utf8_sha256=hashlib.sha256(r['source'].encode()).hexdigest()) for i,r in enumerate(programs)])
    prereg_path=fixture/'PREREGISTRATION.json';prereg_path.write_bytes(canonical(prereg))
    # Verify the preregistration is unchanged before every compilation and export.
    prereg_hash=digest(prereg_path)
    units=[];tarpath=work/'corpus.tar'
    env=dict(os.environ,LEAN_SYSROOT=str(a.prefix.resolve()),LEAN_STACK_SIZE_KB='262144',LEAN_NAT_MAX_SIZE='4096',RUSTUP_AUTO_INSTALL='0',CARGO_NET_OFFLINE='true')
    with tarfile.open(tarpath,'w',format=tarfile.USTAR_FORMAT) as archive:
        for i,row in enumerate(programs):
            assert digest(prereg_path)==prereg_hash
            directory=work/str(i);directory.mkdir();source=directory/'PublicCounter.lean';source.write_text(row['source'])
            assert digest(source)==prereg['sources'][i]['utf8_sha256']
            taskenv=dict(env,LEAN_PATH=str(a.prefix/'lib/lean')+':'+str(directory))
            result=subprocess.run([str(a.prefix/'bin/lean'),'-o',str(directory/'PublicCounter.olean'),str(source)],env=taskenv,cwd=directory,capture_output=True,timeout=120)
            (directory/'compile.stdout').write_bytes(result.stdout);(directory/'compile.stderr').write_bytes(result.stderr)
            if result.returncode:raise ValueError((i,row['family'],result.stdout,result.stderr))
            result=subprocess.run([str(a.exporter),'PublicCounter','--','PublicCounter.target',*PRIMITIVES,*QUOTIENTS,'Classical.choice','propext','Quot.sound'],env=taskenv,cwd=directory,capture_output=True,timeout=120)
            (directory/'export.stderr').write_bytes(result.stderr)
            if result.returncode:raise ValueError((i,row['family'],result.stderr))
            raw=result.stdout
            metadata=json.loads(raw.splitlines()[0])['meta']
            if metadata['lean']['githash'] != LEAN or metadata['format']['version'] != '3.1.0':
                raise ValueError('pinned public export metadata mismatch')
            name=f'public-{i:02d}.ndjson';member=tarfile.TarInfo(name);member.size=len(raw);member.mode=0o444;member.mtime=0
            archive.addfile(member,io.BytesIO(raw))
            units.append(dict(source_index=i,family=row['family'],size=row['size'],member=name,raw_bytes=len(raw),raw_sha256=hashlib.sha256(raw).hexdigest()))
            print(json.dumps(dict(exported=i+1,total=len(programs),family=row['family'],bytes=len(raw))),flush=True)
    compressed=work/'corpus.tar.zst'
    subprocess.run(['zstd','-q','-19','-T1',str(tarpath),'-o',str(compressed)],check=True,timeout=180)
    parts=[]
    with compressed.open('rb') as source:
        for index in range(1000):
            block=source.read(8*2**20)
            if not block:break
            name=f'public-corpus-{index:03d}.tar.zst.part';path=fixture/name;path.write_bytes(block)
            parts.append(dict(path=name,bytes=len(block),sha256=hashlib.sha256(block).hexdigest()))
    manifest=dict(version=2,scope='public-generated-Init/v1',lean_source_commit=LEAN,export_source_commit=EXPORT,
        generator='scripts/zk_registry/generate_public_counter_corpus.py',generator_sha256=digest(Path(__file__)),preregistration_sha256=prereg_hash,
        counts={family:sum(r['family']==family for r in programs) for family in sorted({r['family'] for r in programs})},
        units=units,parts=parts,compressed_bytes=compressed.stat().st_size,compressed_sha256=digest(compressed),
        provenance='Only committed public generator templates and public pinned Lean Init. No local-docs data inputs.')
    (fixture/'manifest.json').write_bytes(canonical(manifest))
    print('SEALED public source and export bytes; counters have not run',flush=True)


if __name__=='__main__':main()
