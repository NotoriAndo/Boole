#!/usr/bin/env python3
"""Stage-specific acceptance v2 controls; fast controls need no installed toolchain."""
from __future__ import annotations

import sys
import os
import json
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import check as C  # noqa: E402
from zk_registry import lean_runner as L  # noqa: E402
from zk_registry import package as P  # noqa: E402


class ForbiddenV2Tests(unittest.TestCase):
    def test_elaboration_commands_are_rejected_by_the_text_stage(self):
        cases = {
            '#eval 1': '#eval',
            'run_cmd pure ()': 'run_cmd',
            'run_elab pure ()': 'run_elab',
            'run_meta pure ()': 'run_meta',
            'initialize value : Nat := pure 1': 'initialize',
            'builtin_initialize value : Nat := pure 1': 'builtin_initialize',
            '@[custom_handler] theorem helper : True := trivial': 'forbidden-attribute',
            'exact Lean.trustCompiler': 'Lean.trustCompiler',
        }
        for source, reason in cases.items():
            with self.subTest(reason=reason):
                reasons = C.forbidden_scan([('helper declarations', source)])
                self.assertTrue(any(hit.startswith(reason + ' in ') for hit in reasons), reasons)

    def test_safe_attributes_and_scopes_are_allowed(self):
        attributes = ('simp', 'reducible', 'irreducible', 'semireducible', 'instance', 'ext',
                      'norm_cast', 'inline', 'specialize')
        for attr in attributes:
            for scope in ('', 'local ', 'scoped '):
                for source in (f'@[{scope}{attr}] theorem helper : True := trivial',
                               f'attribute [{scope}{attr}] helper'):
                    with self.subTest(source=source):
                        self.assertEqual(C.forbidden_scan([('helper declarations', source)]), [])
        self.assertEqual(C.forbidden_scan([('helper declarations',
                                           '@[simp 100, ext] theorem helper : True := trivial\n'
                                           'attribute [-simp] helper')]), [])

    def test_unknown_mixed_and_nested_attributes_fail_closed(self):
        sources = ('@[simp, custom_handler]', 'attribute [local simp, scoped custom_handler] helper',
                   '@[simp (custom_handler), custom_handler]', '@[Lean.simp]', '@[«simp»]',
                   '@[simp', 'attribute [custom_handler] helper', '@[]', '@[simpΩ]', '@[simp_custom]')
        for source in sources:
            with self.subTest(source=source):
                hits = C.forbidden_scan([('helper declarations', source)])
                self.assertTrue(any(hit.startswith('forbidden-attribute in ') for hit in hits), hits)

    def test_code_installing_commands_are_rejected(self):
        names = ('elab', 'elab_rules', 'simproc', 'dsimproc', 'simproc_decl', 'dsimproc_decl',
                 'macro', 'macro_rules', 'syntax', 'run_tac', 'builtin_simproc', 'builtin_dsimproc',
                 'builtin_simproc_decl', 'builtin_dsimproc_decl', 'declare_syntax_cat', 'register_simp_attr',
                 'register_builtin_option', 'register_tactic_tag', 'register_error_explanation',
                 'register_custom_handler', 'declare_custom_elaborator', 'register_ξ')
        for name in names:
            with self.subTest(command=name):
                self.assertTrue(C.forbidden_scan([('helper declarations', name + ' anything')]))
        self.assertEqual(C.forbidden_scan([('helper declarations',
                                           '-- elab and @[bad]\n/- register_simp_attr x -/\n'
                                           'def text := "@[bad] simproc"')]), [])


class EnvironmentV2Tests(unittest.TestCase):
    def test_old_environment_is_retired_by_the_environment_stage(self):
        env = L.LeanEnv('/not-used', [], 'v4.33.1', {}, 'ab' * 32, '/not-used')
        self.assertEqual(L.production_environment_reason(env), 'environment-retired')

    def test_retired_package_has_a_named_error_before_compilation(self):
        from test_zk_registry_check import fake_env, submission
        from test_zk_registry_package import make_package

        with tempfile.TemporaryDirectory() as directory:
            package, _ = make_package(directory)
            statement = Path(package, 'Statement.lean').read_text()
            proof = Path(directory, 'Solution.lean')
            proof.write_text(submission(statement, '  trivial\n'))
            result = C.check(package, str(proof), fake_env(directory))
        self.assertEqual((result['verdict'], result['reason']), ('ERROR', 'environment-retired'))
        self.assertEqual(result['compile'], 'skipped')


