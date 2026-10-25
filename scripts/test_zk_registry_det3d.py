"""C8 package budgets must derive from the descriptor's measured own path."""
import copy
import unittest
import json
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import tempfile
import re
from unittest.mock import patch

from zk_registry import fuel_caps as F


class BoundCaps(unittest.TestCase):
    def setUp(self):
        self.reference = dict(path='certified-own-delta/v1', problem='test/positive', environment='a' * 64,
                              statement='b' * 64, reference_export='c' * 64,
                              own_declarations='d' * 64,
                              units=dict(lean=81, nanoda=137), eligible=True)
        self.key = F.digest(self.reference)
        self.authority = dict(version=1, factors=dict(lean=4, nanoda=4),
                              references={self.key: self.reference})
        self.cap = dict(version=1, reference=self.key, problem='test/positive', environment='a' * 64,
                        statement='b' * 64, own_declarations='d' * 64,
                        units=dict(lean=324, nanoda=548))
        self.bound = dict(cap=self.cap, sha256=F.digest(self.cap))

    def test_measured_reference_sets_exact_fourfold_budgets(self):
        result = F.resolve(self.bound, self.authority)
        self.assertEqual(result['units'], dict(lean=324, nanoda=548))
        self.assertEqual(result['reference_export'], 'c' * 64)

    def test_unknown_reference_never_becomes_eligible(self):
        authority = copy.deepcopy(self.authority)
        authority['references'] = {}
        with self.assertRaises(F.FuelCapError):
            F.resolve(self.bound, authority)

    def test_cap_cannot_be_rebound_or_increased(self):
        for key, value in [('environment', 'e' * 64), ('statement', 'e' * 64),
                           ('problem', 'test/unknown'),
                           ('own_declarations', 'e' * 64), ('reference', 'e' * 64),
                           ('units', dict(lean=325, nanoda=548)),
                           ('units', dict(lean=True, nanoda=548))]:
            bound = copy.deepcopy(self.bound)
            bound['cap'][key] = value
            bound['sha256'] = F.digest(bound['cap'])
            with self.subTest(key=key, value=value), self.assertRaises(F.FuelCapError):
                F.resolve(bound, self.authority)
        changed = copy.deepcopy(self.bound)
        changed['cap']['units']['lean'] += 1
        with self.assertRaises(F.FuelCapError):
            F.resolve(changed, self.authority)

    def test_only_eligible_own_path_and_supported_counter_units_qualify(self):
        for field, value in [('eligible', False), ('path', 'full-export/v1'),
                             ('units', dict(lean=0, nanoda=137))]:
            authority = copy.deepcopy(self.authority)
            authority['references'][self.key][field] = value
            with self.subTest(field=field), self.assertRaises(F.FuelCapError):
                F.resolve(self.bound, authority)

    def test_authority_factors_are_fixed_per_kernel(self):
        for factors in [dict(lean=4, nanoda=8), dict(lean=4), dict(lean=True, nanoda=4)]:
            authority = copy.deepcopy(self.authority)
            authority['factors'] = factors
            with self.subTest(factors=factors), self.assertRaises(F.FuelCapError):
                F.resolve(self.bound, authority)

    def test_malformed_and_unbound_fields_do_not_reach_native_limits(self):
        for record in [None, {}, dict(cap={}, sha256='a' * 64),
                       dict(self.bound, extra=True), dict(self.bound, sha256='a' * 64)]:
            with self.subTest(record=record), self.assertRaises(F.FuelCapError):
                F.resolve(record, self.authority)
        malformed = copy.deepcopy(self.bound)
        malformed['cap']['units']['lean'] = float('nan')
        with self.assertRaises(F.FuelCapError):
            F.resolve(malformed, self.authority)

    def test_actual_problem_statement_and_state_bytes_are_bound(self):
        from zk_registry.closure import digest
        with tempfile.TemporaryDirectory() as temporary:
            state = Path(temporary) / ('a' * 64) / 'state.olean'
            state.parent.mkdir()
            state.write_bytes(b'certified state control')
            self.authority['environments'] = {'a' * 64: dict(
                eligible=True, certified_kernels=['lean', 'nanoda'], profiles={
                    'Control-cpu': dict(state_sha256=digest(state), certificate_sha256='e' * 64)})}
            problem = dict(package_id='test/positive', fuel_cap=self.bound)
            with patch.object(F, 'load_authority', return_value=self.authority), \
                    patch.object(F.platform, 'system', return_value='Control'), \
                    patch.object(F.platform, 'machine', return_value='cpu'):
                budget = F.bind(problem, 'b' * 64, temporary)
                self.assertEqual((budget.lean, budget.nanoda), (324, 548))
                for changed, statement in [(dict(problem, package_id='unknown'), 'b' * 64),
                                           (problem, 'f' * 64)]:
                    with self.assertRaises(F.FuelCapError):
                        F.bind(changed, statement, temporary)
                state.write_bytes(b'changed state')
                with self.assertRaises(F.FuelCapError):
                    F.bind(problem, 'b' * 64, temporary)

    def test_real_package_validator_accepts_bound_metadata_and_rejects_bad_units(self):
        from test_zk_registry_package import make_package
        from zk_registry import package as P
        with tempfile.TemporaryDirectory() as temporary:
            directory, problem = make_package(temporary)
            cap = copy.deepcopy(self.bound)
            reference = dict(self.reference, problem=problem['package_id'])
            key = F.digest(reference)
            authority = dict(self.authority, references={key: reference})
            cap['cap'].update(problem=problem['package_id'], reference=key)
            cap['sha256'] = F.digest(cap['cap'])
            problem['fuel_cap'] = cap
            with patch.object(F, 'load_authority', return_value=authority):
                self.assertEqual(P.validate_problem(problem, directory), [])
            for value in (True, 0, -1, 2**64, '324'):
                broken = copy.deepcopy(problem)
                broken['fuel_cap']['cap']['units']['lean'] = value
                with self.subTest(value=value):
                    self.assertTrue(P.validate_problem(broken, directory))
            problem['fuel_cap']['cap']['problem'] = 'x' * 1025
            self.assertIn('$.fuel_cap.cap.problem: longer than 1024',
                          P.validate_problem(problem, directory))


