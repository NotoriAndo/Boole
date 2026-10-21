"""Review regressions through checker, scanner and sealed-export interfaces."""
import sys
import unittest
import json
import tempfile
import platform
import ast
import itertools
import time
from unittest.mock import patch
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import check as C
from zk_registry import lean_runner as L, closure as D, artifact_pins, safe_fs as FS
from zk_registry import correctness_runtime as CR, sandbox as SB
import test_zk_registry_check as check_fixtures


class CompilePrecedence(unittest.TestCase):
    def test_precedence_product(self):
        # Every availability kind crosses every finding and replay state.
        availability = ['none', 'compile-exit', 'compile-timeout', 'compile-memory',
                        'compile-signal', 'post-exit', 'post-timeout', 'post-memory',
                        'post-signal', 'silent-compile-exit']
        findings = ['none', 'compile-boundary-invalid', 'post-boundary-invalid',
                    'compile-boundary-error', 'post-boundary-error', 'both-boundaries',
                    'compile-errors', 'compile-limit', 'sorry-warning', 'printed-sorry',
                    'printed-disallowed', 'both-printed', 'post-target', 'post-kind',
                    'post-type', 'post-sorry', 'post-disallowed', 'post-axiom', 'post-unsafe', 'missing-print', 'existing-invalid', 'existing-fail']
        for available, finding, replay in itertools.product(
                availability, findings, ['ok', 'failed', 'absent', 'missing-post']):
            with self.subTest(availability=available, finding=finding, replay=replay):
                post = dict(rc=0, replay='ok', target=dict(kind='theorem'),
                            type_sha256='reference', axioms=[], consts=[])
                result = dict(compile_rc=0, n_errors=0, printed_axioms=[],
                              sorry_warnings=0, errors=[], post=post)
                expected_finding = 'PASS'
                if finding.startswith('existing-'):
                    expected_finding = 'INVALID' if finding == 'existing-invalid' else 'FAIL'
                elif finding.endswith('boundary-invalid'):
                    (result if finding.startswith('compile') else post)['boundary'] = dict(
                        verdict='INVALID', reason='owned-descendants-remained')
                    # A boundary launcher exits 125 for every non-PASS record.
                    if finding.startswith('compile'):
                        result['compile_rc'] = 125
                    else:
                        post['rc'] = 125
                    expected_finding = 'INVALID'
                elif finding.endswith('boundary-error'):
                    (result if finding.startswith('compile') else post)['boundary'] = dict(
                        verdict='ERROR', reason='sandbox-unavailable')
                    expected_finding = 'ERROR'
                elif finding == 'both-boundaries':
                    result['boundary'] = dict(verdict='INVALID', reason='owned-descendants-remained')
                    post['boundary'] = dict(verdict='ERROR', reason='sandbox-unavailable')
                    result['compile_rc'] = 125
                    expected_finding = 'ERROR'
                elif finding == 'compile-errors':
                    result.update(compile_rc=1, n_errors=1, errors=['structured error'])
                    expected_finding = 'FAIL'
                elif finding == 'compile-limit':
                    result.update(compile_rc=1, n_errors=1,
                                  error_kinds=['nested.runtime.maxHeartbeats'])
                    expected_finding = 'INVALID'
                elif finding == 'sorry-warning':
                    result['sorry_warnings'] = 1
                    expected_finding = 'FAIL'
                elif finding.startswith('printed-') or finding == 'both-printed':
                    result['printed_axioms'] = (['sorryAx'] if finding == 'printed-sorry' else
                        ['Lean.ofReduceBool'] if finding == 'printed-disallowed' else
                        ['sorryAx', 'Lean.ofReduceBool'])
                    expected_finding = 'FAIL' if finding == 'printed-sorry' else 'INVALID'
                elif finding == 'missing-print':
                    result['printed_axioms'] = None
                    expected_finding = 'FAIL'
                elif finding.startswith('post-'):
                    expected_finding = 'INVALID'
                    if finding == 'post-target': post['target'] = 'missing'
                    elif finding == 'post-kind': post['target'] = dict(kind='definition')
                    elif finding == 'post-type': post['type_sha256'] = 'different'
                    elif finding == 'post-sorry':
                        post['axioms'] = ['sorryAx']; expected_finding = 'FAIL'
                    elif finding == 'post-disallowed': post['axioms'] = ['Lean.ofReduceBool']
                    elif finding == 'post-axiom': post['consts'] = [dict(name='bad', kind='axiom', unsafe=False)]
                    elif finding == 'post-unsafe': post['consts'] = [dict(name='bad', kind='definition', unsafe=True)]
                if replay == 'failed': post['replay'] = 'FAIL: unavailable'
                elif replay == 'absent': post['replay'] = None
                elif replay == 'missing-post': result['post'] = None
                if finding.startswith('post-') and (replay == 'missing-post' or
                        ('boundary' not in finding and replay != 'ok')):
                    expected_finding = 'PASS'
                if finding == 'both-boundaries' and replay == 'missing-post':
                    expected_finding = 'INVALID'
                active_availability = available != 'none' and not (
                    available.startswith('post-') and replay == 'missing-post')
                if available == 'compile-exit': result['compile_rc'] = 124
                elif available == 'compile-timeout': result['compile_timeout'] = True
                elif available == 'compile-memory': result['compile_memkill'] = True
                elif available == 'compile-signal': result['compile_killed_by'] = 'signal'
                elif available == 'post-exit': post['rc'] = 134
                elif available == 'post-timeout': post['timeout'] = True
                elif available == 'post-memory': post['memkill'] = True
                elif available == 'post-signal': post['killed_by'] = 'signal'
                elif available == 'silent-compile-exit': result.update(compile_rc=1, n_errors=0)
                expected = ('ERROR' if active_availability else expected_finding if expected_finding != 'PASS'
                            else 'ERROR' if replay != 'ok' else 'PASS')
                if available == 'silent-compile-exit' and any(
                        record and record.get('boundary') for record in [result, result['post']]):
                    expected = expected_finding
                invalid = ['existing finding'] if finding == 'existing-invalid' else []
                fail = ['existing finding'] if finding == 'existing-fail' else []
                error = []
                C.apply_compile_result(result, dict(reference_type_sha256='reference'), invalid, fail, error)
                self.assertEqual(C.verdict_of(dict(invalid=invalid, fail=fail, error=error)), expected)

    def test_missing_print_output_ranks_below_every_deterministic_finding(self):
        fixture = check_fixtures.VerdictMappingTests()
        for finding, compile_fields, post_fields, expected in [
            ('compile-error', dict(n_errors=1, errors=['structured error']), {}, 'FAIL'),
            ('sorry-warning', dict(sorry_warnings=1), {}, 'FAIL'),
            ('post-type', {}, dict(type_sha256='different'), 'INVALID'),
            ('post-sorry', {}, dict(axioms=['sorryAx']), 'FAIL'),
            ('post-disallowed', {}, dict(axioms=['Lean.ofReduceBool']), 'INVALID'),
            ('post-axiom', {}, dict(consts=[dict(name='bad', kind='axiom', unsafe=False)]), 'INVALID'),
            ('post-unsafe', {}, dict(consts=[dict(name='bad', kind='definition', unsafe=True)]), 'INVALID'),
        ]:
            with self.subTest(finding=finding):
                result = fixture.ok_compile(printed_axioms=None, post=fixture.ok_post(**post_fields), **compile_fields)
                invalid, fail, error = fixture.verdict(result)
                self.assertEqual(C.verdict_of(dict(invalid=invalid, fail=fail, error=error)), expected)
                self.assertFalse(any('output missing' in reason for reason in fail + error))

    def test_missing_print_preserves_existing_findings(self):
        for verdict in ['INVALID', 'FAIL', 'ERROR']:
            with self.subTest(verdict=verdict):
                findings = dict(invalid=[], fail=[], error=[])
                findings[verdict.lower() if verdict != 'INVALID' else 'invalid'].append('existing finding')
                C.apply_compile_result(dict(compile_rc=0, n_errors=0, printed_axioms=None, post=None),
                                       {}, findings['invalid'], findings['fail'], findings['error'])
                self.assertEqual(C.verdict_of(findings), verdict)
                self.assertEqual(sum(map(len, findings.values())), 1)

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

    def test_print_axioms_is_bound_to_appended_line_and_full_name(self):
        text = 'namespace Example\ntheorem goal : True := by trivial\nend Example\n'
        appended = text.count('\n') + 2
        def message(data, line):
            return dict(severity='information', data=data, pos=dict(line=line, column=0))
        valid = message("'Example.goal' does not depend on any axioms", appended)
        forged = message("'Example.goal' depends on axioms: [Lean.ofReduceBool]", 2)
        cases = [([valid, forged], []), ([forged], None),
                 ([message("'Other.goal' does not depend on any axioms", appended)], None),
                 ([message("solver text: 'Example.goal' does not depend on any axioms", appended)], None)]
        for messages, expected in cases:
            with self.subTest(messages=messages), tempfile.TemporaryDirectory() as temp:
                env = L.LeanEnv('/unused', [], '', {}, '', temp)
                problem = dict(checker=dict(theorem_fqn='Example.goal', files=[], lean_opts=[]))
                output = '\n'.join(json.dumps(message) for message in messages)
                with patch.object(L, 'run_lean', return_value=L.RunResult(1, output, 0, False)):
                    result = C.compile_in_sandbox(env, problem, temp, text, str(Path(temp)/'work'),
                                                  10, SB.Sandbox.fixture())
                self.assertEqual(result['printed_axioms'], expected)
                self.assertEqual(Path(temp, 'work/proof/ProofMod.lean').read_text().splitlines()[appended-1],
                                 'set_option pp.fullNames true in #print axioms Example.goal')

    def test_raw_fid_lines_do_not_establish_gate_verdicts(self):
        from zk_registry import gates as G
        with tempfile.TemporaryDirectory() as temp:
            env = L.LeanEnv('/unused', [], '', {}, '', temp)
            output = 'FID raw ACCEPT\n' + json.dumps(dict(severity='information', data='FID structured ACCEPT\nFID-DONE'))
            with patch.object(L, 'run_lean', return_value=L.RunResult(0, output, 0, False)):
                verdicts, _ = G.lean_verdicts(env, temp, 'Example', 0, [], temp, 10)
            self.assertEqual(verdicts, dict(structured='ACCEPT'))
        raw = 'x' * 600
        self.assertEqual(L.parse_messages(raw)[0]['data'], 'non-JSON output: ' + raw[:500])

    def test_non_json_stderr_is_raw_output_and_not_a_lean_error(self):
        with tempfile.TemporaryDirectory() as temp:
            env = L.LeanEnv('/unused', [], '', {}, '', temp)
            problem = dict(checker=dict(theorem_fqn='X.goal', files=[], lean_opts=[]))
            output = 'allocator could not reserve memory\n'
            with patch.object(L, 'run_lean', return_value=L.RunResult(1, output, 0, False)):
                result = C.compile_in_sandbox(env, problem, temp, 'theorem goal : True := by trivial',
                                              str(Path(temp)/'check'), 10, SB.Sandbox.fixture())
            self.assertEqual(result['n_errors'], 0)
            self.assertEqual(Path(temp, 'check/proof/compile.log').read_text(), output)
            invalid, fail, error = [], [], []
            C.apply_compile_result(result, {}, invalid, fail, error)
            self.assertEqual((invalid, fail, error), ([], [], ['resource-unavailable']))
            messages = L.parse_messages(output + json.dumps(dict(severity='error', data='Lean error')), structured_only=True)
            self.assertEqual([message['data'] for message in L.errors(messages)], ['Lean error'])
            self.assertEqual(L.parse_messages(output)[0]['data'], 'non-JSON output: ' + output.rstrip('\n'))

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