class ForbiddenVersionTests(unittest.TestCase):
    def test_historical_correctness_packages_keep_their_recorded_v1_set(self):
        from test_zk_registry_correctness_gnark import package_fixture

        problem = package_fixture()
        problem['checker'].pop('forbidden_version', None)
        problem['checker']['forbidden_tokens'] = list(C.FORBIDDEN_LABELS_V1)
        self.assertEqual(P.validate_problem(problem), [])

    def test_previous_v2_correctness_metadata_remains_readable(self):
        from test_zk_registry_correctness_gnark import package_fixture

        problem = package_fixture()
        problem['checker']['forbidden_version'] = 2
        problem['checker']['forbidden_tokens'] = list(C.FORBIDDEN_LABELS_V2)
        self.assertEqual(P.validate_problem(problem), [])


class SandboxV2Tests(unittest.TestCase):
    def test_production_without_the_pinned_sandbox_is_unavailable(self):
        from zk_registry.sandbox import Sandbox, SandboxUnavailable

        with self.assertRaisesRegex(SandboxUnavailable, '^sandbox-unavailable$'):
            Sandbox.production(landrun_path=None, system='Linux')

    def test_fixture_policy_cannot_acquire_production_assurance(self):
        from test_zk_registry_check import fake_env
        from test_zk_registry_package import make_package
        from zk_registry.sandbox import Sandbox

        with tempfile.TemporaryDirectory() as directory:
            package, _ = make_package(directory)
            env = fake_env(directory)
            result = C.check(package, str(Path(package, 'Statement.lean')), env,
                             policy_fixture=env.pins(), sandbox=Sandbox('production', 'not-executed'))
            self.assertEqual(result['assurance'], 'test-fixture')
            self.assertEqual(result['compile'], 'skipped')


class N2StageTests(unittest.TestCase):
    def fixture(self):
        row = dict(name='Semantics.oracle', type_sha256='ab' * 32, module='Semantics',
                   module_olean_sha256='cd' * 32, measuring_stick='fixture-stick')
        actual = {row['name']: dict(row, declared_in_module=True)}
        passed = {'measuring_sticks': {'fixture-stick': dict(status='PASS', modules={'Semantics': 'cd' * 32})}}
        return row, actual, passed

    def test_bound_fixture_axiom_requires_conformance_pass(self):
        from zk_registry import axiom_policy as A

        row, actual, passed = self.fixture()
        rejected = A.validate([row], actual)
        self.assertEqual((rejected['verdict'], rejected['reason']), ('INVALID', 'conformance-required'))
        accepted = A.validate([row], actual, conformance_fixture=passed)
        self.assertEqual(accepted['verdict'], 'PASS')
        self.assertIn(row['name'], accepted['allowed_axioms'])
        self.assertEqual(accepted['assurance'], 'test-fixture')

    def test_name_type_and_declaring_module_are_all_bound(self):
        from zk_registry import axiom_policy as A

        row, actual, passed = self.fixture()
        for key in ('type_sha256', 'module_olean_sha256', 'module', 'declared_in_module'):
            value = False if key == 'declared_in_module' else 'ef' * 32
            modified = {row['name']: dict(actual[row['name']], **{key: value})}
            self.assertEqual(A.validate([row], modified, conformance_fixture=passed)['reason'], 'n2-binding-mismatch')

    def test_n1_is_not_rescued_by_a_fixture_pass(self):
        from zk_registry import axiom_policy as A

        row, actual, passed = self.fixture()
        row['name'] = 'Semantics._native.native_decide.ax_1_1'
        actual = {row['name']: dict(row, declared_in_module=True)}
        result = A.validate([row], actual, conformance_fixture=passed)
        self.assertEqual((result['verdict'], result['reason']), ('INVALID', 'native-evaluator-axiom'))

    def test_malformed_binding_identifiers_are_rejected_not_executed(self):
        from zk_registry import axiom_policy as A

        row, actual, passed = self.fixture()
        row['measuring_stick'] = []
        result = A.validate([row], actual, conformance_fixture=passed)
        self.assertEqual((result['verdict'], result['reason']), ('INVALID', 'n2-binding-invalid'))


