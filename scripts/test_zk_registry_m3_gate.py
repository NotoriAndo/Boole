"""M3 real-release contracts, using synthetic inventories only."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import error_policy as E
from zk_registry.m3_gate import codec as C
from zk_registry.m3_gate import front as F
from zk_registry.m3_gate import mapping as M
from zk_registry.m3_gate import adapter as A
from test_statement_identity import fixture


def descriptor():
    tools = ('comparator', 'nanoda', 'lean_replay', 'counter_replay', 'exporter',
             'lean_runtime', 'imports', 'sandbox', 'codec')
    return dict(version=1, adapter='boole-m3-real/v1', source_sha256='1'*64,
        kernel_set_digest='2'*64, policy=dict(version=2, rules_sha256=E.RULES_SHA256),
        caps_sha256='3'*64, fuel_authority_sha256='4'*64, artifact_table_sha256='5'*64,
        grammar=1, fuel=dict(lean_weight=1, nanoda_weight=1, cap_charge='bounded-debit/v1'),
        profiles={name: dict(assurance=assurance, tools={key: '6'*64 for key in tools})
                  for name, assurance in [('Darwin-arm64', 'development'), ('Linux-aarch64', 'production')]})


class Descriptor(unittest.TestCase):
    def test_host_selection_keeps_the_same_global_identity(self):
        value = descriptor(); raw = C.encode(value); identity = C.identity(value)
        for name in value['profiles']:
            self.assertEqual(C.for_host(raw, identity, name)['gate_rule_digest'], identity)
        for raw in (b'{"version":1,"version":1}', C.encode(value)+b'\n'):
            with self.assertRaises(ValueError):
                C.decode(raw)
    def test_noncanonical_or_unbound_descriptors_are_refused(self):
        good = descriptor()
        mutations = [dict(reversed(list(good.items()))), dict(good, unknown=1),
                     dict(good, grammar=1.0), dict(good, grammar=True),
                     dict(good, path='/synthetic'), dict(good, clock=1)]
        for value in mutations:
            with self.subTest(value=list(value)):
                with self.assertRaises(ValueError):
                    C.decode(C.canonical(value))

    def test_content_and_domain_identities_and_all_profiles_are_bound(self):
        value = descriptor(); raw = C.encode(value)
        self.assertEqual(C.decode(raw, C.identity(value)), value)
        self.assertNotEqual(C.sha(raw), C.identity(value))
        for profile in value['profiles']:
            selected = copy.deepcopy(value); selected['profiles'] = {profile: value['profiles'][profile]}
            with self.subTest(profile=profile), self.assertRaises(ValueError):
                C.decode(C.canonical(selected), C.identity(value))


class Front(unittest.TestCase):
    def test_selector_preserves_numeric_components_without_string_aliases(self):
        self.assertEqual(F.selector((('str', 'N'), ('num', 1), ('str', 'x'))), 'N.1.x')
        for name in ((('str', 'N'), ('str', '1')), (('str', 'N.1'),), (('str', 'N'), ('str', 'λ'))):
            with self.subTest(name=name), self.assertRaisesRegex(F.Refusal, 'node_fault'):
                F.selector(name)

    def test_numeric_axiom_is_a_rejection_not_a_package_void(self):
        self.rows[1] = {'in': 1, 'num': dict(pre=0, i=1)}
        self.write(self.rows); export = F.read_export(self.path)
        with self.assertRaisesRegex(F.Refusal, 'axiom_rejected'):
            A.axioms(export, ((('str', 'goal'),),))
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name)/'export.ndjson'
        self.rows = fixture()
        # Synthetic transport-only fixture; the named axiom is permitted by the
        # closed policy. No mocked native execution here is proof evidence.
        self.rows[1]['str']['str'] = 'propext'
        self.write(self.rows)
        export = F.read_export(self.path)
        base = {F.name_key(name): F.payload(export, name) for name in export.declarations if name != (('str', 'goal'),)}
        raw = C.canonical(dict(version=1, environment='7'*64, base=dict(sorted(base.items())), library={}))
        self.inventory = F.Inventory(raw, C.sha(raw))
        self.pkg = dict(version=1, mode='certified-own', problem='synthetic', env_id='7'*64,
            target=[['str', 'goal']], statement=export.identity('goal')[F.I.SUFFIX],
            reference_sha256=export.raw_sha256, inventory_sha256=self.inventory.sha256, fuel_cap=None)

    def write(self, rows):
        self.path.write_bytes(b''.join(C.canonical(row)+b'\n' for row in rows))

    def test_checked_own_set_comes_from_authenticated_inventory(self):
        checked = F.partition(F.read_export(self.path), self.inventory, self.pkg)
        self.assertEqual(checked.own, ((('str', 'goal'),),))
        bad = bytearray(self.inventory.raw); bad[-2] = ord('x')
        with self.assertRaises(F.Refusal):
            F.Inventory(bytes(bad), self.inventory.sha256)

    def test_reserved_own_name_is_rejected_before_kernel(self):
        self.rows.insert(1, {'in': 5, 'str': dict(pre=0, str='Leaf')})
        next(row for row in self.rows if row.get('in') == 4)['str'] = dict(pre=5, str='evil')
        self.write(self.rows); pkg = dict(self.pkg, target=[['str', 'Leaf'], ['str', 'evil']])
        with self.assertRaisesRegex(F.Refusal, 'reserved_name'):
            F.partition(F.read_export(self.path), self.inventory, pkg)

    def test_cached_theorem_proofs_are_part_of_payload_identity(self):
        self.rows.insert(1, {'in': 6, 'str': dict(pre=0, str='cached')})
        self.rows.append(dict(thm=dict(name=6, levelParams=[], type=3, value=1, all=[6])))
        self.write(self.rows); export = F.read_export(self.path)
        table = self.inventory.data(); table['base'][F.name_key((('str', 'cached'),))] = F.payload(export, (('str', 'cached'),))
        table['base'] = dict(sorted(table['base'].items())); raw = C.canonical(table)
        inventory = F.Inventory(raw, C.sha(raw))
        self.rows[-1]['thm']['value'] = 2; self.write(self.rows)
        with self.assertRaisesRegex(F.Refusal, 'front_invalid'):
            F.partition(F.read_export(self.path), inventory, self.pkg)

    def test_malformed_unreachable_metadata_and_intern_graphs_are_rejected(self):
        for tail in (b'{"junk":0}\n', b'{"ie":99,"bvar":1.5}\n', b'{"ie":99,"ie":100,"bvar":0}\n',
                     b'{"ie":99,"app":{"fn":99,"arg":0}}\n'):
            with self.subTest(tail=tail):
                self.write(self.rows)
                with self.path.open('ab') as stream:
                    stream.write(tail)
                with self.assertRaisesRegex(F.Refusal, 'front_invalid'):
                    F.read_export(self.path)

    def test_own_literal_limit_does_not_charge_an_unused_library_payload(self):
        self.rows.insert(1, {'in': 6, 'str': dict(pre=0, str='allowedLibrary')})
        self.rows += [{'ie': 99, 'strVal': 'x'*4097},
                      {'def': dict(name=6, levelParams=[], type=0, value=99, hints='opaque', safety='unsafe', all=[6])}]
        self.write(self.rows); export = F.read_export(self.path)
        table = self.inventory.data(); name = (('str', 'allowedLibrary'),)
        table['library'][F.name_key(name)] = F.payload(export, name)
        raw = C.canonical(table); inventory = F.Inventory(raw, C.sha(raw))
        self.assertEqual(F.partition(export, inventory, self.pkg).library, (name,))
        # The exact same literal in the own proof must hit the structural cap.
        next(row['thm'] for row in self.rows if 'thm' in row)['value'] = 99
        self.write(self.rows)
        with self.assertRaisesRegex(F.Refusal, 'structural_cap'):
            F.partition(F.read_export(self.path), inventory, self.pkg)


class Adapter(Front):
    def setUp(self):
        super().setUp()
        self.bundle = F.pack(self.path.read_bytes())
        self.desc = descriptor(); self.desc['artifact_table_sha256'] = self.inventory.sha256
        ref = dict(path='certified-own-delta/v1', problem=self.pkg['problem'], environment='7'*64,
                   statement=self.pkg['statement'], own_declarations='8'*64,
                   reference_export=self.pkg['reference_sha256'], units=dict(lean=64, nanoda=64), eligible=True)
        key = A.FC.digest(ref)
        cap = dict(version=1, reference=key, problem=self.pkg['problem'], environment='7'*64,
                   statement=self.pkg['statement'], own_declarations='8'*64, units=dict(lean=256, nanoda=256))
        self.pkg['fuel_cap'] = dict(cap=cap, sha256=A.FC.digest(cap))
        self.authority = dict(version=1, factors=dict(lean=4, nanoda=4), references={key: ref})
        self.context = dict(chain_id=720926, problem_id='1'*64, commit_hash='2'*64, solver='3'*40,
            env_id='7'*64, export_digest=C.sha(self.bundle), package_sha256=C.sha(C.canonical(self.pkg)),
            leaf_kind=1, lane=1, slot_fuel=1000)
        self.native = dict(verdict='PASS', reason=None, rc=0, comparator_status=[
            dict(version=1, stage='nanoda-fuel', kernel='nanoda', debit=3),
            dict(version=1, stage='fuel', kernel='lean', debit=2),
            dict(version=1, stage='final', rc=0)])

    def invoke(self, label, native=None, bundle=None, phase='first', package_bytes=None):
        context = dict(self.context)
        raw = self.bundle if bundle is None else bundle
        context['export_digest'] = C.sha(raw)
        encoded = C.canonical(self.pkg) if package_bytes is None else package_bytes
        context['package_sha256'] = C.sha(encoded)
        with patch.object(A, 'verify_tools'), patch.object(A.FC, 'load_authority', return_value=self.authority), \
             patch.object(A.FC, 'bind', return_value=A.FC.Budget(256, 256, '7'*64, 'synthetic', '8'*64, '9'*64)), \
             patch.object(A.D, 'compare_exports', return_value=self.native if native is None else native) as compare:
            result = A.run(raw, encoded, self.inventory.raw, context, C.encode(self.desc), C.identity(self.desc),
                self.path, object(), 'synthetic', Path(self.tmp.name)/label, phase=phase)
        return result, compare, context

    def test_entrypoint_passes_explicit_own_set_and_binds_final_reply(self):
        result, compare, context = self.invoke('valid')
        self.assertEqual((result['outcome'], result['fuel']), ('ACCEPT', 5))
        self.assertEqual(compare.call_args.kwargs['candidate_declarations'], ['goal'])
        self.assertEqual(result['kernels']['lean'], dict(state='executed', debit=2, charged=2))
        self.assertEqual(A.decode_reply(C.canonical(result), self.desc, context), result)
        for key, value in [('input_digest', 'a'*64), ('fuel', 0), ('reason', 1), ('statement_id', '0'*64)]:
            with self.subTest(field=key), self.assertRaises(ValueError):
                A.decode_reply(C.canonical(dict(result, **{key: value})), self.desc, context)

    def test_early_cap_does_not_fabricate_lean_work(self):
        native = dict(verdict='INVALID', reason='resource-cap', rc=3, comparator_status=[
            dict(version=1, stage='nanoda-fuel', kernel='nanoda', debit=999),
            dict(version=1, stage='policy', reason='resource-cap'), dict(version=1, stage='final', rc=3)])
        result, _, context = self.invoke('early', native)
        self.assertEqual((result['outcome'], result['fuel']), ('REJECT', 256))
        self.assertEqual(result['kernels']['lean'], dict(state='not-run', debit=None, charged=0))
        A.decode_reply(C.canonical(result), self.desc, context)
        for index, events in enumerate((native['comparator_status'][1:], self.native['comparator_status'][1:])):
            result, _, _ = self.invoke('missing-'+str(index), dict(native, comparator_status=events))
            self.assertEqual((result['outcome'], result['failure_class'], result['fuel']), ('DEFER', 'D-node', None))

    def test_generic_native_cap_without_exhausted_aggregate_defers(self):
        native = dict(verdict='INVALID', reason='resource-cap', rc=3, comparator_status=[
            dict(version=1, stage='nanoda-fuel', kernel='nanoda', debit=120),
            dict(version=1, stage='policy', reason='resource-cap'), dict(version=1, stage='final', rc=3)])
        result, _, context = self.invoke('ambiguous-cap', native)
        self.assertEqual((result['outcome'], result['failure_class'], result['row']),
                         ('DEFER', 'D-node', 'cap_cause_missing'))
        self.assertEqual(result['fuel'], 120)
        self.assertEqual(result['kernels']['lean']['state'], 'not-run')
        A.decode_reply(C.canonical(result), self.desc, context)

    def test_miner_parser_and_generated_decoder_failures_keep_their_roles(self):
        result, compare, _ = self.invoke('bad-miner', bundle=F.pack(b'{\n'))
        self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0106)); compare.assert_not_called()
        result, _, _ = self.invoke('bad-generated', dict(verdict='ERROR', reason='sandbox-unavailable', rc=70))
        self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))

    def test_authenticated_malformed_package_uses_void_only_fingerprint(self):
        result, compare, context = self.invoke('bad-package', package_bytes=b'{}')
        self.assertEqual((result['outcome'], result['reason']), ('VOID', 0x0801))
        self.assertEqual(result['statement_id'], M.void_statement(context)); compare.assert_not_called()
        A.decode_reply(C.canonical(result), self.desc, context)

    def test_reply_needs_actual_both_kernel_runs_for_acceptance(self):
        result, _, context = self.invoke('both-needed')
        result.update(kernels=M.not_run(), fuel=0)
        with self.assertRaises(ValueError):
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_deferred_reply_still_has_a_strict_kernel_channel(self):
        result, _, context = self.invoke('unavailable', dict(verdict='ERROR', reason='sandbox-unavailable', rc=70))
        A.decode_reply(C.canonical(result), self.desc, context)
        for changes in (dict(kernels={}), dict(kernels=M.not_run(), fuel=10),
                        dict(kernels=dict(lean=dict(state='unknown', debit=9, charged=0), nanoda=M.unknown()['nanoda'])),
                        dict(statement_id={}), dict(statement_id=M.void_statement(context)),
                        dict(counter_identity={'unknown': 1})):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                A.decode_reply(C.canonical(dict(result, **changes)), self.desc, context)

    def test_unrepresentable_valid_target_defers_without_package_void(self):
        with self.assertRaisesRegex(F.Refusal, 'node_fault'):
            F.fqn([['str', 'goal.with.dot']])
        self.rows[4]['str']['str'] = 'goal.with.dot'
        self.write(self.rows)
        export = F.read_export(self.path)
        self.pkg.update(target=[['str', 'goal.with.dot']], reference_sha256=export.raw_sha256)
        result, compare, _ = self.invoke('target-representation', bundle=F.pack(self.path.read_bytes()))
        self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
        compare.assert_not_called()

    def test_library_terminal_after_own_guards_in_all_three_phases(self):
        self.rows.insert(1, {'in': 6, 'str': dict(pre=0, str='allowedLibrary')})
        self.rows.append({'def': dict(name=6, levelParams=[], type=0, value=1, hints='opaque', safety='safe', all=[6])})
        self.write(self.rows); export = F.read_export(self.path)
        table = self.inventory.data(); name = (('str', 'allowedLibrary'),)
        table['library'][F.name_key(name)] = F.payload(export, name)
        raw = C.canonical(table); self.inventory = F.Inventory(raw, C.sha(raw))
        self.pkg.update(inventory_sha256=self.inventory.sha256, reference_sha256=export.raw_sha256)
        self.desc['artifact_table_sha256'] = self.inventory.sha256
        for phase in ('first', 'retry', 'replay'):
            result, compare, _ = self.invoke('library-'+phase, bundle=F.pack(self.path.read_bytes()), phase=phase)
            self.assertEqual((result['outcome'], result['reason'], result['failure_class']), ('REJECT', 0x010a, 'S'))
            compare.assert_not_called()
        self.rows.insert(1, {'in': 7, 'str': dict(pre=0, str='badOwn')})
        self.rows.append({'axiom': dict(name=7, levelParams=[], type=0, isUnsafe=False)})
        self.write(self.rows)
        result, compare, _ = self.invoke('own-before-library', bundle=F.pack(self.path.read_bytes()))
        self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0106)); compare.assert_not_called()


# Independent contract vectors: each observation is exercised through the public
# classifier; the expected tuple is never derived from the implementation table.
MAPPING_CASES = [
    ('accept', 'ACCEPT', 0, 'apply', ''),
    ('front_invalid', 'REJECT', 0x0106, 'apply', ''),
    ('structural_cap', 'REJECT', 0x0108, 'apply', ''),
    ('reserved_name', 'REJECT', 0x0201, 'apply', ''),
    ('statement_mismatch', 'REJECT', 0x0107, 'apply', ''),
    ('axiom_rejected', 'REJECT', 0x0305, 'apply', ''),
    ('kernel_reject', 'REJECT', 0x0301, 'apply', ''),
    ('kernel_disagreement', 'REJECT', 0x0302, 'apply', ''),
    ('resource_cap', 'REJECT', 0x0303, 'apply', ''),
    ('depth_cause_missing', 'DEFER', 0, 'repair', 'D-node'),
    ('cap_cause_missing', 'DEFER', 0, 'repair', 'D-node'),
    ('tool_fault', 'REJECT', 0x0307, 'apply', ''),
    ('kernel_panic_cause_missing', 'DEFER', 0, 'repair', 'D-node'),
    ('output_terminal', 'REJECT', 0x0109, 'terminal', 'S'),
    ('library_terminal', 'REJECT', 0x010a, 'terminal', 'S'),
    ('transient', 'DEFER', 0, 'retry', 'T'),
    ('input_failure', 'DEFER', 0, 'retry', 'D-input'),
    ('node_fault', 'DEFER', 0, 'repair', 'D-node'),
    ('input_availability', 'STALL', 0, 'wait', ''),
    ('input_integrity', 'STOP', 0, 'preserve', ''),
    ('package_invalid', 'VOID', 0x0801, 'apply', ''),
    ('fuel_binding_invalid', 'VOID', 0x0803, 'apply', ''),
    ('generator_unavailable', 'VOID', 0x0804, 'apply', ''),
    ('malformed_result', 'DEFER', 0, 'repair', 'D-node'),
    ('quarantine_void', 'VOID', 0x0802, 'fork-only', ''),
]


class Mapping(unittest.TestCase):
    def test_every_mapping_row_has_a_contract_vector(self):
        self.assertEqual(set(M.ROWS), {row[0] for row in MAPPING_CASES})

    def test_input_role_physical_precedence_and_fork_authority(self):
        for phase in ('first', 'retry', 'replay'):
            for key in ('tool_fault', 'resource_cap', 'kernel_reject', 'kernel_disagreement', 'library_terminal', 'output_terminal'):
                with self.subTest(key=key, phase=phase):
                    result = M.classify(M.Observation(key, role='reference'), descriptor(), phase)
                    self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
            result = M.classify(M.Observation('resource_cap', physical=True), descriptor(), phase)
            self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'T'))
            result = M.classify(M.Observation('quarantine_void'), descriptor(), phase)
            self.assertEqual(result['outcome'], 'DEFER')
        bad = descriptor(); bad['policy'] = dict(version=1, rules_sha256=E.LEGACY_POLICY_SHA256)
        with self.assertRaises(ValueError):
            M.classify(M.Observation('library_terminal'), bad)

    def test_void_fingerprint_cannot_claim_a_proof_or_another_void(self):
        context = dict(chain_id=720926, problem_id='7'*64, env_id='8'*64, package_sha256='9'*64)
        fingerprint = M.void_statement(context)
        M.check_statement('VOID', 0x0801, fingerprint, context)
        for outcome, reason, statement in [('VOID', 0x0801, '0'*64), ('ACCEPT', 0, fingerprint),
                ('REJECT', 0x0106, fingerprint), ('VOID', 0x0802, fingerprint)]:
            with self.subTest(outcome=outcome), self.assertRaises(ValueError):
                M.check_statement(outcome, reason, statement, context)

    def test_early_nanoda_cap_and_missing_counter_are_distinct(self):
        events = [dict(version=1, stage='nanoda-fuel', kernel='nanoda', debit=120),
                  dict(version=1, stage='policy', reason='resource-cap'),
                  dict(version=1, stage='final', rc=3)]
        states, fuel = M.kernel_states(events, 3, 'INVALID', dict(lean=100, nanoda=100),
                                      dict(lean_weight=2, nanoda_weight=3))
        self.assertEqual(states['lean'], dict(state='not-run', debit=None, charged=0))
        self.assertEqual(states['nanoda'], dict(state='executed', debit=120, charged=100))
        self.assertEqual(fuel, 300)
        for bad in [events[1:], [dict(events[0], debit=None), *events[1:]]]:
            with self.subTest(events=bad), self.assertRaises(ValueError):
                M.kernel_states(bad, 3, 'INVALID', dict(lean=100, nanoda=100),
                                dict(lean_weight=2, nanoda_weight=3))


def mapping_test(case):
    key, outcome, reason, action, group = case
    def run(self):
        for phase in ('first', 'retry', 'replay'):
            with self.subTest(row=key, phase=phase):
                result = M.classify(M.Observation(key, fork_authorized=(key == 'quarantine_void')), descriptor(), phase)
                self.assertEqual(result['row'], key)
                self.assertEqual((result['outcome'], result['reason'], result['action'], result['failure_class']),
                                 (outcome, reason, action, group), key)
    return run


for case in MAPPING_CASES:
    setattr(Mapping, 'test_row_'+case[0], mapping_test(case))


if __name__ == '__main__':
    unittest.main()
