"""Review regressions through checker, scanner and sealed-export interfaces."""
import sys
import unittest
import json
import tempfile
import platform
import ast
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import check as C
from zk_registry import lean_runner as L, closure as D, artifact_pins, safe_fs as FS
from zk_registry import correctness_runtime as CR, sandbox as SB
import test_zk_registry_check as check_fixtures


class CompilePrecedence(unittest.TestCase):
    def test_generated_decide_native_axiom_cannot_be_allowlisted(self):
        axiom = 'Example.target._native.decide.ax_1_1'
        fixture = check_fixtures.VerdictMappingTests()
        result = fixture.ok_compile(printed_axioms=[axiom], post=fixture.ok_post(replay=None))
        invalid, fail, error = [], [], []
        C.apply_compile_result(result, {}, invalid, fail, error, allowed_axioms={axiom})
        self.assertTrue(invalid)
        self.assertEqual(error, [])

    def test_post_boundary_availability_precedes_compile_boundary_rejection(self):
        fixture = check_fixtures.VerdictMappingTests()
        result = fixture.ok_compile(boundary=dict(verdict='INVALID', reason='resource-cap'),
            post=fixture.ok_post(boundary=dict(verdict='ERROR', reason='sandbox-unavailable')))
        self.assertEqual(fixture.verdict(result), ([], [], ['sandbox-unavailable']))

    def test_silent_nonzero_compile_exit_is_availability_error(self):
        fixture = check_fixtures.VerdictMappingTests()
        result = fixture.ok_compile(compile_rc=1, n_errors=0, errors=[], post=None)
        self.assertEqual(fixture.verdict(result), ([], [], ['resource-unavailable']))

    def test_deterministic_findings_survive_unavailable_replay(self):
        fixture = check_fixtures.VerdictMappingTests()
        for finding, axioms, sorry in [('INVALID', ['Lean.ofReduceBool'], []),
                                      ('FAIL', ['sorryAx'], ['uses sorry']),
                                      ('BOTH', ['Lean.ofReduceBool', 'sorryAx'], ['uses sorry']),
                                      ('PASS', [], [])]:
            for replay in ['ok', 'FAIL: legacy exception', None, 'missing-post']:
                for availability in [False, True]:
                    with self.subTest(finding=finding, replay=replay, availability=availability):
                        result = fixture.ok_compile(printed_axioms=axioms, sorry_warnings=sorry,
                            post=None if replay == 'missing-post' else fixture.ok_post(replay=replay, rc=1 if replay != 'ok' else 0))
                        if availability:
                            result['compile_timeout'] = True
                        invalid, fail, error = fixture.verdict(result)
                        actual = C.verdict_of(dict(invalid=invalid, fail=fail, error=error))
                        expected = ('ERROR' if availability else 'INVALID' if finding == 'BOTH' else finding if finding != 'PASS'
                                    else 'ERROR' if replay != 'ok' else 'PASS')
                        self.assertEqual(actual, expected)
                        if availability:
                            self.assertEqual(error, ['resource-unavailable'])
                        elif finding != 'PASS':
                            self.assertEqual(error, [])


class LeanLexing(unittest.TestCase):
    def test_public_checker_rejects_unclassifiable_source_without_crashing(self):
        from test_zk_registry_package import make_package
        for body in [b'  exact "unfinished\n', b'  exact \xff\n']:
            with self.subTest(body=body), tempfile.TemporaryDirectory() as temp:
                package, _ = make_package(temp)
                statement = Path(package, 'Statement.lean').read_text()
                proof = Path(temp)/'Proof.lean'
                proof.write_bytes(check_fixtures.submission(statement, '  trivial\n').encode().replace(b'  trivial\n', body))
                env = check_fixtures.fake_env(temp)
                result = C.check(package, str(proof), env, policy_fixture=env.pins())
                self.assertEqual((result['verdict'], result.get('reason')), ('INVALID', 'unparseable-source'))

    def test_shared_literal_corpus(self):
        path = Path(C.__file__).parents[2]/'fixtures/lean-lexing/corpus-v1.json'
        for case in json.loads(path.read_text())['cases']:
            with self.subTest(case=case['name']):
                hits = C.forbidden_scan([('proof', case['source'])])
                self.assertEqual(any(h.startswith('unparseable-source in ') for h in hits), case['invalid'])
                if not case['invalid']:
                    self.assertEqual(any(h.startswith('axiom in ') for h in hits), case['has_axiom'])

    def test_critical_raw_layer_and_native_forms(self):
        for token in ['maxHeartbeats', 'maxRecDepth', 'decide +native',
                      'decide (config := { native := true })', 'bv_decide', 'bv_check']:
            for source in [token, '-- '+token, '"'+token+'"', '«'+token+'»']:
                with self.subTest(source=source):
                    self.assertTrue(C.forbidden_scan([('proof', source)]))


