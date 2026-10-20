"""Verdict determinism through checker and sealed-export interfaces."""
import sys
import json
import hashlib
import platform
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import check as C
from zk_registry import sandbox as SB, lean_runner as L
from zk_registry import closure as D, correctness_runtime as CR, gates as G
from zk_registry import acceptance as A
from zk_registry import package as P
import test_zk_registry_check as check_tests


class AvailabilityVerdicts(unittest.TestCase):
    def test_rejection_exit_with_two_acceptance_markers_is_an_error(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            challenge, solution = root/'Challenge', root/'Solution'
            challenge.write_text('sealed'); solution.write_text('sealed')
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
            result = L.RunResult(2, 'Lean default kernel accepts the solution\n'
                'nanoda kernel accepts the solution\nresource-cap: in a declaration name\n', 0, False)
            with patch.object(CR, 'runner', return_value=lambda *args: result):
                actual = D.compare_exports(str(challenge), str(solution), 'X.goal', tools,
                    SB.Sandbox.fixture(), str(root/'compare'), 10, [])
            self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'sandbox-unavailable'))

    def test_compilation_limits_are_uniform_gate_options(self):
        with tempfile.TemporaryDirectory() as temp:
            env = L.LeanEnv('/unused', [], '', {}, '', temp)
            package_options = ['-DautoImplicit=false', '-DmaxHeartbeats=0',
                               '-DmaxRecDepth=999999', '--tstack=400000']
            with patch.object(L, 'run_lean', return_value=L.RunResult(0, '', 0, False)) as run:
                L.compile_module(env, temp, 'Proof.lean', temp, package_options, 10)
            args = run.call_args.args[1]
            self.assertNotIn('-DmaxHeartbeats=0', args)
            self.assertNotIn('-DmaxRecDepth=999999', args)
            self.assertNotIn('--tstack=400000', args)
            self.assertIn('-DautoImplicit=false', args)
            self.assertEqual(sum(a.startswith('-DmaxHeartbeats=') for a in args), 1)
            self.assertEqual(sum(a.startswith('-DmaxRecDepth=') for a in args), 1)
            self.assertEqual(sum(a.startswith('--tstack=') for a in args), 1)

    def test_submission_compilation_also_uses_gate_limits(self):
        with tempfile.TemporaryDirectory() as temp:
            env = L.LeanEnv('/unused', [], '', {}, '', str(Path(temp)/'scratch'))
            problem = dict(checker=dict(theorem_fqn='X.goal', files=[],
                lean_opts=['-DmaxHeartbeats=0', '-DmaxRecDepth=999999', '--tstack=400000']))
            result = L.RunResult(1, json.dumps(dict(severity='error', kind='tactic.failed', data='fixture')), 0, False)
            with patch.object(L, 'run_lean', return_value=result) as run:
                C.compile_in_sandbox(env, problem, temp, 'theorem goal : True := by trivial',
                                     str(Path(temp)/'check'), 10, SB.Sandbox.fixture())
            args = run.call_args.args[1]
            self.assertNotIn('-DmaxHeartbeats=0', args)
            self.assertNotIn('-DmaxRecDepth=999999', args)
            self.assertNotIn('--tstack=400000', args)
            self.assertIn('-DmaxHeartbeats=4194304', args)
            self.assertIn('-DmaxRecDepth=16384', args)
            self.assertIn('--tstack=65536', args)

    def test_comparator_unknown_exit_never_reads_semantic_stdout(self):
        for code in (1, 70, 101, 134, 200, 255):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                challenge, solution = root/'Challenge', root/'Solution'
                challenge.write_text('sealed'); solution.write_text('sealed')
                tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
                result = L.RunResult(code, 'axiom mismatch resource-cap: constants', 0, False)
                with patch.object(CR, 'runner', return_value=lambda *args: result):
                    actual = D.compare_exports(str(challenge), str(solution), 'X.goal', tools,
                        SB.Sandbox.fixture(), str(root/'compare'), 10, [])
                self.assertEqual(actual['verdict'], 'ERROR')

    def test_compile_limit_classification_uses_diagnostic_kind(self):
        for kind, message, invalid_expected in (
            ('runtime.maxHeartbeats', 'localized diagnostic', ['resource-cap']),
            ('nested.runtime.maxRecDepth', 'localized diagnostic', ['resource-cap']),
            ('nested.nested.nested.runtime.maxHeartbeats', 'localized diagnostic', ['resource-cap']),
            ('tactic.failed', 'user text says maxHeartbeats', []),
        ):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temp:
                env = L.LeanEnv('/unused', [], '', {}, '', str(Path(temp)/'scratch'))
                result = L.RunResult(1, json.dumps(dict(severity='error', kind=kind, data=message)), 0, False)
                problem = dict(checker=dict(theorem_fqn='X.goal', lean_opts=[], files=[]))
                with patch.object(L, 'run_lean', return_value=result):
                    compiled = C.compile_in_sandbox(env, problem, temp, 'theorem goal : True := by trivial',
                        str(Path(temp)/'check'), 10, SB.Sandbox.fixture())
                invalid, fail, error = check_tests.VerdictMappingTests().verdict(compiled)
                self.assertEqual(invalid, invalid_expected)
                self.assertEqual(error, [])
                self.assertEqual(bool(fail), not bool(invalid_expected))

    def test_frozen_v3_policy_metadata_remains_readable(self):
        from test_zk_registry_correctness_gnark import package_fixture
        problem = package_fixture()
        problem['checker']['forbidden_version'] = 3
        problem['checker']['forbidden_tokens'] = [label for label in C.FORBIDDEN_LABELS
                                                   if label != 'limit-override']
        self.assertEqual(P.validate_problem(problem), [])

    def test_rebuilt_challenge_digest_mismatch_is_error(self):
        from test_zk_registry_package import make_package
        from test_zk_registry_check import fake_env, submission
        with tempfile.TemporaryDirectory() as temp:
            package, problem = make_package(temp)
            proof = Path(temp)/'Solution.lean'
            proof.write_text(submission(Path(package, 'Statement.lean').read_text(), '  trivial\n'))
            env = fake_env(temp)
            binding = A.RootBinding(problem['package_id'], dict(measuring_stick_axioms=[],
                challenge_export=dict(sha256='a'*64)), 'c'*64)
            with patch.object(A, 'read', return_value=binding), patch.object(D, 'prepare_reference',
                return_value=dict(verdict='PASS', sha256='b'*64)), patch.object(C, 'compile_in_sandbox') as compile:
                actual = C.check(package, str(proof), env, policy_fixture=env.pins(),
                    sandbox=SB.Sandbox.fixture(), tools=D.Tools('', '', '', '', None))
            self.assertEqual(actual['verdict'], 'ERROR', actual)
            self.assertEqual(actual['error'], ['challenge-export-mismatch'])
            compile.assert_not_called()

    def test_exporter_resolves_pinned_init_before_solver_shadow(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            pinned, proof = root/'toolchain/lib/lean', root/'proof'
            pinned.mkdir(parents=True); proof.mkdir()
            (pinned/'Init.olean').write_text('pinned core')
            (proof/'Init').mkdir()
            env = L.LeanEnv(str(root/'toolchain'), [], '', {}, '', str(root/'scratch'))
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
            def export(argv, task_env, *args):
                # Lean package lookup commits to the first root containing Init,
                # including an empty solver-created Init directory.
                for path in task_env['LEAN_PATH'].split(':'):
                    candidate = Path(path)/'Init'
                    if candidate.is_dir() or candidate.with_suffix('.olean').is_file():
                        ok = candidate.with_suffix('.olean').is_file()
                        return L.RunResult(0 if ok else 1, 'sealed export' if ok else 'missing Init', 0, False)
                raise AssertionError('pinned core missing from export search path')
            with patch.object(CR, 'runner', return_value=export):
                actual = D.export_module(env, 'ProofMod', 'X.goal', str(root/'sealed/Solution'),
                    tools, SB.Sandbox.fixture(), str(proof), [str(proof)], 10, [])
            self.assertEqual(actual['verdict'], 'PASS', actual)

    def test_provisional_caps_are_pinned_together(self):
        caps = L.testnet_caps()
        self.assertEqual(caps['elaboration'], dict(status='PROVISIONAL-TESTNET',
            max_heartbeats=4194304, max_rec_depth=16384, worker_stack_bytes=67108864))
        self.assertEqual(caps['status'], 'PROVISIONAL-TESTNET')
        self.assertFalse(caps['mainnet_valid'])
        self.assertEqual(caps['lean'], dict(max_heartbeats=262144, max_rec_depth=2048,
                                          worker_stack_bytes=67108864))
        self.assertEqual(caps['nanoda'], dict(steps_per_declaration=33554432, worker_stack_bytes=67108864))
        self.assertEqual(caps['conron'], dict(fuel_decrements_per_phase=8589934592))

    def test_replay_uses_fixed_provisional_worker_stack(self):
        with tempfile.TemporaryDirectory() as temp:
            env = L.LeanEnv('/unused', [], '', {}, '', temp)
            with patch.object(L, 'run_lean', return_value=L.RunResult(1, '', 0, False)) as run:
                G.run_replay(env, 'Proof.olean', 'X.goal', str(Path(temp)/'out'),
                             ['--tstack=32768'], [], 10)
            self.assertIn('--tstack=65536', run.call_args.args[1])
            self.assertNotIn('--tstack=32768', run.call_args.args[1])

    def test_comparator_untyped_lean_negative_cannot_be_disagreement(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            challenge, solution = root/'Challenge', root/'Solution'
            challenge.write_text('sealed'); solution.write_text('sealed')
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
            result = L.RunResult(2, 'nanoda kernel accepts the solution\n'
                'Lean default kernel rejects the solution\nresource-cap: in a declaration name\n', 0, False)
            with patch.object(CR, 'runner', return_value=lambda *args: result):
                actual = D.compare_exports(str(challenge), str(solution), 'X.goal', tools,
                    SB.Sandbox.fixture(), str(root/'compare'), 10, [])
            self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'replay-unavailable'))

    def test_legacy_unbounded_replay_failure_is_an_error(self):
        fixture = check_tests.VerdictMappingTests()
        result = fixture.ok_compile(post=fixture.ok_post(replay='FAIL: no structured rejection', rc=1))
        self.assertEqual(fixture.verdict(result), ([], [], ['replay-unavailable']))

    def test_changed_sealed_exports_are_node_errors(self):
        for changed in ('Challenge', 'Solution'):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                challenge, solution = root/'Challenge', root/'Solution'
                challenge.write_text('sealed'); solution.write_text('sealed')
                tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
                def run(*args):
                    (root/changed).write_text('corrupted after seal')
                    return L.RunResult(0, 'Lean default kernel accepts the solution\n'
                        'nanoda kernel accepts the solution\n', 0, False)
                with patch.object(CR, 'runner', return_value=run):
                    result = D.compare_exports(str(challenge), str(solution), 'X.goal', tools,
                        SB.Sandbox.fixture(), str(root/'compare'), 10, [])
                self.assertEqual((result['verdict'], result['reason']), ('ERROR', 'export-integrity'))

    def test_pids_exhaustion_cannot_be_a_candidate_rejection(self):
        boundary = dict(verdict='INVALID', reason='resource-cap',
                        observation=dict(pids_max_events=1))
        result = L.RunResult(1, 'compile failed', 0, False, boundary=boundary)
        stopped = SB.boundary_failure(result)
        self.assertEqual((stopped['verdict'], stopped['reason']), ('ERROR', 'resource-unavailable'))
        with self.assertRaises(L.ResourceUnavailable):
            L.enforce_boundary(result)

    def test_compile_and_replay_crashes_are_errors(self):
        fixture = check_tests.VerdictMappingTests()
        for code in (-6, 134, -11, 139):
            for stage in ('compile', 'replay'):
                with self.subTest(code=code, stage=stage):
                    result = fixture.ok_compile()
                    if stage == 'compile':
                        result.update(compile_rc=code, post=None)
                    else:
                        result['post'].update(rc=code, replay=None)
                    self.assertEqual(fixture.verdict(result), ([], [], ['resource-unavailable']))


