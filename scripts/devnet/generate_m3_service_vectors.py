"""Offline real Registry API checkpoint for M3's candidate VOID parity."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'scripts'))
from zk_registry import service as S


def sha(data):
    return hashlib.sha256(data).hexdigest()


def generate():
    with tempfile.TemporaryDirectory(prefix='boole-m3-service-') as temporary:
        directory = Path(temporary)
        package, submission = directory/'package', directory/'submission'
        package.mkdir()
        submission.mkdir()
        (package/'problem.json').write_text('{}\n')
        (submission/'proof.txt').write_text('deterministic M3 parity submission\n')
        now, result = [100], ['ERROR']
        context = dict(host='host-a', worker='worker-a', profile='production',
                       canary=True, envelope='standard', stage='kernel')

        def verifier(*_):
            if result[0] == 'PASS':
                return dict(verdict='PASS', reason='injected-pass')
            return dict(verdict='ERROR', reason='silent-exit',
                        attempt=dict(context, resource_events=[]))

        old_gate, fixed_gate = '11'*32, '22'*32
        registry = S.Registry(str(directory/'registry'), verifier, gate=old_gate,
                              gates={fixed_gate: verifier}, clock=lambda: now[0])
        registry.root.mkdir()
        manifest = registry._capture(str(package), package=True)
        payload = dict(entries=[dict(id='problem', kind='C', status='OPEN',
                                     submittable=True, root_id='problem')],
                       packages={'problem': manifest}, excluded={}, reveal_window=65,
                       challenge_window=1, gate=old_gate,
                       # This vector is the frozen version1 policy contract.
                       error_policy=dict(version=1, rules_sha256=sha(S.E.LEGACY_POLICY_PATH.read_bytes())))
        registry._append('catalog', payload, 0, [])
        rows = []

        def checkpoint(phase):
            state = registry.status()
            rows.append(dict(phase=phase, statuses={seq: c['status'] for seq, c in state['commits'].items()},
                             winner=state['problems']['problem'].get('winner')))

        for seq, height in [(1, 6), (2, 7)]:
            commitment = S.commitment('problem', f'solver-{seq}', S.submission_digest(str(submission)), f'salt-{seq}')
            assert registry.commit('problem', f'solver-{seq}', commitment, height)['sequence'] == seq
        registry.reveal(1, 'problem', 'solver-1', str(submission), 'salt-1', 8)
        result[0] = 'PASS'
        registry.reveal(2, 'problem', 'solver-2', str(submission), 'salt-2', 9)
        checkpoint('earlier-checking-later-passed')
        result[0] = 'ERROR'
        now[0] = 200
        context.update(host='host-b', worker='worker-b')
        registry.recheck(1, 10, attempt=dict(context))
        checkpoint('quarantined')
        now[0] = 4000
        context.update(host='host-c', worker='worker-c')
        registry.recheck(1, 11, gate=fixed_gate, attempt=dict(context))
        checkpoint('fixed-gate-unresolved')
        assert registry.void(1, 12)['outcome'] == 'VOID'
        checkpoint('void-releases-later-winner')
        return dict(schema='boole-devnet-m3-service-void-parity-v1.3-draft',
                    provenance=dict(source='scripts/zk_registry/service.py',
                                    source_sha256=sha((ROOT/'scripts/zk_registry/service.py').read_bytes()),
                                    policy_sha256=sha(S.E.LEGACY_POLICY_PATH.read_bytes()),
                                    generator='scripts/devnet/generate_m3_service_vectors.py',
                                    generator_sha256=sha(Path(__file__).read_bytes()),
                                    execution='Registry.commit/reveal/recheck/void; synthetic catalog and injected verifier; fixed integer clocks',
                                    event_log_sha256=sha(registry.log.read_bytes())),
                    rows=rows)


encoded = json.dumps(generate(), indent=2, sort_keys=True)+'\n'
target = ROOT/'fixtures/devnet-m3/service-void-v1.3.json'
if sys.argv[1:] == ['--check']:
    assert target.read_text() == encoded, 'service VOID parity vector differs'
elif not sys.argv[1:]:
    target.write_text(encoded)
else:
    raise SystemExit('usage: generate_m3_service_vectors.py [--check]')