class ComparatorBudget(unittest.TestCase):
    def test_certified_state_cannot_live_beneath_a_submission_writer(self):
        from dataclasses import replace
        from zk_registry import closure as C, safe_fs as FS
        from zk_registry.sandbox import Sandbox
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            writer = root / 'solver-output'
            state = writer / 'cache/state.olean'
            state.parent.mkdir(parents=True)
            state.write_bytes(b'control')
            left, right = root / 'left', root / 'right'
            left.write_bytes(b'sealed')
            right.write_bytes(b'sealed')
            marker = root / 'started'
            binary = root / 'comparator'
            binary.write_text('#!/bin/sh\n: > "' + str(marker) + '"\nexit 70\n')
            binary.chmod(0o755)
            budget = F.Budget(324, 548, 'a' * 64, str(state), C.digest(state), 'b' * 64)
            tools = C.Tools(str(binary), '/usr/bin/true', '/usr/bin/true', '/usr', None)
            boundary = replace(Sandbox.fixture(), exposed_writers={str(writer): FS.root_identity(writer)})
            row = C.compare_exports(str(left), str(right), 'Control.target', tools,
                                    boundary, str(root / 'work'), 5, [], fuel_budget=budget)
            self.assertEqual(row['verdict'], 'ERROR')
            self.assertFalse(marker.exists(), 'an exposed cache must be refused before native execution')
            from zk_registry.error_policy import classify
            self.assertEqual(classify(row['reason'], row['stage']), 'D-node')

    def test_both_caps_and_state_are_used_and_recorded(self):
        from zk_registry import closure as C
        from zk_registry.sandbox import Sandbox
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state, left, right = [root / name for name in ('state', 'left', 'right')]
            for path in (state, left, right):
                path.write_bytes(b'immutable control')
            binary = root / 'comparator'
            binary.write_text('#!/usr/bin/python3\nimport os,sys,json\nfrom pathlib import Path\n'
                              'config=json.loads(Path(sys.argv[1]).read_text())\n'
                              'assert config["lean_kernel"]["aggregate_raw"]==324\n'
                              'assert config["nanoda_kernel"]["aggregate_work"]==548\n'
                              'assert Path(config["certified_base_olean"]).read_bytes()==b"immutable control"\n'
                              'events=[dict(version=1,stage="policy",reason="certified-base-mismatch"),'
                              'dict(version=1,stage="final",rc=2)]\n'
                              'Path(os.environ["COMPARATOR_STATUS_FILE"]).write_text('
                              '"".join(json.dumps(e)+"\\n" for e in events))\nsys.exit(2)\n')
            binary.chmod(0o755)
            budget = F.Budget(324, 548, 'a' * 64, str(state), C.digest(state), 'b' * 64)
            tools = C.Tools(str(binary), '/usr/bin/true', '/usr/bin/true', '/usr', None,
                            lean_replay='pinned control')
            row = C.compare_exports(str(left), str(right), 'Control.target', tools,
                                    Sandbox.fixture(), str(root / 'work'), 5, [], fuel_budget=budget)
            self.assertEqual((row['verdict'], row['reason']), ('INVALID', 'certified-base-mismatch'))
            self.assertEqual(row['fuel_cap']['units'], dict(lean=324, nanoda=548))