class TextPolicy(unittest.TestCase):
    def test_policy_loader_rejects_word_patterns_with_nonword_edges(self):
        for word in ['#eval', 'debug.', 'αnative', 'nativeα', '']:
            with self.subTest(word=word), self.assertRaises(ValueError):
                C.text_pattern(dict(text=word, label='bad', boundary='word'))

    def test_policy_loader_rejects_non_ascii_whitespace_and_bad_bounded_edges(self):
        policy = dict(C.TEXT_POLICY, ascii_whitespace=' \u00a0')
        with self.assertRaises(ValueError):
            C.validate_text_policy(policy)
        for boundary, word in [('prefix', '.debug'), ('prefix', ''),
                               ('suffix', '#eval.'), ('suffix', '')]:
            with self.subTest(boundary=boundary, word=word), self.assertRaises(ValueError):
                C.text_pattern(dict(text=word, label='bad', boundary=boundary))

    def test_runner_manifest_is_private(self):
        manifest = Path(C.__file__).parents[2]/'crates/boole-lean-runner/Cargo.toml'
        self.assertIn('publish = false', manifest.read_text())

    def test_current_metadata_contains_every_reported_policy_label(self):
        reported = {rule['label'] for rule in C.TEXT_POLICY['patterns']} | {'forbidden-attribute'}
        self.assertEqual(reported - set(C.FORBIDDEN_LABELS), set())

    def test_public_checker_rejects_invalid_utf8_without_crashing(self):
        from test_zk_registry_package import make_package
        with tempfile.TemporaryDirectory() as temp:
            package, _ = make_package(temp)
            statement = Path(package, 'Statement.lean').read_text()
            proof = Path(temp)/'Proof.lean'
            proof.write_bytes(check_fixtures.submission(statement, '  trivial\n').encode().replace(
                b'  trivial\n', b'  exact \xff\n'))
            env = check_fixtures.fake_env(temp)
            result = C.check(package, str(proof), env, policy_fixture=env.pins())
            self.assertEqual((result['verdict'], result.get('reason')), ('INVALID', 'submission-source-encoding'))

    def test_shared_text_policy_cases(self):
        path = Path(C.__file__).parents[2]/'fixtures/submission-policy/cases-v4.json'
        for case in json.loads(path.read_text())['cases']:
            with self.subTest(case=case['name']):
                expected = [f"{hit['label']} in proof (line {hit['line']} of that part)"
                            + (f" [forbidden word: {hit['word']}]" if hit['word'] not in hit['label']
                               and hit['label'] != 'forbidden-attribute' else '') for hit in case['hits']]
                if case.get('more_lines'):
                    expected[-1] += ' [more offending lines omitted]'
                self.assertEqual(C.forbidden_scan([('proof', case['source'])]), expected)

    def test_public_checker_rejects_forbidden_word_in_every_text_form(self):
        from test_zk_registry_package import make_package
        for source in ['axiom bad : False', '-- axiom', 'let text := "axiom"']:
            with self.subTest(source=source), tempfile.TemporaryDirectory() as temp:
                package, _ = make_package(temp)
                statement = Path(package, 'Statement.lean').read_text()
                proof = Path(temp)/'Proof.lean'
                proof.write_text(check_fixtures.submission(statement, '\n  '+source+'\n  trivial\n'))
                env = check_fixtures.fake_env(temp)
                result = C.check(package, str(proof), env, policy_fixture=env.pins())
                self.assertEqual(result['verdict'], 'INVALID')
                self.assertIn('axiom in proof (line 3 of that part)', result['invalid'])

    def test_sixteen_mib_scan_finishes_under_ten_seconds(self):
        text = ("/- nested /- inner -/ text -/ s!\"a{x}b\" f '' s\n" * 400000)[:16 * 1024 * 1024]
        self.assertEqual(len(text.encode()), 16 * 1024 * 1024)
        started = time.monotonic()
        self.assertEqual(C.forbidden_scan([('proof', text)]), [])
        self.assertLess(time.monotonic() - started, 10)
    def test_attribute_brackets_use_bounded_memory_and_time(self):
        code = """import json,resource,sys,time
sys.path.insert(0,sys.argv[1])
from zk_registry import check as C
unit=sys.argv[2]
text=(unit*((16*1024*1024)//len(unit))) if unit.startswith('@') else '@['+unit*(16*1024*1024)
scale=1024*1024 if sys.platform=='darwin' else 1024
before=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss/scale
started=time.monotonic()
hits=C.forbidden_scan([('proof',text)])
print(json.dumps(dict(hits=hits,seconds=time.monotonic()-started,
    growth_mib=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss/scale-before)))
"""
        import os
        for bracket in ['(', '[', '{', '@[', '@[simp] ']:
            with self.subTest(bracket=bracket):
                result = L.run_process([sys.executable, '-c', code, str(Path(__file__).parent), bracket],
                                       dict(os.environ), str(Path(__file__).parent), 12, 230, poll_s=0.02)
                self.assertFalse(result.memkill, 'attribute scan exceeded the bounded test memory guard')
                self.assertEqual(result.rc, 0, result.out)
                receipt = json.loads(result.out)
                print(json.dumps(dict(unit=bracket, **receipt)), flush=True)
                self.assertEqual(receipt['hits'], [] if bracket == '@[simp] ' else
                                 ['forbidden-attribute in proof (line 1 of that part)'])
                self.assertLess(receipt['seconds'], 10)
                self.assertLess(receipt['growth_mib'], 200)



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
    def test_shared_policy_and_fixture_trigger_both_macos_ci_events(self):
        workflow = (Path(__file__).parents[1]/'.github/workflows/macos-isolation.yml').read_text()
        for path in ['scripts/zk_registry/data/submission_policy_v4.json', 'fixtures/submission-policy/**']:
            self.assertEqual(workflow.count('      - "'+path+'"\n'), 2, path)

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
