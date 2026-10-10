"""Closed-local registry service contracts; synthetic fixtures only in offline tests."""

from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
import subprocess
import shutil

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import service as S
from zk_registry import package as P
from zk_registry import ratchet_halo2 as H
from test_zk_registry_ratchet import offline_problem
from test_zk_registry_ratchet_native import problem_fixture


class CatalogTests(unittest.TestCase):
    def test_adapter_binds_candidate_model_digest_from_generated_checker_manifest(self):
        from zk_registry import service_check as SC
        with tempfile.TemporaryDirectory() as scratch:
            package = Path(scratch) / 'generated'
            package.mkdir()
            model = package / 'Candidate.lean'
            model.write_text('-- synthetic generated candidate model\n')
            model_sha = P.sha256_file(str(model))
            (package / 'problem.json').write_bytes(S.canonical(dict(checker=dict(files=[
                dict(path='Reference.lean', role='import', sha256='ab' * 32),
                dict(path='Candidate.lean', role='import', sha256=model_sha)]))))
            problem = dict(reference=dict(model=dict(file='Reference.lean')))
            report = dict(candidate=dict(source_sha256='cd' * 32, r1cs_sha256='ef' * 32),
                          checker=dict(package=str(package)))
            artifact = SC.ratchet_artifact(problem, report)
            self.assertEqual(artifact['model_sha256'], model_sha)
            self.assertEqual(artifact['source_sha256'], report['candidate']['source_sha256'])
            model.write_text('-- tampered candidate model\n')
            with self.assertRaisesRegex(S.RegistryError, 'candidate model'):
                SC.ratchet_artifact(problem, report)

    def test_native_next_rung_keeps_reference_immutable_and_is_submittable(self):
        for family in ('zokrates', 'halo2'):
            with tempfile.TemporaryDirectory() as scratch:
                directory, problem = problem_fixture(scratch, family)
                changed = copy.deepcopy(problem)
                changed['record']['rung'] = 1
                if family == 'zokrates':
                    changed['record'].update(nonlinear=0, linear=0, total=0, r1cs_sha256='de' * 32)
                else:
                    changed['record']['domain_rows'] //= 2
                    changed['record']['priced'] = H.priced(changed['record'])
                self.assertEqual(P.validate_problem(changed, directory), [])
                self.assertEqual(changed['reference'], problem['reference'])
                changed['record']['rung'] = 0
                self.assertTrue(P.validate_problem(changed, directory))

    def test_native_later_records_reject_equal_worse_unknown_or_bad_typed_costs(self):
        for family in ('zokrates', 'halo2'):
            with tempfile.TemporaryDirectory() as scratch:
                directory, problem = problem_fixture(scratch, family)
                for mutation in ('equal', 'worse', 'unknown', 'type', 'reference-file'):
                    changed = copy.deepcopy(problem)
                    changed['record']['rung'] = 1
                    key = 'nonlinear' if family == 'zokrates' else 'domain_rows'
                    if mutation == 'worse':
                        changed['record'][key] *= 2
                    elif mutation == 'unknown':
                        changed['record']['unsupported-cost'] = 0
                    elif mutation == 'type':
                        changed['record'][key] = True
                    elif mutation == 'reference-file':
                        changed['reference']['model']['sha256'] = 'ee' * 32
                    self.assertTrue(P.validate_problem(changed, directory), (family, mutation))

    def test_nonstandard_json_numbers_and_duplicate_keys_are_rejected(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'input.json'
            for text in ('{"value":NaN}', '{"value":Infinity}', '{"value":-Infinity}', '{"value":1,"value":2}'):
                path.write_text(text)
                with self.assertRaises(S.RegistryError):
                    S.read_json(path)

    def test_catalog_validates_public_reference_and_excludes_private_items(self):
        with tempfile.TemporaryDirectory() as scratch:
            problem_dir, problem = offline_problem(scratch)
            private = Path(scratch) / 'private'
            private.mkdir()
            (private / 'problem.json').write_text(json.dumps(dict(
                schema_version=P.SCHEMA_VERSION, status='DET-FALSE-CANDIDATE', package_id='private-id')))
            catalog = S.build_catalog([problem_dir, str(private)])
            self.assertEqual([row['id'] for row in catalog['entries']], [problem['package_id']])
            self.assertEqual(catalog['entries'][0]['kind'], 'R')
            self.assertEqual(catalog['excluded'], {'private-or-nonissuable': 1})
            self.assertNotIn('private-id', json.dumps(catalog))
            model = Path(problem_dir) / problem['reference']['model']['file']
            model.write_text(model.read_text() + '\n-- changed\n')
            with self.assertRaisesRegex(S.RegistryError, 'invalid package'):
                S.build_catalog([problem_dir])

    def test_cli_builds_lists_and_exports_only_public_request_files(self):
        with tempfile.TemporaryDirectory() as scratch:
            problem_dir, problem = offline_problem(scratch)
            store = str(Path(scratch) / 'store')
            output = str(Path(scratch) / 'public')
            base = [sys.executable, '-m', 'zk_registry.service', '--store', store]
            env = dict(os.environ, PYTHONPATH=str(Path(__file__).resolve().parent), PYTHONDONTWRITEBYTECODE='1')
            built = subprocess.run(base + ['catalog', 'build', '--root', problem_dir],
                                   env=env, capture_output=True, text=True)
            self.assertEqual(built.returncode, 0, built.stderr)
            listed = subprocess.run(base + ['catalog', 'list'], env=env, capture_output=True, text=True)
            self.assertEqual(json.loads(listed.stdout)[0]['id'], problem['package_id'])
            exported = subprocess.run(base + ['export-public', '--out', output],
                                      env=env, capture_output=True, text=True)
            self.assertEqual(exported.returncode, 0, exported.stderr)
            catalog = json.loads((Path(output) / 'catalog.json').read_text())
            self.assertEqual(catalog[0]['id'], problem['package_id'])
            names = [path.name for path in Path(output).rglob('*') if path.is_file()]
            self.assertIn('Request.json', names)
            self.assertNotIn('events.jsonl', names)
            self.assertNotIn('Solution.lean', names)


class IntakeTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        problem_dir, self.problem = offline_problem(self.scratch.name)
        self.problem_id = self.problem['package_id']
        self.submission = Path(self.scratch.name) / 'answer'
        self.submission.mkdir()
        (self.submission / 'Solution.lean').write_text('synthetic checker-boundary answer\n')
        self.submission_sha = S.submission_digest(str(self.submission))
        self.root = Path(self.scratch.name) / 'store'
        self.registry = S.Registry.create(str(self.root), S.build_catalog([problem_dir]), reveal_window=10,
                                          verifier=self.verify)

    @staticmethod
    def verify(problem_dir, submission_dir, work_dir):
        """Offline external-checker boundary; production/CLI cannot select this test double."""
        return dict(verdict='PASS', record=dict(nonlinear=1, linear=0, total=1,
                    r1cs_sha256='cd' * 32, model_sha256='ef' * 32, source_sha256='ab' * 32))

    def commit(self, solver='alice', salt='first', time=1):
        value = S.commitment(self.problem_id, solver, self.submission_sha, salt)
        return self.registry.commit(self.problem_id, solver, value, time)

    def test_commit_reveal_binds_solver_problem_salt_and_updates_record(self):
        committed = self.commit()
        for solver, salt in [('alice', 'wrong'), ('bob', 'first')]:
            rejected = self.registry.reveal(committed['sequence'], self.problem_id, solver,
                                            str(self.submission), salt, 2)
            self.assertEqual(rejected['outcome'], 'REJECTED')
            self.assertEqual(rejected['reason'], 'commitment-mismatch')
        accepted = self.registry.reveal(committed['sequence'], self.problem_id, 'alice',
                                        str(self.submission), 'first', 3)
        self.assertEqual(accepted['outcome'], 'ACCEPTED')
        state = self.registry.status()
        self.assertEqual(state['problems'][self.problem_id]['status'], 'ACCEPTED')
        next_id = self.problem_id + '@1'
        self.assertEqual(state['problems'][next_id]['status'], 'OPEN')
        self.assertEqual(state['problems'][next_id]['record']['nonlinear'], 1)
        self.assertEqual(state['problems'][next_id]['root_id'], self.problem_id)

    def test_independent_replay_rechecks_verdicts_and_reproduces_exact_state(self):
        committed = self.commit()
        self.registry.reveal(committed['sequence'], self.problem_id, 'alice', str(self.submission), 'first', 2)
        target = Path(self.scratch.name) / 'replica'
        replica = self.registry.replay(str(target), verifier=self.verify)
        self.assertEqual(S.canonical(replica.status()), S.canonical(self.registry.status()))
        self.assertEqual(replica.log.read_bytes(), self.registry.log.read_bytes())
        rejecting = lambda *_: dict(verdict='INVALID', reason='invalid-proof')
        with self.assertRaisesRegex(S.RegistryError, 'replayed verdict differs'):
            self.registry.replay(str(Path(self.scratch.name) / 'disagreeing'), verifier=rejecting)

    def test_earliest_valid_commit_wins_even_when_later_reveal_arrives_first(self):
        early = self.commit()
        late = self.commit(solver='bob', salt='later', time=2)
        pending = self.registry.reveal(late['sequence'], self.problem_id, 'bob',
                                        str(self.submission), 'later', 3)
        self.assertEqual(pending['outcome'], 'PENDING')
        self.assertEqual(self.registry.status()['problems'][self.problem_id]['status'], 'OPEN')
        accepted = self.registry.reveal(early['sequence'], self.problem_id, 'alice',
                                         str(self.submission), 'first', 4)
        self.assertEqual(accepted['outcome'], 'ACCEPTED')
        state = self.registry.status()
        self.assertEqual(state['problems'][self.problem_id]['winner'], early['sequence'])
        self.assertEqual(state['commits'][str(late['sequence'])]['status'], 'DUPLICATE')

    def test_expiry_unblocks_pending_reveal_and_rejects_exact_deadline(self):
        early = self.commit()
        late = self.commit(solver='bob', salt='later', time=2)
        self.registry.reveal(late['sequence'], self.problem_id, 'bob', str(self.submission), 'later', 3)
        state = self.registry.advance(11)
        self.assertEqual(state['commits'][str(late['sequence'])]['status'], 'ACCEPTED')
        rejected = self.registry.reveal(early['sequence'], self.problem_id, 'alice',
                                        str(self.submission), 'first', 11)
        self.assertEqual(rejected['reason'], 'expired-window')
        with self.assertRaisesRegex(S.RegistryError, 'backwards'):
            self.registry.advance(10)

    def test_wrong_problem_and_mutated_submission_never_consume_commit(self):
        committed = self.commit()
        wrong = self.registry.reveal(committed['sequence'], 'different/problem', 'alice',
                                     str(self.submission), 'first', 2)
        self.assertEqual(wrong['reason'], 'commitment-mismatch')
        (self.submission / 'Solution.lean').write_text('changed answer\n')
        changed = self.registry.reveal(committed['sequence'], self.problem_id, 'alice',
                                       str(self.submission), 'first', 3)
        self.assertEqual(changed['reason'], 'commitment-mismatch')
        self.assertEqual(self.registry.status()['commits'][str(committed['sequence'])]['status'], 'COMMITTED')

    def test_a_next_rung_accepts_a_new_bound_record_without_overwriting_history(self):
        first = self.commit()
        self.registry.reveal(first['sequence'], self.problem_id, 'alice', str(self.submission), 'first', 2)
        next_id = self.problem_id + '@1'
        (self.submission / 'Solution.lean').write_text('second independently checked answer\n')

        def second_check(package_dir, submission_dir, work_dir):
            record = S.read_json(Path(package_dir) / 'problem.json')['record']
            self.assertEqual(record['nonlinear'], 1)
            return dict(verdict='PASS', record=dict(nonlinear=0, linear=0, total=0))

        self.registry.verifier = second_check
        value = S.commitment(next_id, 'alice', S.submission_digest(str(self.submission)), 'second')
        second = self.registry.commit(next_id, 'alice', value, 3)
        result = self.registry.reveal(second['sequence'], next_id, 'alice', str(self.submission), 'second', 4)
        self.assertEqual(result['outcome'], 'ACCEPTED')
        state = self.registry.status()
        self.assertEqual(state['problems'][self.problem_id]['accepted_record']['nonlinear'], 1)
        self.assertEqual(state['problems'][next_id]['accepted_record']['nonlinear'], 0)
        self.assertEqual(state['problems'][self.problem_id + '@2']['status'], 'OPEN')

    def test_rehashed_invalid_catalog_windows_and_duplicate_ids_fail_replay(self):
        events = [json.loads(line) for line in self.registry.log.read_bytes().splitlines()]
        original = copy.deepcopy(events[0])
        mutations = [lambda payload: payload.update(reveal_window=0),
                     lambda payload: payload.update(challenge_window=True),
                     lambda payload: payload['entries'].append(copy.deepcopy(payload['entries'][0]))]
        for index, mutate in enumerate(mutations):
            event = copy.deepcopy(original)
            mutate(event['payload'])
            event.pop('hash')
            event['hash'] = S.event_hash(event)
            self.registry.log.write_bytes(S.canonical(event) + b'\n')
            with self.assertRaisesRegex(S.RegistryError, 'catalog'):
                self.registry.replay(str(Path(self.scratch.name) / ('invalid-replica-' + str(index))))
        self.registry.log.write_bytes(S.canonical(original) + b'\n')

    def test_reveal_duplicate_does_not_reaccept_and_log_tampering_fails_closed(self):
        early = self.commit()
        late = self.commit(solver='bob', salt='later', time=2)
        self.registry.reveal(early['sequence'], self.problem_id, 'alice', str(self.submission), 'first', 3)
        duplicate = self.registry.reveal(late['sequence'], self.problem_id, 'bob',
                                          str(self.submission), 'later', 4)
        self.assertEqual(duplicate['outcome'], 'DUPLICATE')
        self.assertEqual(len(self.registry.status()['problems']), 2)
        log = self.registry.log.read_bytes()
        self.registry.log.write_bytes(log.replace(b'"solver":"bob"', b'"solver":"eve"', 1))
        with self.assertRaises(S.RegistryError):
            self.registry.status()
        self.registry.log.write_bytes(log[:-1])
        with self.assertRaisesRegex(S.RegistryError, 'incomplete'):
            self.registry.status()

    def test_invalid_earlier_proof_unblocks_valid_later_reveal(self):
        early = self.commit()
        late = self.commit(solver='bob', salt='later', time=2)
        pending = self.registry.reveal(late['sequence'], self.problem_id, 'bob',
                                        str(self.submission), 'later', 3)
        self.assertEqual(pending['outcome'], 'PENDING')
        self.registry.verifier = lambda *_: dict(verdict='INVALID', reason='invalid-proof')
        rejected = self.registry.reveal(early['sequence'], self.problem_id, 'alice',
                                         str(self.submission), 'first', 4)
        self.assertEqual(rejected['outcome'], 'REJECTED')
        state = self.registry.status()
        self.assertEqual(state['problems'][self.problem_id]['winner'], late['sequence'])
        self.assertEqual(state['commits'][str(early['sequence'])]['verdict']['reason'], 'invalid-proof')

    def test_replay_rejects_corrupt_content_object_and_preserves_source_log(self):
        committed = self.commit()
        self.registry.reveal(committed['sequence'], self.problem_id, 'alice', str(self.submission), 'first', 2)
        original_log = self.registry.log.read_bytes()
        object_sha = S.file_manifest(str(self.submission))[0]['sha256']
        (self.root / 'objects' / object_sha).write_text('corrupted proof object\n')
        with self.assertRaisesRegex(S.RegistryError, 'content object corrupted'):
            self.registry.replay(str(Path(self.scratch.name) / 'corrupt-replica'), verifier=self.verify)
        self.assertEqual(self.registry.log.read_bytes(), original_log)

    def test_simultaneous_cli_writers_get_unique_durable_sequences(self):
        base = [sys.executable, '-m', 'zk_registry.service', '--store', str(self.root), '--time', '1',
                'commit', '--problem', self.problem_id, '--commitment', 'ab' * 32, '--solver']
        env = dict(os.environ, PYTHONPATH=str(Path(__file__).resolve().parent), PYTHONDONTWRITEBYTECODE='1')
        children = [subprocess.Popen(base + [solver], env=env, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, text=True) for solver in ('alice', 'bob', 'carol')]
        sequences = []
        for child in children:
            stdout, stderr = child.communicate(timeout=10)
            self.assertEqual(child.returncode, 0, stderr)
            sequences.append(json.loads(stdout)['sequence'])
        self.assertEqual(sorted(sequences), [1, 2, 3])
        self.assertEqual(len(self.registry.status()['commits']), 3)

    def test_symlink_and_unsafe_manifest_paths_are_not_admitted(self):
        (self.submission / 'link.lean').symlink_to(self.submission / 'Solution.lean')
        with self.assertRaisesRegex(S.RegistryError, 'symbolic links'):
            S.submission_digest(str(self.submission))
        for path in ('../Solution.lean', '/absolute', 'dir//file', 'dir/./file', 'dir\\file'):
            with self.assertRaises(S.RegistryError):
                S.safe_relative(path)


@unittest.skipUnless(os.environ.get('ZK_REGISTRY_SERVICE_LIVE_FIXTURES'),
                     'opt-in existing positive R/C artifacts and pinned native/Lean tools required')
class LiveAcceptanceTests(unittest.TestCase):
    def test_existing_r_c_proofs_negative_controls_and_rechecked_replay(self):
        from zk_registry.service_check import ProductionVerifier
        fixtures = S.read_json(Path(os.environ['ZK_REGISTRY_SERVICE_LIVE_FIXTURES']))
        work = Path(fixtures['work_root'])
        work.mkdir(parents=True, exist_ok=True)
        attempt = Path(tempfile.mkdtemp(prefix='attempt-', dir=work))
        verifier = ProductionVerifier(S.read_json(Path(fixtures['config'])))
        cases = fixtures['cases']
        registry = S.Registry.create(str(attempt / 'store'), S.build_catalog([row['package'] for row in cases]),
                                      reveal_window=3600, verifier=verifier)
        logical_time = 1
        results = []
        for case in cases:
            problem = S.read_json(Path(case['package']) / 'problem.json')
            ident = problem['package_id']
            original_sha = P.sha256_file(case['proof'])
            if case['kind'] == 'C':
                for mode in ('sorry', 'changed-statement'):
                    submission = attempt / mode
                    submission.mkdir()
                    statement = problem['statement']['text']
                    if mode == 'changed-statement':
                        statement = statement.replace('M13.Soundness', 'True')
                    (submission / 'Solution.lean').write_text(statement)
                    value = S.commitment(ident, mode, S.submission_digest(str(submission)), 'local-salt')
                    committed = registry.commit(ident, mode, value, logical_time)
                    logical_time += 1
                    result = registry.reveal(committed['sequence'], ident, mode,
                                             str(submission), 'local-salt', logical_time)
                    logical_time += 1
                    results.append(dict(kind='C', control=mode, **result))
                    self.assertEqual(result['outcome'], 'REJECTED', result)
                    self.assertEqual(result['reason'], 'invalid-proof', result)
            submission = attempt / ('accepted-' + case['kind'])
            submission.mkdir()
            shutil.copyfile(case['proof'], submission / 'Solution.lean')
            if case.get('candidate'):
                source = Path(case['candidate'])
                shutil.copyfile(source, submission / 'Candidate.go')
            value = S.commitment(ident, 'known-' + case['kind'], S.submission_digest(str(submission)), 'local-salt')
            committed = registry.commit(ident, 'known-' + case['kind'], value, logical_time)
            logical_time += 1
            result = registry.reveal(committed['sequence'], ident, 'known-' + case['kind'],
                                     str(submission), 'local-salt', logical_time)
            logical_time += 1
            results.append(dict(kind=case['kind'], control='known-positive', **result))
            (attempt / 'RESULTS.json').write_bytes(S.canonical(results) + b'\n')
            self.assertEqual(result['outcome'], 'ACCEPTED', result)
            self.assertEqual(P.sha256_file(case['proof']), original_sha)
            state = registry.status()
            expected = 'VERIFIED' if case['kind'] == 'C' else 'ACCEPTED'
            self.assertEqual(state['problems'][ident]['status'], expected)
        replica = registry.replay(str(attempt / 'replica'), verifier=verifier)
        self.assertEqual(S.canonical(replica.status()), S.canonical(registry.status()))
        (attempt / 'REPLAY.json').write_bytes(S.canonical(dict(
            byte_identical=True, state_sha256=S.digest(replica.status()),
            events_sha256=P.sha256_file(str(replica.log)))) + b'\n')


if __name__ == '__main__':
    unittest.main()