class AvailabilityServiceTests(unittest.TestCase):
    def test_resource_error_is_retryable_and_replay_does_not_rejudge_availability(self):
        from zk_registry import service as S
        from test_zk_registry_ratchet import offline_problem

        with tempfile.TemporaryDirectory() as directory:
            package, problem = offline_problem(directory)
            answer = Path(directory, 'answer')
            answer.mkdir()
            (answer / 'Solution.lean').write_text('synthetic boundary fixture\n')
            unavailable = lambda *_: dict(verdict='ERROR', reason='resource-unavailable')
            registry = S.Registry.create(str(Path(directory, 'store')), S.build_catalog([package]),
                                         reveal_window=10, verifier=unavailable)
            ident = problem['package_id']
            binding = S.commitment(ident, 'solver', S.submission_digest(str(answer)), 'salt')
            first = registry.commit(ident, 'solver', binding, 1)
            result = registry.reveal(first['sequence'], ident, 'solver', str(answer), 'salt', 2)
            self.assertEqual((result['outcome'], result['reason']), ('ERROR', 'resource-unavailable'))
            self.assertEqual(registry.status()['commits']['1']['status'], 'COMMITTED')

            def must_not_rejudge(*_):
                raise AssertionError('availability evidence is not a mathematical reveal verdict')

            replica = registry.replay(str(Path(directory, 'replica')), verifier=must_not_rejudge)
            self.assertEqual(replica.status(), registry.status())
            registry.verifier = lambda *_: dict(verdict='INVALID', reason='synthetic-invalid')
            retry = registry.reveal(1, ident, 'solver', str(answer), 'salt', 3)
            self.assertEqual(retry['outcome'], 'REJECTED')


class CatalogBindingTests(unittest.TestCase):
    def test_missing_or_changed_reference_is_not_a_catalog_binding(self):
        from zk_registry import acceptance as A

        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, '^reference-export-unpinned$'):
                A.read(directory, dict(package_id='synthetic'), required=True)

    def test_repin_uses_a_new_root_and_retains_source_and_floor_bytes(self):
        import json
        from zk_registry import acceptance as A, catalog_v2 as V, closure as D
        from test_zk_registry_ratchet import offline_problem

        with tempfile.TemporaryDirectory() as directory:
            source, problem = offline_problem(directory)
            old_metadata = D.digest(Path(source, 'problem.json'))
            export = Path(directory, 'Challenge.ndjson')
            from test_statement_identity import fixture
            from zk_registry.statement_identity import identity
            export.write_text(''.join(json.dumps(x)+'\n' for x in fixture()))
            reference = dict(verdict='PASS', path=str(export), sha256=D.digest(export),
                             reference_type_sha256='ab' * 32, assurance='test-fixture', theorem='goal')
            dest = Path(directory, 'new-root/package')
            row = V.repin_package(source, str(dest), reference, old_reference_type_sha256='ab' * 32)
            self.assertEqual(D.digest(Path(source, 'problem.json')), old_metadata)
            self.assertEqual(row['migration']['floor'], 'carried over, not rerun')
            self.assertEqual(row['migration']['battery'], 'carried over, not rerun')
            changed = json.loads((dest / 'problem.json').read_text())
            self.assertEqual(changed['statement_closure_sha256'], identity(export, 'goal')['statement_closure_sha256'])
            self.assertEqual(P.validate_problem(changed, str(dest)), [])
            self.assertEqual(changed['env']['lean'], 'v4.34.1')
            binding = A.read(str(dest), changed, required=True)
            self.assertEqual(binding.package_id, problem['package_id'])
            (dest / 'acceptance/Challenge.ndjson').write_text('changed\n')
            with self.assertRaisesRegex(ValueError, '^challenge-export-mismatch$'):
                A.read(str(dest), changed, required=True)


class ResourceStageTests(unittest.TestCase):
    def test_postcheck_timeout_is_availability_not_kernel_rejection(self):
        result = dict(compile_rc=0, n_errors=0, sorry_warnings=0, printed_axioms=[],
                      post=dict(replay='unknown', timeout=True))
        invalid, failed, errors = [], [], []
        C.apply_compile_result(result, {}, invalid, failed, errors)
        self.assertEqual(errors, ['resource-unavailable'])
        self.assertEqual(invalid, [])

    def test_native_memory_stop_is_availability_not_candidate_rejection(self):
        from unittest.mock import Mock, patch
        from zk_registry.sandbox import Sandbox

        native = Mock(return_value=L.RunResult(-9, '', 0.1, False, True, 12001, 'rss-limit'))
        native.submission_boundary = Sandbox.fixture()
        with patch.object(L, 'run_process', native):
            with self.assertRaisesRegex(L.ResourceUnavailable, '^resource-unavailable$'):
                L.run_submission(['not-executed'], {}, '.', 1, 12000)


