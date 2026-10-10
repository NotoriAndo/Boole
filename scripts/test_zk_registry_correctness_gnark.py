"""Correctness contracts bind all original wires and keep evaluation out of statements."""

import copy
import json
import os
import shutil
import sys
import tempfile
import unittest
from types import SimpleNamespace
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import correctness_gnark as C, correctness_build as B, package as P, spec_problem as S, check as Core
from zk_registry import lean_runner as L
import test_zk_registry_spec_problem as SpecToy


def unit_fixture():
    """A small synthetic canonical-input modular reduction interface."""
    return dict(id='CG01', reference_operation='reduce', model_module='ZkDet.Toy.Model',
                n_wires=3, native_prime='101', inputs=[dict(wire=1, name='secret In0_Limbs_0')],
                input_groups=[dict(name='In0', wires=[1], bits=4, modulus='7')], scalar_or_bit_inputs=[],
                emulated_outputs=[dict(wires=[2], bits=4, modulus='7')], native_outputs=[],
                field_parameters=dict(modulus='7', bits_per_limb=4, nb_limbs=1),
                output_decoding='mod-foreign-q', spec_width=4, spec_constant=12, output_limb_bounds=True)


def package_fixture():
    """A schema-only C fixture; live original-model and checker controls are separate."""
    unit = unit_fixture()
    digest = 'ab' * 32
    unit.update(registry_package_id='fixture', source={}, gnark_version='fixture', compiler={},
                original_metadata_sha256=digest, wrapper_sha256=digest, harness_sha256=digest,
                model_file='ZkDet/Toy/Model.lean', model_sha256=digest, original_r1cs_sha256=digest,
                n_constraints=2, hints={}, hint_semantics='free/untrusted original prover wires', model_assumptions=[],
                gnark_commitment_model={}, instantiation={}, standard_function='Field.Reduce', parameter_field='toy',
                reference_cost=dict(nonlinear=1, linear=1, total=2))
    accepted = dict(file='Candidate.lean', module='Candidate', sha256=digest, tested_parameters=[],
                    gate_evidence={key: dict(status='PASS', file=key + '.json', sha256=digest) for key in ('D', 'M', 'C')})
    request = SpecToy.request()
    request['files'] = [dict(path='Candidate.lean', sha256=digest, role='specification')]
    request['files'] += [dict(path=key + '.json', sha256=digest, role='gate') for key in ('D', 'M', 'C')]
    spec = S.record(request, accepted)
    modules = C.render_modules(unit)
    files = [dict(path=unit['model_file'], role='import', sha256=digest),
             dict(path='Candidate.lean', role='import', sha256=digest)]
    for path in ('Bindings.lean', 'Statements.lean'):
        files.append(dict(path=path, role='import', sha256=__import__('hashlib').sha256(modules[path].encode()).hexdigest()))
    files.append(dict(path='ProofStatement.lean', role='statement',
                      sha256=__import__('hashlib').sha256(C.theorem_template(unit, 'soundness').encode()).hexdigest()))
    files += [dict(path=path, role='evidence', sha256=digest)
              for path in ('evidence/nonvacuity.json', 'evidence/real.txt', 'evidence/battery.json', 'spec/problem.json')]
    nv = dict(status='NONVACUOUS', file='evidence/nonvacuity.json', sha256=digest,
              witness_file='evidence/real.txt', witness_sha256=digest)
    battery = dict(file='evidence/battery.json', sha256=digest, forms=C.BATTERY_FORMS,
                   attempts=[dict(index=i, completed=True, closed=False) for i in range(1, 8)], checker_verdict=None)
    env = dict(lean='v4.33.1', mathlib='ab' * 20, lake_manifest_sha256=digest, packages={})
    return C.make_record(unit, spec, 'soundness', files, digest, nv, battery, env, '2026-10-10T00:00:00Z')


