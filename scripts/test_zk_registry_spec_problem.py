"""Specification package identity, acceptance evidence and file-binding regressions."""

import copy
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import package as P, spec_problem as S


def request():
    """A synthetic public-standard request; no upstream defect fixture."""
    digest = 'ab' * 32
    return dict(family='reduce', standard=dict(tier='T1', source='synthetic residue definition', sha256=digest),
                parameters=dict(modulus='Nat', width='Nat'),
                reference=dict(name='synthetic reference', version='1', sha256=digest),
                mutants=dict(count=12, sha256=digest), protocol=dict(version='D/M/C', sha256=digest),
                files=[], created_utc='2026-10-10T00:00:00Z')


class SpecProblemTests(unittest.TestCase):
    def test_ready_metadata_does_not_replace_parameter_and_mutant_content(self):
        """A complete-looking OPEN manifest cannot qualify empty parameter/mutant JSON files."""
        with tempfile.TemporaryDirectory() as root:
            data = request()
            for key in ('standard', 'parameters', 'reference', 'mutants', 'protocol'):
                path = Path(root, key + '.json')
                path.write_text('{}')
                digest = P.sha256_file(str(path))
                data[key].update(file=path.name, sha256=digest)
                data['files'].append(dict(path=path.name, sha256=digest, role=key))
            code = Path(root, 'mutants.py')
            code.write_text('')
            data['parameters']['scope_complete'] = True
            data['reference']['executable'] = True
            data['mutants'].update(family=data['family'], code=code.name,
                                   code_sha256=P.sha256_file(str(code)))
            data['files'].append(dict(path=code.name, sha256=P.sha256_file(str(code)), role='mutants'))
            problem = S.record(data)
            self.assertEqual(S.authoring_prerequisites(problem), [])
            self.assertIn('parameter-scope-content', S.authoring_prerequisites(problem, root))
            self.assertIn('function-specific-mutant-content', S.authoring_prerequisites(problem, root))
            scope = dict(function=problem['family'], count=1, units=[dict(
                field_parameters=dict(modulus='7', bits_per_limb=4, nb_limbs=1),
                reference_family=problem['family'], reference_domain='canonical', reference_observation='residue')])
            parameter_file = Path(root, problem['parameters']['file'])
            parameter_file.write_text(json.dumps(scope))
            digest = P.sha256_file(str(parameter_file))
            problem['parameters']['sha256'] = digest
            next(row for row in problem['files'] if row['path'] == parameter_file.name)['sha256'] = digest
            self.assertIn('parameter-scope-content', S.authoring_prerequisites(problem, root))

    def test_unbound_inventory_is_not_a_qualified_authorship_request(self):
        """The allowed pending public window is distinct from missing request prerequisites."""
        problem = S.record(request())
        self.assertIn('standard-file-binding', S.authoring_prerequisites(problem))
        self.assertIn('parameter-scope-file-binding', S.authoring_prerequisites(problem))
        self.assertIn('independent-executable-reference', S.authoring_prerequisites(problem))
        for key in ('standard', 'parameters', 'reference', 'mutants', 'protocol'):
            problem[key]['file'] = key + '.json'
            problem[key]['sha256'] = 'ab' * 32
            problem['files'].append(dict(path=key + '.json', role=key, sha256=problem[key]['sha256']))
        problem['parameters']['scope_complete'] = True
        problem['reference']['executable'] = True
        problem['mutants']['family'] = problem['family']
        problem['mutants']['code'] = 'mutants.py'
        problem['mutants']['code_sha256'] = 'ab' * 32
        problem['files'].append(dict(path='mutants.py', role='mutants', sha256='ab' * 32))
        self.assertEqual(S.authoring_prerequisites(problem), [])
        problem['standard']['authority_pending'] = True
        problem['reference']['executable_reference_binding'] = 'pending'
        problem['mutants']['fixed_protocol_only'] = True
        self.assertEqual(S.authoring_prerequisites(problem),
                         ['authoritative-public-standard', 'independent-executable-reference',
                          'function-specific-fixed-mutants'])
        problem['mutants']['fixed_protocol_only'] = False
        problem['mutants']['family'] = 'unrelated-family'
        self.assertIn('function-specific-fixed-mutants', S.authoring_prerequisites(problem))

    @patch.object(S, 'historical_binding_errors', return_value=[])
    def test_accepted_gate_content_not_just_pass_metadata(self, _history):
        """Exercise the gate parser with a synthetic trusted-history mock, never real local acceptance."""
        import json
        with tempfile.TemporaryDirectory() as root:
            data = request()
            data['protocol']['version'] = 'Task M D/M/C frozen'
            data['files'] = []
            for key in ('reference', 'mutants', 'protocol'):
                data[key]['file'] = key + '.json'
                path = Path(root, key + '.json')
                path.write_text('{}')
                data[key]['sha256'] = P.sha256_file(str(path))
                data['files'].append(dict(path=path.name, role=key, sha256=data[key]['sha256']))
            Path(root, 'Candidate.lean').write_text('namespace Candidate\nend Candidate\n')
            digest = P.sha256_file(str(Path(root, 'Candidate.lean')))
            data['files'].append(dict(path='Candidate.lean', role='specification', sha256=digest))
            accepted = dict(file='Candidate.lean', module='Candidate', sha256=digest,
                            tested_parameters=[], gate_evidence={})
            entry = dict(unit='M01', D='PASS', seeded_rows=10000, M_killed=12,
                         evaluation='synthetic.json', seed_sha256='ab' * 32,
                         mutant_disagreements=[dict(index=i, input={}, Lean=['0'], mutant=['1']) for i in range(12)])
            records = dict(D=dict(status='PASS', entries=[entry]), M=dict(status='PASS', entries=[entry]),
                           C=dict(status='NO_COUNTEREXAMPLE', agent='independent-fixture',
                                  start_utc='2026-10-10T00:00:00Z', end_utc='2026-10-10T00:01:00Z', input_count=1))
            for name, record in records.items():
                path = Path(root, name + '.json')
                path.write_text(json.dumps(record))
                digest = P.sha256_file(str(path))
                data['files'].append(dict(path=path.name, role='gate', sha256=digest))
                accepted['gate_evidence'][name] = dict(status='PASS', file=path.name, sha256=digest)
            problem = S.record(data, accepted)
            self.assertEqual(P.validate_problem(problem, root), [])
            for name in ('D', 'M', 'C'):
                original = Path(root, name + '.json').read_text()
                Path(root, name + '.json').write_text('null')
                digest = P.sha256_file(str(Path(root, name + '.json')))
                changed = copy.deepcopy(problem)
                next(row for row in changed['files'] if row['path'] == name + '.json')['sha256'] = digest
                changed['specification']['gate_evidence'][name]['sha256'] = digest
                self.assertTrue(P.validate_problem(changed, root))
                Path(root, name + '.json').write_text(original)
            mutations = [('C', lambda record: record.update(agent={})),
                         ('C', lambda record: record.update(start_utc=1)),
                         ('C', lambda record: record.update(input_count=True)),
                         ('C', lambda record: record.update(start_utc='2026-10-10T00:02:00Z')),
                         ('D', lambda record: record['entries'][0].pop('evaluation')),
                         ('M', lambda record: record['entries'][0].update(mutant_disagreements=[]))]
            for name, mutate in mutations:
                path = Path(root, name + '.json')
                original = path.read_text()
                record = json.loads(original)
                mutate(record)
                path.write_text(json.dumps(record))
                digest = P.sha256_file(str(path))
                changed = copy.deepcopy(problem)
                next(row for row in changed['files'] if row['path'] == name + '.json')['sha256'] = digest
                changed['specification']['gate_evidence'][name]['sha256'] = digest
                self.assertTrue(P.validate_problem(changed, root), name)
                path.write_text(original)

    def test_approved_history_rejects_rehashed_replacement_specs_and_gates(self):
        """Only retained approved bytes may enter C; arbitrary PASS summaries cannot approve a new spec."""
        import json
        catalogue = json.loads((Path(S.__file__).parent / 'data/spec_history_m_2026_10_10.json').read_text())
        expected = catalogue['specifications']['reduce']
        problem = dict(family='reduce',
                       specification=dict(sha256=expected['specification_sha256'],
                                          tested_parameters=expected['tested_parameters'],
                                          gate_evidence={key: dict(sha256=value)
                                                         for key, value in expected['gate_sha256'].items()}),
                       reference=dict(sha256=expected['reference_sha256']),
                       mutants=dict(sha256=expected['mutants_sha256']),
                       protocol=dict(sha256=expected['protocol_sha256']))
        self.assertEqual(S.historical_binding_errors(problem), [])
        for section, key in [('specification', 'sha256'), ('reference', 'sha256'),
                             ('mutants', 'sha256'), ('protocol', 'sha256')]:
            changed = copy.deepcopy(problem)
            changed[section][key] = 'ff' * 32
            self.assertTrue(S.historical_binding_errors(changed))
        for key in ('D', 'M', 'C'):
            changed = copy.deepcopy(problem)
            changed['specification']['gate_evidence'][key]['sha256'] = 'ff' * 32
            self.assertTrue(S.historical_binding_errors(changed))
        changed = copy.deepcopy(problem)
        changed['specification']['tested_parameters'] = []
        self.assertTrue(S.historical_binding_errors(changed))
        changed['family'] = 'unapproved'
        self.assertTrue(S.historical_binding_errors(changed))

    def test_open_does_not_claim_a_specification_or_checker(self):
        """Missing accepted text remains an OPEN S problem, never a proof task."""
        problem = S.record(request())
        self.assertEqual(P.validate_problem(problem), [])
        self.assertEqual(problem['status'], 'OPEN')
        self.assertNotIn('specification', problem)
        self.assertFalse(P.has_statement(problem))

    def test_identity_binds_all_request_inputs(self):
        """Standard, reference, parameters, mutants and protocol all affect identity."""
        original = request()
        ident = S.record(original)['package_id']
        for key in ('standard', 'reference', 'mutants', 'protocol'):
            altered = copy.deepcopy(original)
            altered[key]['sha256'] = 'cd' * 32
            self.assertNotEqual(S.record(altered)['package_id'], ident)
        altered = copy.deepcopy(original)
        altered['parameters']['width'] = 'fixed 64'
        self.assertNotEqual(S.record(altered)['package_id'], ident)

    def test_acceptance_requires_all_gates_and_pending_public_window(self):
        """Local study evidence is not a completed public challenge window."""
        accepted = dict(file='Candidate.lean', module='Candidate', sha256='cd' * 32,
                        gate_evidence={name: dict(status='PASS', file=name + '.json', sha256='ef' * 32)
                                       for name in ('D', 'M', 'C')}, tested_parameters=[])
        data = request()
        data['files'] = [dict(path='Candidate.lean', sha256='cd' * 32, role='specification')]
        data['files'] += [dict(path=name + '.json', sha256='ef' * 32, role='gate') for name in ('D', 'M', 'C')]
        problem = S.record(data, accepted)
        self.assertEqual(P.validate_problem(problem), [])
        for name in ('D', 'M', 'C'):
            bad = copy.deepcopy(problem)
            bad['specification']['gate_evidence'][name]['status'] = 'FAIL'
            self.assertTrue(P.validate_problem(bad))
        bad = copy.deepcopy(problem)
        bad['public_challenge']['status'] = 'passed'
        self.assertTrue(P.validate_problem(bad))

    def test_accepted_local_requires_a_complete_file_closure(self):
        """A status label or a missing gate manifest is not local acceptance."""
        problem = S.record(request())
        problem['status'] = 'ACCEPTED-LOCAL'
        self.assertTrue(P.validate_problem(problem))

    def test_manifest_digest_and_safe_paths(self):
        """Package bytes cannot escape the directory or change after binding."""
        with tempfile.TemporaryDirectory() as root:
            path = Path(root, 'reference.txt')
            path.write_text('public fixture\n')
            data = request()
            data['files'] = [dict(path='reference.txt', sha256=P.sha256_file(str(path)), role='reference')]
            problem = S.record(data)
            self.assertEqual(P.validate_problem(problem, root), [])
            path.write_text('changed\n')
            self.assertTrue(P.validate_problem(problem, root))
            problem['files'][0]['path'] = '../outside'
            self.assertTrue(P.validate_problem(problem))

    def test_package_digest_is_canonical_and_acceptance_sensitive(self):
        """The linked S digest binds status and all evidence, not just an operation name."""
        problem = S.record(request())
        self.assertEqual(S.package_digest(problem), S.package_digest(dict(reversed(list(problem.items())))))
        altered = copy.deepcopy(problem)
        altered['created_utc'] = '2026-10-11T00:00:00Z'
        self.assertNotEqual(S.package_digest(problem), S.package_digest(altered))


if __name__ == '__main__':
    unittest.main()