class ArtifactPins(unittest.TestCase):
    def test_internal_runtime_library_alias_binds_target_bytes_and_rejects_escape(self):
        from zk_registry import artifact_pins
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); prefix = root/'prefix'
            (prefix/'bin').mkdir(parents=True); (prefix/'lib/lean').mkdir(parents=True)
            (prefix/'bin/lean').write_bytes(b'lean')
            library = prefix/'lib/libunwind.so.1'; library.write_bytes(b'original library')
            alias = prefix/'lib/libunwind.so'; alias.symlink_to(library.name)
            env = L.LeanEnv(str(prefix), [], 'v4.34.1', {}, '', temp)
            try:
                pins = artifact_pins.snapshot(env)
            except OSError as error:
                self.fail('Official internal runtime library alias must be fingerprintable: '+str(error))
            expected = dict(env.pins(), artifact_profiles={platform.system()+'-'+platform.machine():pins})
            self.assertIsNone(L.production_environment_reason(env, expected))
            library.write_bytes(b'replaced library')
            self.assertEqual(L.production_environment_reason(env, expected), 'environment-mismatch')
            library.write_bytes(b'original library')
            external = root/'external.so'; external.write_bytes(library.read_bytes())
            alias.unlink(); alias.symlink_to(external)
            self.assertEqual(L.production_environment_reason(env, expected), 'environment-mismatch')

    def test_trusted_provisioning_alias_uses_canonical_artifact_bytes(self):
        from zk_registry import artifact_pins
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            prefix, modules = root/'prefix', root/'modules'
            (prefix/'bin').mkdir(parents=True)
            (prefix/'lib/lean').mkdir(parents=True)
            (prefix/'bin/lean').write_bytes(b'lean executable')
            (prefix/'lib/lean/Init.olean').write_bytes(b'core')
            modules.mkdir(); (modules/'Mathlib.olean').write_bytes(b'mathlib')
            env = L.LeanEnv(str(prefix), [str(modules)], 'v4.34.1', {}, '', temp)
            expected = dict(env.pins(), artifact_profiles={platform.system()+'-'+platform.machine():
                                                          artifact_pins.snapshot(env)})
            (root/'toolchain-alias').symlink_to(prefix, target_is_directory=True)
            (root/'modules-alias').symlink_to(modules, target_is_directory=True)
            env.toolchain = str(root/'toolchain-alias')
            env.lean_path = [str(root/'modules-alias')]
            self.assertIsNone(L.production_environment_reason(env, expected))
            (modules/'Mathlib.olean').write_bytes(b'replaced behind alias')
            self.assertEqual(L.production_environment_reason(env, expected), 'environment-mismatch')

    def test_comparator_runtime_prefix_is_also_digest_pinned(self):
        from zk_registry import artifact_pins
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            prefix = root/'prefix'
            (prefix/'bin').mkdir(parents=True); (prefix/'lib/lean').mkdir(parents=True)
            (prefix/'bin/lean').write_bytes(b'lean executable')
            core = prefix/'lib/lean/Init.olean'; core.write_bytes(b'core')
            env = L.LeanEnv(str(prefix), [], 'v4.34.1', {}, '', temp)
            profile = platform.system()+'-'+platform.machine()
            expected = dict(artifact_profiles={profile:artifact_pins.snapshot(env)})
            pins = dict(kernel_prefix='prefix')
            for name in ('comparator', 'exporter', 'nanoda'):
                binary = root/name; binary.write_bytes(b'#!/bin/sh\nexit 0\n'); binary.chmod(0o755)
                pins[name] = dict(path=name, sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            descriptor = dict(binary_profiles={profile:pins}, comparator=dict(lean='v4.34.1'))
            with patch.object(D.Path, 'read_text', return_value=json.dumps(descriptor)), \
                    patch.object(L, 'production_descriptor', return_value=expected):
                self.assertEqual(D.Tools.load(str(root)).kernel_prefix, str(prefix.resolve()))
                core.write_bytes(b'replaced runtime core')
                with self.assertRaises(D.ToolsUnavailable): D.Tools.load(str(root))

    def test_imported_olean_set_changes_fail_closed(self):
        from zk_registry import artifact_pins
        for change in ('replace', 'add', 'remove', 'reorder', 'shadow-directory'):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                (root/'bin').mkdir(); (root/'bin/lean').write_bytes(b'lean')
                core, mathlib = root/'lib/lean', root/'mathlib'
                core.mkdir(parents=True); mathlib.mkdir()
                (core/'Init.olean').write_bytes(b'core')
                (mathlib/'Mathlib.olean').write_bytes(b'accepted Mathlib')
                other = root/'other'; other.mkdir()
                (other/'Other.olean').write_bytes(b'other')
                env = L.LeanEnv(str(root), [str(mathlib), str(other)], 'v4.34.1', {}, '', temp)
                expected = dict(env.pins(), artifact_profiles={platform.system()+'-'+platform.machine():
                                                              artifact_pins.snapshot(env)})
                self.assertIsNone(L.production_environment_reason(env, expected))
                if change == 'replace': (mathlib/'Mathlib.olean').write_bytes(b'replaced Mathlib')
                elif change == 'add': (core/'Injected.olean').write_bytes(b'extra')
                elif change == 'remove': (core/'Init.olean').unlink()
                elif change == 'reorder': env.lean_path.reverse()
                else: (mathlib/'Init').mkdir()
                self.assertEqual(L.production_environment_reason(env, expected), 'environment-mismatch')

    def test_same_version_replaced_lean_binary_fails_closed(self):
        from zk_registry import artifact_pins
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root/'bin').mkdir()
            (root/'lib/lean').mkdir(parents=True)
            lean = root/'bin/lean'
            lean.write_bytes(b'original executable')
            env = L.LeanEnv(str(root), [], 'v4.34.1', {}, '', temp)
            expected = env.pins()
            expected['artifact_profiles'] = {platform.system()+'-'+platform.machine():
                                            artifact_pins.snapshot(env)}
            self.assertIsNone(L.production_environment_reason(env, expected))
            lean.write_bytes(b'replaced but same version string')
            self.assertEqual(L.production_environment_reason(env, expected), 'environment-mismatch')


if __name__ == '__main__':
    unittest.main()
