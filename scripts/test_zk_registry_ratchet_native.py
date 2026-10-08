"""Shared new-family package validator regressions (no external defect fixtures)."""

import copy
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import (
    package as P,
    ratchet_zokrates as K,
    ratchet_halo2 as H,
    ratchet as RT,
    ratchet_native as N,
    lean_emit as E,
)
import test_zk_registry_ratchet as Toy


def problem_fixture(root, family):
    """A synthetic OPEN native-family record reusing the toy reference model and DET fixture."""
    directory, old = Toy.offline_problem(root)
    ref = old['reference']
    model = {k: v for k, v in ref['model'].items() if k in ('file', 'module', 'namespace', 'sha256')}
    sha = 'ab' * 32
    counts = {k: old['record'][k] for k in ('nonlinear', 'linear', 'total')}
    common = {k: ref[k] for k in ('registry_package_id', 'repo', 'repo_url', 'commit', 'path', 'call')}
    common.update(lean_opts=E.LEAN_OPTIONS)
    env = {k: old['env'][k] for k in ('lean', 'mathlib', 'lake_manifest_sha256')}
    if family == 'zokrates':
        source = Path(directory, 'reference/main.zok')
        source.parent.mkdir()
        source.write_text('def main(field a, field b) -> field { return 2*a*b; }\n')
        common.update(
            model=dict(model, n_wires=6, counts=counts, r1cs_sha256=sha),
            source_dir='.',
            source=dict(file='reference/main.zok', sha256=P.sha256_file(str(source))),
            compiler=dict(version='0.8.8', style='brace', binary_sha256=sha, source='synthetic fixture'),
            io=dict(
                prime=str(Toy.PR),
                prime_name='bn128',
                input_names=['main.a', 'main.b'],
                output_names=['main.ret0'],
                input_types=['field', 'field'],
                output_types=['field'],
                n_pub_in=2,
                n_prv_in=0,
            ),
        )
        record = dict(counts, r1cs_sha256=sha, rung=0, source='fixture')
        framework = K
    else:
        layout = Path(directory, 'reference/layout.ir.json')
        layout.parent.mkdir()
        layout.write_text('{}\n')
        cost = dict(
            domain_rows=64,
            advice_columns=4,
            fixed_columns=2,
            gate_instances=10,
            copies=8,
            lookups=0,
            degree_ge={'2': 6},
            max_degree=2,
            variables=20,
            nodes=30,
        )
        cost['priced'] = H.priced(cost)
        common.update(
            model=dict(model, ir_sha256=P.sha256_file(str(layout)), n_wires=20, cost=cost),
            ir=dict(file='reference/layout.ir.json', sha256=P.sha256_file(str(layout)), model_sha256=sha),
            compiler=dict(version='0.3', binary_sha256=sha, source='fixture'),
            io=dict(
                prime=str(Toy.PR), prime_name='bn128', input_names=['a0r0'], output_names=['a1r0'], instance_columns=0
            ),
            target=dict(
                test='fixture_test',
                package='fixture',
                workspace='main',
                source='src/lib.rs',
                field='bn128',
                line='zcash-0.3',
            ),
            build=dict(
                toolchain='fixture',
                command=['test', '--offline', '--locked'],
                editable=['main/src/lib.rs'],
                protected=['harness/'],
                cargo_lock_sha256=sha,
                wrapper_sha256=sha,
                exporter_sha256=sha,
                rustc_sha256=sha,
                cargo_sha256=sha,
            ),
        )
        record = dict(cost, rung=0, source='fixture')
        framework = H
    p = dict(
        schema_version=framework.SCHEMA_VERSION,
        kind='ratchet',
        family=family,
        package_id=old['package_id'],
        status='OPEN',
        reference=common,
        record=record,
        metric=framework.METRIC,
        admissibility=['fixture'],
        statement_shape='relation equivalence',
        determinism=old['determinism'],
        det=old['det'],
        simulate=dict(seed='fixture', profiles='fixture'),
        snapshot=old['snapshot'],
        env=env,
        generator=dict(name=framework.GENERATOR_NAME, version='1.0', sources_sha256=sha),
        created_utc='2026-10-08T00:00:00Z',
    )
    P.write_json(str(Path(directory, 'problem.json')), p)
    return directory, p


class RegistryDispatch(unittest.TestCase):
    def test_both_full_open_package_schemas_and_tampering(self):
        """Both native schemas bind the DET model, native cost, generator and file paths."""
        for family in ('zokrates', 'halo2'):
            with tempfile.TemporaryDirectory() as root:
                directory, p = problem_fixture(root, family)
                self.assertEqual(P.validate_problem(p, directory), [])
                for mutation in ('det', 'record', 'schema', 'path'):
                    q = copy.deepcopy(p)
                    if mutation == 'det':
                        q['det']['model_sha256'] = 'cd' * 32
                    elif mutation == 'record':
                        q['record']['total' if family == 'zokrates' else 'priced'] += 1
                    elif mutation == 'schema':
                        q['generator']['name'] = 'boole-zk-registry-ratchet'
                    else:
                        q['reference']['model']['file'] = '../outside.lean'
                    self.assertTrue(P.validate_problem(q, directory), (family, mutation))

    def test_candidate_model_digest_and_interface_are_bound(self):
        """The candidate's checker import must match its recorded model digest."""
        with tempfile.TemporaryDirectory() as root:
            directory, p = problem_fixture(root, 'zokrates')
            candidate = dict(
                source_sha256='ab' * 32,
                r1cs_sha256='ab' * 32,
                raw_sha256='ab' * 32,
                model_sha256='ab' * 32,
                io=p['reference']['io'],
                **{k: p['record'][k] for k in ('nonlinear', 'linear', 'total')},
                smaller=False
            )
            model = '-- synthetic native candidate\n'
            candidate['model_sha256'] = RT.sha(model.encode())
            statement, files = N.write_candidate_package(p, directory, candidate, model, str(Path(root, 'candidate')))
            q = N.candidate_problem(p, candidate, files, statement, 'ab' * 32)
            self.assertEqual(P.validate_problem(q, str(Path(root, 'candidate'))), [])
            q['candidate']['model_sha256'] = 'cd' * 32
            self.assertTrue(P.validate_problem(q))

    def test_open_native_families_do_not_claim_a_checker_statement(self):
        """Only CANDIDATE native-family packages carry a proof-checker statement."""
        for version in (K.SCHEMA_VERSION, H.SCHEMA_VERSION):
            self.assertFalse(P.has_statement(dict(schema_version=version, status="OPEN")))
            self.assertTrue(P.has_statement(dict(schema_version=version, status="CANDIDATE")))

    def test_family_schemas_are_not_silently_base_det_schema(self):
        """Each native family dispatches to its dedicated ratchet schema."""
        for version in (K.SCHEMA_VERSION, H.SCHEMA_VERSION):
            self.assertEqual(P.load_schema(version)["$id"], version)


if __name__ == "__main__":
    unittest.main()