class CalibrationEnvelope(unittest.TestCase):
    def row(self, problem, units):
        return dict(problem=problem, status='MEASURED', environment='a' * 64,
                    reference_export='c' * 64,
                    reference=dict(path='certified-own-delta/v1', problem=problem,
                                   environment='a' * 64, statement='b' * 64,
                                   reference_export='c' * 64, own_declarations='d' * 64,
                                   eligible=False, units=dict(lean=units, nanoda=units)),
                    counters=dict(lean=dict(vector='fixed'), nanoda=dict(vector='fixed')),
                    identity=dict(seconds=0.1, peak_rss_mb=500),
                    comparator=dict(verdict='PASS', seconds=0.2, peak_rss_mb=600),
                    measurement={k: dict(seconds=0.5, peak_rss_mib=600) for k in ('lean', 'nanoda')},
                    timing={k: dict(check_ns=100000000) for k in ('lean', 'nanoda')})

    def test_exact_fourfold_caps_require_measured_path_and_fit_with_margin(self):
        from zk_registry import fit_fuel_caps as C
        rows = [self.row('eligible', 100), self.row('large', 1000000000)]
        cohort = dict(complete=2, total=2, rows=rows)
        envs = {'a' * 64: dict(eligible=True, certified_kernels=['lean', 'nanoda'],
                              profiles={'Darwin-arm64': {}, 'Linux-aarch64': {}})}
        result = C.fit(cohort, copy.deepcopy(cohort), envs)
        self.assertEqual(len(result['caps']), 1)
        cap = result['caps'][0]
        self.assertEqual(cap['problem'], 'eligible')
        self.assertEqual(F.resolve(cap['fuel_cap'], result['authority'])['units'], dict(lean=400, nanoda=400))
        self.assertEqual(result['exclusions'][0]['reason'], 'tier-required')
        self.assertFalse(result['authority']['unknown_pool_eligible'])

    def test_mismatched_vectors_and_incomplete_cohorts_never_make_caps(self):
        from zk_registry import fit_fuel_caps as C
        cohort = dict(complete=1, total=1, rows=[self.row('one', 100)])
        bad = copy.deepcopy(cohort)
        bad['rows'][0]['counters']['lean']['vector'] = 'changed'
        with self.assertRaises(ValueError):
            C.fit(cohort, bad, {})
        bad['complete'] = 0
        with self.assertRaises(ValueError):
            C.fit(cohort, bad, {})

    def test_upper_fit_covers_every_observation_with_nonnegative_intercept(self):
        from zk_registry import fit_fuel_caps as C
        points = [(100, 1), (200, 3), (300, 4.5)]
        fitted = C.upper_fit(points)
        self.assertGreaterEqual(fitted['intercept'], 0)
        for x, y in points:
            self.assertGreaterEqual(fitted['intercept'] + fitted['slope'] * x + 1e-12, y)

    def test_actual_comparator_envelope_and_acceptance_are_required(self):
        from zk_registry import fit_fuel_caps as C
        envs = {'a' * 64: dict(eligible=True, certified_kernels=['lean', 'nanoda'],
                              profiles={'Darwin-arm64': {}, 'Linux-aarch64': {}})}
        for changed in [None, dict(verdict='ERROR', seconds=1, peak_rss_mb=600),
                        dict(verdict='PASS', seconds=1800, peak_rss_mb=600),
                        dict(verdict='PASS', seconds=1, peak_rss_mb=11000)]:
            row = self.row('one', 100)
            row['comparator'] = changed
            cohort = dict(complete=1, total=1, rows=[row])
            with self.subTest(comparator=changed):
                result = C.fit(cohort, copy.deepcopy(cohort), envs)
                self.assertFalse(result['authority']['references'])
                self.assertEqual(len(result['exclusions']), 1)


class CounterCI(unittest.TestCase):
    def test_corpus_manifest_opens_on_a_case_sensitive_filesystem(self):
        from zk_registry import counter_identity as identity

        class ExactCasePath(type(Path())):
            def open(self, *args, **kwargs):
                if self.name not in [p.name for p in self.parent.iterdir()]:
                    raise FileNotFoundError(str(self))
                return super().open(*args, **kwargs)

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            # A malformed manifest must be read and rejected before any tool
            # executes. Linux must reach this check using the tracked filename.
            (root / 'manifest.json').write_text(json.dumps(dict(counts={}, units=[])))
            with self.assertRaisesRegex(ValueError, 'fixed 32-unit public corpus'):
                identity.corpus(ExactCasePath(root), root / 'out', None, None)

    def test_x86_job_compares_fixed_goldens_instead_of_recording_its_own(self):
        root = Path(__file__).resolve().parent.parent
        workflow = (root / '.github/workflows/det3-counter-identity.yml').read_text()
        manifest = json.loads((root / 'fixtures/public-counter-identity-v1/manifest.json').read_text())
        self.assertIn('det3-counter-identity-x86_64:', workflow)
        self.assertIn('runs-on: ubuntu-latest', workflow)
        self.assertIn('test "$(uname -m)" = x86_64', workflow)
        self.assertIn('--expected fixtures/public-counter-identity-v1/GOLDEN-aarch64.json', workflow)
        self.assertNotIn('--record', workflow)
        self.assertNotIn('continue-on-error', workflow)
        self.assertIn('ref: ' + manifest['lean_source_commit'], workflow)
        self.assertEqual(manifest['counts'], {family: 2 for family in manifest['counts']})
        self.assertEqual(len(manifest['units']), 32)
        actions = re.findall(r'^\s*(?:- )?uses:\s*(\S+)', workflow, re.MULTILINE)
        self.assertTrue(actions)
        for action in actions:
            self.assertRegex(action, r'@[0-9a-f]{40}$')


if __name__ == '__main__':
    unittest.main()