class SymlinkLoops(unittest.TestCase):
    def test_public_checker_submission_loop_returns_invalid(self):
        from test_zk_registry_package import make_package
        with tempfile.TemporaryDirectory() as temp:
            package, _ = make_package(temp)
            proof = Path(temp)/'Proof.lean'; proof.symlink_to(proof.name)
            env = check_fixtures.fake_env(temp)
            result = C.check(package, str(proof), env, policy_fixture=env.pins())
            self.assertEqual((result['verdict'], result['reason']), ('INVALID', 'submission-output-path'))

    def test_cli_trusted_runtime_loop_writes_error_report(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); runtime = root/'runtime'; runtime.symlink_to(runtime.name)
            report = root/'report.json'
            rc = C.main(['unused-package', 'unused-proof', '--lean-env', 'unused-env',
                         '--tools-root', str(runtime), '--report', str(report)])
            self.assertEqual(rc, 3)
            self.assertEqual(json.loads(report.read_text())['verdict'], 'ERROR')

    def test_trusted_artifact_resolve_runtime_error_is_structured(self):
        env = L.LeanEnv('/unused', [], 'v4.34.1', {}, '', '/unused')
        descriptor = dict(env.pins(), artifact_profiles={platform.system()+'-'+platform.machine():{}})
        with patch.object(artifact_pins, 'snapshot', side_effect=RuntimeError('Symlink loop')):
            self.assertEqual(L.production_environment_reason(env, descriptor), 'environment-mismatch')

    def test_trusted_tools_resolve_runtime_error_is_structured(self):
        descriptor = dict(binary_profiles={platform.system()+'-'+platform.machine():{}})
        with patch.object(D.Path, 'read_text', return_value=json.dumps(descriptor)), \
                patch.object(D.Path, 'resolve', side_effect=RuntimeError('Symlink loop')):
            with self.assertRaises(D.ToolsUnavailable):
                D.Tools.load('/trusted')

    def test_submission_loop_remains_invalid(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            loop = root/'loop'; loop.symlink_to('loop')
            with FS.submission_io({str(root)}):
                with self.assertRaises(L.BoundaryInvalid):
                    FS.Path(loop).read_text()


class ComparatorStatus(unittest.TestCase):
    def test_only_private_status_events_establish_verdicts(self):
        accepted = [dict(stage='nanoda', result='accept'), dict(stage='lean', result='accept'),
                    dict(stage='quotient', result='accept')]
        cases = [
            ('forged-markers', None, 0, 'ERROR', 'sandbox-unavailable'),
            ('valid-no-stdout-markers', accepted, 0, 'PASS', None),
            ('both-acceptance-final2', accepted, 2, 'ERROR', 'sandbox-unavailable'),
            ('quotient-rejection', accepted[:2]+[dict(stage='quotient', result='reject')], 2, 'INVALID', 'kernel-rejection'),
            ('export-integrity', [dict(stage='error', reason='export-integrity')], 2, 'ERROR', 'export-integrity'),
            ('disagreement', [dict(stage='nanoda', result='reject'), *accepted[1:]], 2, 'INVALID', 'kernel-disagreement'),
            ('resource-cap', [dict(stage='policy', reason='resource-cap')], 2, 'INVALID', 'resource-cap'),
        ]
        for name, events, code, verdict, reason in cases:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp); challenge = root/'Challenge'; solution = root/'Solution'
                challenge.write_text('sealed'); solution.write_text('sealed')
                tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
                def run(command, env, *args):
                    if events is not None and env.get('COMPARATOR_STATUS_FILE'):
                        rows = [dict(version=1, **event) for event in events]
                        rows.append(dict(version=1, stage='final', rc=code))
                        Path(env['COMPARATOR_STATUS_FILE']).write_text(''.join(json.dumps(row)+'\n' for row in rows))
                    output = ('Lean default kernel accepts the solution\nnanoda kernel accepts the solution\n'
                              if name != 'valid-no-stdout-markers' else 'kernel declaration names only\n')
                    return L.RunResult(code, output, 0, False)
                with patch.object(CR, 'runner', return_value=run):
                    result = D.compare_exports(str(challenge), str(solution), 'X.goal', tools,
                        SB.Sandbox.fixture(), str(root/'compare'), 10, [])
                self.assertEqual((result['verdict'], result['reason']), (verdict, reason))

    def test_boolean_final_exit_is_not_a_status_integer(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); challenge, solution = root/'Challenge', root/'Solution'
            challenge.write_text('sealed'); solution.write_text('sealed')
            tools = D.Tools('/usr/bin/true', '/usr/bin/true', '/usr/bin/true', '/usr', None)
            def run(command, env, *args):
                events = [dict(version=1, stage=stage, result='accept') for stage in ['nanoda', 'lean', 'quotient']]
                events.append(dict(version=1, stage='final', rc=False))
                Path(env['COMPARATOR_STATUS_FILE']).write_text(''.join(json.dumps(e)+'\n' for e in events))
                return L.RunResult(0, '', 0, False)
            with patch.object(CR, 'runner', return_value=run):
                result = D.compare_exports(str(challenge), str(solution), 'X.goal', tools,
                    SB.Sandbox.fixture(), str(root/'compare'), 10, [])
            self.assertEqual((result['verdict'], result['reason']), ('ERROR', 'sandbox-unavailable'))