@unittest.skipUnless(all(os.environ.get(key) for key in (
    'ZK_REGISTRY_ACCEPTANCE_V2_ENV', 'ZK_REGISTRY_ACCEPTANCE_V2_TOOLS', 'ZK_REGISTRY_ACCEPTANCE_V2_WORK_ROOT')),
    'opt-in exact v2 environment/tools and task-local work root required')
class LiveFullGateV2Tests(unittest.TestCase):
    def test_synthetic_package_complete_gate_and_statement_control(self):
        """Exercise the public package checker, never select test policy through production configuration."""
        from test_zk_registry_package import make_package
        from zk_registry import catalog_v2 as K, closure as D
        from zk_registry.sandbox import Sandbox

        root = Path(os.environ['ZK_REGISTRY_ACCEPTANCE_V2_WORK_ROOT'])
        root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=root) as directory:
            env = L.load_production_env(os.environ['ZK_REGISTRY_ACCEPTANCE_V2_ENV'])
            env.scratch = str(Path(directory, 'environment'))
            tools = D.Tools.load(os.environ['ZK_REGISTRY_ACCEPTANCE_V2_TOOLS'])
            boundary = Sandbox.development() if sys.platform == 'darwin' else Sandbox.production(tools.landrun)
            package, problem = make_package(directory)
            namespace = problem['checker']['theorem_fqn'].rsplit('.', 1)[0]
            statement = 'namespace ' + namespace + '\ntheorem det : 0 = (0 : Nat) := by\n  sorry\nend '
            statement += namespace + '\n'
            Path(package, 'Statement.lean').write_text(statement)
            problem['statement']['text'] = statement
            problem['env'].update(env.pins())
            for row in problem['checker']['files']:
                if row['role'] == 'statement':
                    row['sha256'] = D.digest(Path(package, row['path']))
            P.write_json(str(Path(package, 'problem.json')), problem)
            reference = D.prepare_reference(env, problem, package, str(Path(directory, 'reference')),
                                            str(Path(directory, 'sealed/Challenge.ndjson')), tools, boundary,
                                            300, sorted(C.ALLOWED_AXIOMS))
            self.assertEqual(reference['verdict'], 'PASS', reference)
            problem['checker']['reference_type_sha256'] = reference['reference_type_sha256']
            P.write_json(str(Path(package, 'problem.json')), problem)
            promoted = Path(directory, 'new-root/package')
            K.repin_package(package, str(promoted), reference,
                            old_reference_type_sha256=reference['reference_type_sha256'])
            proof = Path(directory, 'Solution.lean')
            proof.write_text(statement.replace('  sorry', '  rfl'))
            result = C.check(str(promoted), str(proof), env, timeout=300, keep=True,
                             work_root=str(Path(directory, 'checks')), policy_fixture=env.pins(),
                             sandbox=boundary, tools=tools)
            self.assertEqual(result['verdict'], 'PASS', result)
            self.assertEqual(result['assurance'], 'test-fixture')
            self.assertTrue(result['two_kernels']['lean_accepts'])
            self.assertTrue(result['two_kernels']['nanoda_accepts'])
            safe = statement.replace('theorem det', '@[simp] theorem helper : 0 = (0 : Nat) := rfl\n'
                                     'theorem det').replace('  sorry', '  rfl')
            proof.write_text(safe)
            allowed = C.check(str(promoted), str(proof), env, timeout=300, keep=True,
                              work_root=str(Path(directory, 'allowed')), policy_fixture=env.pins(),
                              sandbox=boundary, tools=tools)
            self.assertEqual(allowed['verdict'], 'PASS', allowed)
            proof.write_text(safe.replace('@[simp]', '@[custom_handler]'))
            forbidden = C.check(str(promoted), str(proof), env, timeout=300, policy_fixture=env.pins(),
                                sandbox=boundary, tools=tools)
            self.assertEqual((forbidden['verdict'], forbidden['reason']), ('INVALID', 'forbidden-attribute'))
            self.assertEqual(forbidden['compile'], 'skipped')
            proof.write_text(statement.replace('0 = (0 : Nat)', '1 = (1 : Nat)').replace('  sorry', '  rfl'))
            changed = C.check(str(promoted), str(proof), env, timeout=300, policy_fixture=env.pins(),
                              sandbox=boundary, tools=tools)
            self.assertEqual(changed['verdict'], 'INVALID', changed)
            self.assertEqual(changed['compile'], 'skipped')


if __name__ == '__main__':
    unittest.main()