class ContractTests(unittest.TestCase):
    def test_direct_strict_families_reuse_only_approved_canonical_domain(self):
        """Strict variants keep unchanged bodies and original widths, not a new spec or raw-output claim."""
        self.assertEqual(C.OPERATIONS.get('Field.ReduceStrict'), 'reduce')
        self.assertEqual(C.OPERATIONS.get('Field.ToBitsCanonical'), 'to_bits')
        self.assertNotIn('Field.Sqrt', C.OPERATIONS)
        self.assertNotIn('Field.ModMulCanonical', C.OPERATIONS)

    def test_direct_mul_family_reuse_does_not_add_projection_aliases(self):
        """Only the checked two-operand residue interface extends the original method mapping."""
        self.assertEqual(C.OPERATIONS['Field.Mul'], 'mul_mod')
        for name in ('Field.Neg', 'Field.AssertIsEqual', 'Field.Inverse', 'Curve.Select'):
            self.assertNotIn(name, C.OPERATIONS)

    def test_fact_recomputes_compile_and_kernel_acceptance(self):
        """A rehashed PASS label cannot hide failed compilation, sorry axioms or replay errors."""
        digest = 'ab' * 32
        checker = dict(reference_type_sha256=digest)
        report = dict(verdict='PASS', invalid=[], fail=[], error=[], compile=dict(
            imports=[dict(path='Model.lean', rc=0, errors=0)], compile_rc=0, compile_timeout=False,
            n_errors=0, errors=[], sorry_warnings=0, printed_axioms=[],
            post=dict(rc=0, timeout=False, replay='ok', target=dict(kind='theorem'),
                      type_sha256=digest, axioms=[], consts=[])))
        self.assertEqual(P._correctness_closure_errors(report, checker), [])
        for section, key, value in [('root', 'invalid', ['kernel rejected']),
                                    ('compile', 'compile_rc', 1), ('compile', 'printed_axioms', ['sorryAx']),
                                    ('post', 'replay', 'error'), ('post', 'type_sha256', 'cd' * 32),
                                    ('post', 'target', dict(kind='definition')), ('post', 'axioms', ['sorryAx']),
                                    ('post', 'timeout', True), ('post', 'consts', [None])]:
            changed = copy.deepcopy(report)
            target = changed if section == 'root' else changed['compile']
            if section == 'post':
                target = target['post']
            target[key] = value
            self.assertTrue(P._correctness_closure_errors(changed, checker), (section, key))
        for malformed in (None, [], dict(verdict='PASS', invalid=[], fail=[], error=[], compile=None)):
            self.assertTrue(P._correctness_closure_errors(malformed, checker))

    def test_unclassified_process_exits_are_not_a_completed_floor(self):
        """Signal-like exits, missing diagnostics and source errors must never qualify OPEN."""
        for rc, errors in [(137, []), (1, []), (1, [dict(severity='error', data='unknown identifier bogus')])]:
            result = SimpleNamespace(rc=rc, timeout=False, memkill=False, killed_by='')
            self.assertFalse(B.battery_completed(result, errors))
        result = SimpleNamespace(rc=1, timeout=False, memkill=False, killed_by='')
        self.assertTrue(B.battery_completed(result, [dict(severity='error', data='omega could not prove the goal:')]))

    def test_invalid_or_incomplete_closure_is_not_fact(self):
        """A literal PASS label cannot authorize an unexecuted or non-frozen form."""
        for attempt in (dict(index=999, closed=True, completed=False),
                        dict(index=1, closed=True, completed=False), None):
            self.assertEqual(C.classify('NONVACUOUS', [attempt], 'PASS'), 'EXCLUDED')

    def test_malformed_nested_counts_and_attempts_are_rejected(self):
        """Malformed package input returns validation errors rather than escaping exceptions."""
        for key in ('counts', 'attempts'):
            problem = package_fixture()
            if key == 'counts':
                problem['binding']['reference_cost']['nonlinear'] = 'not-a-count'
            else:
                problem['battery']['attempts'] = [None]
            self.assertTrue(P.validate_problem(problem))

    def test_schema_dispatch_and_binding_tampering(self):
        """C has its own schema and pins model, spec, module closure, resource limits and type."""
        problem = package_fixture()
        self.assertEqual(P.validate_problem(problem), [])
        self.assertTrue(P.has_statement(problem))
        for key in ('model_sha256', 'original_r1cs_sha256', 'native_prime'):
            altered = copy.deepcopy(problem)
            altered['binding'][key] = 'cd' * 32
            self.assertTrue(P.validate_problem(altered))
        altered = copy.deepcopy(problem)
        altered['checker']['allowed_axioms'].append('sorryAx')
        self.assertTrue(P.validate_problem(altered))
        altered = copy.deepcopy(problem)
        altered['files'][2]['path'] = 'Evaluation.lean'
        self.assertTrue(P.validate_problem(altered))

    def test_production_text_core_rejects_sorry_and_changed_statement(self):
        """The new path preserves production core text and forbidden-construct checks."""
        statement = C.theorem_template(unit_fixture(), 'soundness')
        invalid, helpers, body = Core.text_check(statement, statement, 'solution')
        self.assertFalse(invalid)
        self.assertTrue(Core.forbidden_scan([('helpers', helpers), ('proof', body)]))
        changed = statement.replace('CG01.Soundness', 'True')
        self.assertTrue(Core.text_check(statement, changed, 'solution')[0])

    def test_statement_is_propositions_only_and_evaluation_is_separate(self):
        """Neither the contract nor its pure bindings imports the witness evaluator."""
        modules = C.render_modules(unit_fixture())
        statements = modules['Statements.lean']
        self.assertIn('def Soundness : Prop :=', statements)
        self.assertIn('def Completeness : Prop :=', statements)
        self.assertNotIn('IO', statements)
        self.assertNotIn('instance', statements)
        self.assertNotIn('Evaluation', statements)
        self.assertNotIn('Evaluation', modules['Bindings.lean'])
        self.assertIn('def main', modules['Evaluation.lean'])
        self.assertIn('OutputLimbsBounded w', statements)

    def test_reduce_bounds_are_conclusions_not_premises(self):
        """Reduce's approved range conditions must not weaken the universal premise."""
        statements = C.render_modules(unit_fixture())['Statements.lean']
        self.assertNotIn('OutputLimbsBounded w →', statements)
        self.assertIn('outputValues w = specResult (dataOf w) ∧ OutputLimbsBounded w', statements)
        self.assertIn('outputValues w = specResult data ∧ OutputLimbsBounded w', statements)

    def test_joint_premises_require_one_row_not_disjoint_rows(self):
        """No fabricated non-vacuity from constraints and domains holding on different rows."""
        first = dict(valid_length=True, Constraints=True, Assumptions=True, CanonicalInputs=False,
                     SpecDomain=True, AcceptedData=True, BindInputs=True)
        second = dict(first, Constraints=False, CanonicalInputs=True)
        self.assertEqual(C.nonvacuity_status([first, second], 'soundness', completed=True), 'VACUOUS')
        good = dict(first, CanonicalInputs=True)
        self.assertEqual(C.nonvacuity_status([good], 'soundness', completed=True), 'NONVACUOUS')
        self.assertEqual(C.nonvacuity_status([good], 'completeness', completed=True), 'NONVACUOUS')
        self.assertEqual(C.nonvacuity_status([good], 'soundness', completed=False), 'UNMEASURED')
        good['Constraints'] = 1
        self.assertEqual(C.nonvacuity_status([good], 'soundness', completed=True), 'VACUOUS')

    def test_output_match_is_never_a_soundness_premise(self):
        """An output diagnostic cannot launder a potentially false universal contract."""
        row = dict(valid_length=True, Constraints=True, Assumptions=True, CanonicalInputs=True,
                   SpecDomain=True, output_matches_diagnostic_not_premise=False)
        self.assertEqual(C.nonvacuity_status([row], 'soundness', completed=True), 'NONVACUOUS')

    def test_battery_fact_requires_accepted_closure_and_all_forms_for_open(self):
        """A timeout or unvalidated tactic is not an OPEN or FACT qualification."""
        clean = [dict(index=i, completed=True, closed=False) for i in range(1, 8)]
        self.assertEqual(C.classify('NONVACUOUS', clean, checker_verdict=None), 'OPEN')
        clean[0]['closed'] = True
        self.assertEqual(C.classify('NONVACUOUS', clean, checker_verdict='PASS'), 'FACT')
        self.assertEqual(C.classify('NONVACUOUS', clean, checker_verdict='FAIL'), 'EXCLUDED')
        self.assertEqual(C.classify('VACUOUS', clean, checker_verdict='PASS'), 'VACUOUS')
        clean[0]['closed'] = False
        clean[-1]['completed'] = False
        self.assertEqual(C.classify('NONVACUOUS', clean, checker_verdict=None), 'EXCLUDED')

    def test_all_input_wires_must_be_bound_exactly_once(self):
        """The generated completeness interface cannot omit or duplicate original inputs."""
        unit = unit_fixture()
        C.render_modules(unit)
        for change in ('omit', 'duplicate', 'escape'):
            bad = copy.deepcopy(unit)
            if change == 'omit':
                bad['input_groups'] = []
            elif change == 'duplicate':
                bad['scalar_or_bit_inputs'] = [1]
            else:
                bad['input_groups'][0]['wires'] = [3]
            with self.assertRaises(ValueError):
                C.render_modules(bad)