class ImportedArtifacts(unittest.TestCase):
    def test_ir_and_ir_signature_changes_break_environment_pin(self):
        for extension in ['.ir', '.ir.sig']:
            with self.subTest(extension=extension), tempfile.TemporaryDirectory() as temp:
                root = Path(temp); (root/'bin').mkdir(); (root/'lib/lean').mkdir(parents=True)
                (root/'bin/lean').write_bytes(b'lean')
                artifact = root/('lib/lean/Init'+extension); artifact.write_bytes(b'original')
                env = L.LeanEnv(str(root), [], 'v4.34.1', {}, '', temp)
                expected = dict(env.pins(), artifact_profiles={platform.system()+'-'+platform.machine():artifact_pins.snapshot(env)})
                self.assertIsNone(L.production_environment_reason(env, expected))
                artifact.write_bytes(b'replaced')
                self.assertEqual(L.production_environment_reason(env, expected), 'environment-mismatch')


class CIImports(unittest.TestCase):
    def test_det1_test_helpers_trigger_both_ci_events(self):
        scripts = Path(__file__).parent
        pending, seen = ['test_zk_registry_det1b', 'test_zk_registry_det1c'], set()
        while pending:
            name = pending.pop()
            if name in seen:
                continue
            seen.add(name)
            for node in ast.walk(ast.parse((scripts/(name+'.py')).read_text())):
                modules = ([node.module] if isinstance(node, ast.ImportFrom) else
                           [alias.name for alias in node.names] if isinstance(node, ast.Import) else [])
                pending.extend(m for m in modules if m and m.startswith('test_zk_registry') and m not in seen)
        workflow = (scripts.parent/'.github/workflows/acceptance-gate-linux.yml').read_text()
        for name in seen:
            self.assertEqual(workflow.count('      - scripts/'+name+'.py\n'), 2, name)


if __name__ == '__main__':
    unittest.main()