@unittest.skipUnless(os.environ.get('ZK_CORRECTNESS_GNARK_LIVE_FIXTURES'), 'known M fixtures and pinned Lean env required')
class KnownMProofTests(unittest.TestCase):
    def test_known_proofs_and_rejected_text_controls(self):
        """Check existing pilot proofs only; no proof or specification is authored by this regression."""
        fixture = json.loads(Path(os.environ['ZK_CORRECTNESS_GNARK_LIVE_FIXTURES']).read_text())
        env = L.load_env(fixture['lean_env'])
        for item in fixture['cases']:
            with tempfile.TemporaryDirectory(dir=fixture['work_root']) as root:
                source = Path(item['proof'])
                original = source.read_bytes()
                accepted = C.check_solution(item['package'], str(source), env, str(Path(root, 'accepted')))
                self.assertEqual(accepted['verdict'], 'PASS', accepted)
                self.assertEqual(source.read_bytes(), original)
                reports = {'existing-proof': accepted}
                statement = Path(item['package'], 'ProofStatement.lean').read_text()
                sorry = Path(root, 'Sorry.lean')
                sorry.write_text(statement)
                rejected = C.check_solution(item['package'], str(sorry), env, str(Path(root, 'sorry')))
                self.assertEqual(rejected['verdict'], 'INVALID', rejected)
                reports['sorry'] = rejected
                changed = Path(root, 'Changed.lean')
                goal = json.loads(Path(item['package'], 'problem.json').read_text())['binding']['id'] + '.Soundness'
                changed.write_text(statement.replace(goal, 'True'))
                rejected = C.check_solution(item['package'], str(changed), env, str(Path(root, 'changed')))
                self.assertEqual(rejected['verdict'], 'INVALID', rejected)
                reports['changed-statement'] = rejected
                if fixture.get('report_root'):
                    destination = Path(fixture['report_root'], source.parent.name + '-checker.json')
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    with destination.open('x') as stream:
                        json.dump(dict(package=item['package'], proof=item['proof'],
                                       original_proof_sha256=P.sha256_file(str(source)),
                                       known_M_fixture=True, new_proof_attempts=0, reports=reports), stream, indent=2)
                    shutil.copytree(root, Path(fixture['report_root'], source.parent.name + '-artifacts'))


if __name__ == '__main__':
    unittest.main()
