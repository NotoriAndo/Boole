"""M3 real-release contracts, using synthetic inventories only."""
import copy
import contextlib
import io
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
    value = dict(version=1, adapter='boole-m3-real/v1', source_sha256='1'*64,
        kernel_set_digest='2'*64, policy=dict(version=2, rules_sha256=E.RULES_SHA256),
        caps_sha256='3'*64, fuel_authority_sha256='4'*64,
        acceptance_environment_sha256=C.sha((Path(A.L.__file__).parent/'data/acceptance_environment_v2.json').read_bytes()),
        acceptance_tools_sha256=C.sha((Path(A.L.__file__).parent/'data/acceptance_tools_v2.json').read_bytes()),
        artifact_table_sha256='5'*64,
        grammar=1, fuel=dict(lean_weight=1, nanoda_weight=1, cap_charge='bounded-debit/v1'),
        profiles={name: dict(assurance=assurance, tools={key: '6'*64 for key in tools})
                  for name, assurance in [('Darwin-arm64', 'development'), ('Linux-aarch64', 'production')]})
    value['kernel_set_digest'] = C.kernel_set_identity(value)
    return value


OBSERVATION_CASES = [
    ('PASS', None, 'accept', ''),
    ('INVALID', 'kernel-rejection', 'kernel_reject', ''),
    ('INVALID', 'kernel-disagreement', 'kernel_disagreement', ''),
    ('INVALID', 'resource-cap', 'resource_cap', ''),
    ('INVALID', 'tool-fault:replay', 'kernel_panic_cause_missing', ''),
    ('INVALID', 'disallowed-axiom', 'trusted_disagreement', 'malformed-record'),
    ('INVALID', 'comparator-statement-mismatch', 'trusted_disagreement', 'malformed-record'),
    ('INVALID', 'certified-base-mismatch', 'trusted_disagreement', 'malformed-record'),
    ('INVALID', 'unknown', 'malformed_result', ''),
    ('ERROR', 'library-outside-certified-base', 'library_terminal', ''),
    ('ERROR', 'resource-unavailable', 'transient', ''),
    ('ERROR', 'capture-failed', 'transient', ''),
    ('ERROR', 'output-cap', 'output_terminal', ''),
    ('ERROR', 'replay-unavailable', 'input_failure', ''),
    ('ERROR', 'silent-exit', 'input_failure', 'malformed-record'),
    ('ERROR', 'sandbox-unavailable', 'node_fault', ''),
    ('ERROR', 'unknown', 'node_fault', ''),
    ('unknown', None, 'malformed_result', ''),
]


class Observations(unittest.TestCase):
    def test_observed_reason_table(self):
        for verdict, reason, row, detail in OBSERVATION_CASES:
            with self.subTest(verdict=verdict, reason=reason):
                observed = A.observation(dict(verdict=verdict, reason=reason))
                self.assertEqual((observed.row, observed.detail), (row, detail))

    def test_generic_nanoda_panic_is_never_a_rejection(self):
        observed = A.observation(dict(verdict='INVALID', reason='tool-fault:replay', rc=4))
        for phase in ('first', 'retry', 'replay'):
            result = M.classify(observed, descriptor(), phase)
            self.assertEqual((result['row'], result['outcome'], result['failure_class']),
                             ('kernel_panic_cause_missing', 'DEFER', 'D-node'))


class Descriptor(unittest.TestCase):
    def test_full_environment_and_tools_files_are_runtime_pins(self):
        from types import SimpleNamespace
        value=descriptor();data=Path(A.L.__file__).parent/'data'
        value['caps_sha256']=A.D.digest(data/'testnet_caps_v2.json')
        value['fuel_authority_sha256']=A.D.digest(data/'fuel_cap_authority_v1.json')
        env=A.L.production_descriptor()['artifact_profiles']['Darwin-arm64']
        with patch.object(A.artifact_pins,'snapshot',return_value=dict(env,olean_roots=env['olean_roots'][:1])), \
             patch.object(A,'tool_pins',return_value=value['profiles']['Darwin-arm64']['tools']), \
             patch.object(A,'source_release',return_value=value['source_sha256']):
            A.verify_tools(value,SimpleNamespace(kernel_prefix='synthetic'),'synthetic')
            for key in ('acceptance_environment_sha256','acceptance_tools_sha256'):
                bad=copy.deepcopy(value);bad[key]='a'*64;bad['kernel_set_digest']=C.kernel_set_identity(bad)
                with self.subTest(key=key),self.assertRaisesRegex(F.Refusal,'node_fault'):
                    A.verify_tools(bad,SimpleNamespace(kernel_prefix='synthetic'),'synthetic')
    def test_profiles_and_kernel_set_are_closed_release_inputs(self):
        good=descriptor()
        bad=copy.deepcopy(good);bad['profiles']['Linux-x86_64']=copy.deepcopy(bad['profiles']['Linux-aarch64'])
        assurance=copy.deepcopy(good);assurance['profiles']['Linux-aarch64']['assurance']='development'
        missing=copy.deepcopy(good);del missing['profiles']['Linux-aarch64']
        digest=copy.deepcopy(good);digest['kernel_set_digest']='9'*64
        for value in (bad,assurance,missing):
            value['kernel_set_digest']=C.kernel_set_identity(value)
        for value in (bad,assurance,missing,digest):
            with self.subTest(profiles=list(value['profiles'])),self.assertRaises(ValueError):
                C.decode(C.canonical(value))

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
    def test_own_allowed_axiom_name_without_inventory_payload_is_rejected(self):
        table=self.inventory.data();del table['base'][F.name_key((('str','propext'),))]
        raw=C.canonical(table);inventory=F.Inventory(raw,C.sha(raw))
        with self.assertRaisesRegex(F.Refusal,'axiom_rejected'):
            F.partition(F.read_export(self.path),inventory,dict(self.pkg,mode='full'))
    def test_natural_literal_limit_is_independent_of_interpreter_int_limits(self):
        from types import SimpleNamespace
        real_int = int; converted = []
        def limited_int(value):
            converted.append(len(value))
            if len(value) > 640:
                raise ValueError('simulated Python integer digit limit')
            return real_int(value)
        export = SimpleNamespace(exprs={99: ('natVal', '1'+'0'*4999)})
        with patch.object(F, 'int', limited_int, create=True):
            F.own_input_caps(export, [99])
        self.assertTrue(all(n <= 640 for n in converted))
        export.exprs[99] = ('natVal', '1'+'0'*9865)
        with patch.object(F, 'int', side_effect=AssertionError('conversion before length guard'), create=True):
            with self.assertRaisesRegex(F.Refusal, 'structural_cap'):
                F.own_input_caps(export, [99])

    def test_selector_preserves_numeric_components_without_string_aliases(self):
        self.assertEqual(F.selector((('str', 'N'), ('num', 1), ('str', 'x'))), 'N.1.x')
        for name in ((('str', 'N'), ('str', '1')), (('str', 'N.1'),), (('str', 'N'), ('str', 'λ'))):
            with self.subTest(name=name), self.assertRaisesRegex(F.Refusal, 'name_grammar'):
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

    def test_json_integer_admission_is_interpreter_independent(self):
        with self.path.open('ab') as stream:
            stream.write(b'{"ie":'+b'1'*1000+b',"bvar":0}\n')
        with self.assertRaisesRegex(F.Refusal,'front_invalid'):
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


class ReservedNames(unittest.TestCase):
    setUp = Front.setUp
    write = Front.write

    def add_definition(self, parts, *, authenticated=False):
        parent = 0
        for index, part in enumerate(parts, 6):
            self.rows.append({'in': index, **({'num': dict(pre=parent, i=part)} if type(part) is int else
                                             {'str': dict(pre=parent, str=part)})})
            parent = index
        self.rows.append({'def': dict(name=parent, levelParams=[], type=0, value=1,
                                     hints='opaque', safety='safe', all=[parent])})
        self.write(self.rows)
        if authenticated:
            export = F.read_export(self.path); table = self.inventory.data()
            name = tuple(('num' if type(part) is int else 'str', part) for part in parts)
            table['base'][F.name_key(name)] = F.payload(export, name)
            table['base'] = dict(sorted(table['base'].items()))
            raw = C.canonical(table); self.inventory = F.Inventory(raw, C.sha(raw))
            self.pkg['inventory_sha256'] = self.inventory.sha256
        return parent

    def rejected(self, expected):
        with self.assertRaises(F.Refusal) as caught:
            F.partition(F.read_export(self.path), self.inventory, self.pkg)
        for phase in ('first', 'retry', 'replay'):
            result = M.classify(M.Observation(caught.exception.row), descriptor(), phase)
            self.assertEqual((result['row'], result['outcome'], result['reason']),
                             (expected, 'REJECT', 0x0201 if expected == 'reserved_name' else 0x0106))

    def test_declare_boolecs_is_reserved(self):
        self.add_definition(('BooleCS', 'X'))
        self.rejected('reserved_name')

    def test_declare_leaf_spec_is_reserved(self):
        self.add_definition(('Leaf', 'spec'))
        self.rejected('reserved_name')

    def test_declare_boole_r12_is_reserved(self):
        self.add_definition(('Boole', 'R12', 'x'))
        self.rejected('reserved_name')

    def test_declare_boole_leaf_is_reserved(self):
        self.add_definition(('Boole', 'Leaf', 'foo'))
        self.rejected('reserved_name')

    def test_private_declaration_cannot_hide_boolecs(self):
        self.add_definition(('_private', 'M', 0, 'BooleCS', 'X'))
        self.rejected('reserved_name')

    def test_private_reference_cannot_hide_admitted_boole_r(self):
        index = self.add_definition(('_private', 'M', 0, 'Boole', 'R12', 'x'), authenticated=True)
        self.rows.append({'ie': 99, 'const': dict(name=index, us=[])})
        next(row['thm'] for row in self.rows if 'thm' in row)['value'] = 99
        self.write(self.rows); self.rejected('reserved_name')

    def test_reserved_redeclaration_precedes_cached_payload_mismatch(self):
        for parts in (('BooleCS', 'Con'), ('Boole', 'Leaf', 'spec')):
            with self.subTest(parts=parts):
                self.setUp()
                index = self.add_definition(parts, authenticated=True)
                next(row['def'] for row in self.rows if 'def' in row and row['def']['name']==index)['value'] = 2
                self.write(self.rows); self.rejected('reserved_name')

    def test_reserved_reference_precedes_an_unrelated_base_mismatch(self):
        index=self.add_definition(('Boole','R12','x'),authenticated=True)
        self.rows.append({'ie':99,'const':dict(name=index,us=[])})
        next(row['thm'] for row in self.rows if 'thm' in row)['value']=99
        next(row['axiom'] for row in self.rows if 'axiom' in row)['type']=1
        self.write(self.rows);self.rejected('reserved_name')

    def test_boole_ratchet_namespace_is_not_an_admission(self):
        self.add_definition(('Boole', 'Ratchet', 'helper'))
        own = F.partition(F.read_export(self.path), self.inventory, self.pkg).own
        self.assertIn((('str', 'Boole'), ('str', 'Ratchet'), ('str', 'helper')), own)

    def test_reference_admitted_boole_r_is_reserved(self):
        index = self.add_definition(('Boole', 'R1', 'x'), authenticated=True)
        self.rows.append({'ie': 99, 'const': dict(name=index, us=[])})
        next(row['thm'] for row in self.rows if 'thm' in row)['value'] = 99
        self.write(self.rows); self.rejected('reserved_name')

    def test_unauthenticated_boolecs_reference_is_unknown_constant(self):
        self.rows += [{'in': 6, 'str': dict(pre=0, str='BooleCS')},
                      {'in': 7, 'str': dict(pre=6, str='Y')},
                      {'ie': 99, 'const': dict(name=7, us=[])}]
        next(row['thm'] for row in self.rows if 'thm' in row)['value'] = 99
        self.write(self.rows); self.rejected('unknown_constant')

    def test_unknown_constant_on_a_trusted_input_stays_node_fault(self):
        self.rows += [{'in': 6, 'str': dict(pre=0, str='BooleCS')},
                      {'in': 7, 'str': dict(pre=6, str='Y')},
                      {'ie': 99, 'const': dict(name=7, us=[])}]
        self.write(self.rows)
        with self.assertRaisesRegex(F.Refusal, 'node_fault'):
            F.read_export(self.path, 'reference')
        for role in ('reference', 'gate-generated'):
            for phase in ('first', 'retry', 'replay'):
                mapped = M.classify(M.Observation('unknown_constant', role=role), descriptor(), phase)
                self.assertEqual((mapped['outcome'], mapped['failure_class']), ('DEFER', 'D-node'))

    def test_authenticated_leaf_reference_is_allowed(self):
        index = self.add_definition(('Leaf', 'spec'), authenticated=True)
        self.rows.append({'ie': 99, 'const': dict(name=index, us=[])})
        next(row['thm'] for row in self.rows if 'thm' in row)['value'] = 99
        self.write(self.rows)
        self.assertEqual(F.partition(F.read_export(self.path), self.inventory, self.pkg).own,
                         ((('str', 'goal'),),))

    def test_authenticated_boolecs_con_and_constructor_references_are_allowed(self):
        self.add_definition(('BooleCS', 'Con'), authenticated=True)
        # Keep distinct intern ids for the constructor's child name.
        self.rows += [{'in': 9, 'str': dict(pre=7, str='mk')},
                      {'def': dict(name=9, levelParams=[], type=0, value=1,
                                   hints='opaque', safety='safe', all=[9])},
                      {'ie': 99, 'const': dict(name=7, us=[])},
                      {'ie': 100, 'const': dict(name=9, us=[])},
                      {'ie': 101, 'app': dict(fn=99, arg=100)}]
        next(row['thm'] for row in self.rows if 'thm' in row)['value'] = 101
        self.write(self.rows); export = F.read_export(self.path); table = self.inventory.data()
        name = (('str', 'BooleCS'), ('str', 'Con'), ('str', 'mk'))
        table['base'][F.name_key(name)] = F.payload(export, name)
        table['base'] = dict(sorted(table['base'].items())); raw = C.canonical(table)
        inventory = F.Inventory(raw, C.sha(raw))
        self.assertEqual(F.partition(export, inventory, self.pkg).own, ((('str', 'goal'),),))


class Adapter(Front):
    def test_generic_panic_keeps_its_defer_cause_without_counters(self):
        result,_,context=self.invoke('panic-without-counters',dict(verdict='INVALID',reason='tool-fault:replay',rc=4))
        self.assertEqual((result['row'],result['outcome'],result['failure_class']),
                         ('kernel_panic_cause_missing','DEFER','D-node'))
        A.decode_reply(C.canonical(result),self.desc,context)
    def test_public_counter_mode_refuses_candidate_selection(self):
        with patch.object(A.D,'compare_exports') as inner:
            for selected in (None, [], ['goal']):
                with self.subTest(selected=selected),self.assertRaises(ValueError):
                    A.compare_exports(public_counter=True,candidate_declarations=selected)
            inner.assert_not_called()
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

    def invoke(self, label, native=None, bundle=None, phase='first', package_bytes=None, authority_error=None):
        context = dict(self.context)
        raw = self.bundle if bundle is None else bundle
        context['export_digest'] = C.sha(raw)
        encoded = C.canonical(self.pkg) if package_bytes is None else package_bytes
        context['package_sha256'] = C.sha(encoded)
        with patch.object(A, 'verify_tools'), patch.object(A.FC, 'load_authority', return_value=self.authority, side_effect=authority_error), \
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

    def test_submission_name_grammar_cannot_become_a_node_defer(self):
        original = copy.deepcopy(self.rows)
        for index, spelling in enumerate(('λ', '', 'goal.with.dot')):
            with self.subTest(spelling=spelling):
                self.rows = copy.deepcopy(original)
                self.rows += [{'in':6,'str':dict(pre=0,str=spelling)},
                              {'def':dict(name=6,levelParams=[],type=0,value=1,hints='opaque',safety='safe',all=[6])}]
                self.write(self.rows)
                self.pkg['reference_sha256'] = C.sha(self.path.read_bytes())
                result, compare, _ = self.invoke('name-grammar-'+str(index), bundle=F.pack(self.path.read_bytes()))
                self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0103))
                compare.assert_not_called()

    def test_pre_kernel_resource_and_output_caps_are_terminal_rejections(self):
        for index, (verdict, reason, expected) in enumerate((('INVALID','resource-cap',0x0108), ('ERROR','output-cap',0x0109))):
            native = dict(verdict=verdict, reason=reason, rc=2, comparator_status=[
                dict(version=1, stage='policy', reason=reason), dict(version=1, stage='final', rc=2)])
            result, _, context = self.invoke('pre-kernel-cap-'+str(index), native)
            self.assertEqual((result['outcome'], result['reason'], result['kernels'], result['fuel']),
                             ('REJECT', expected, M.not_run(), 0))
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_output_limit_keeps_known_kernel_debits_and_never_invents_them(self):
        native=dict(self.native,verdict='ERROR',reason='output-cap')
        result,_,context=self.invoke('output-with-counters',native)
        self.assertEqual((result['outcome'],result['reason'],result['fuel']),('REJECT',0x0109,5))
        self.assertTrue(all(v['state']=='executed' for v in result['kernels'].values()))
        A.decode_reply(C.canonical(result),self.desc,context)
        native=dict(verdict='ERROR',reason='output-cap',rc=2,comparator_status=[])
        result,_,_=self.invoke('output-without-records',native)
        self.assertEqual((result['outcome'],result['fuel']),('DEFER',None))

    def test_cli_oversized_bundle_is_a_typed_structural_rejection(self):
        from zk_registry.m3_gate import __main__ as main
        directory = Path(self.tmp.name)/'cli'; directory.mkdir()
        for name, raw in [('descriptor',C.encode(self.desc)),('package',C.canonical(self.pkg)),
                          ('inventory',self.inventory.raw),('context',C.canonical(self.context))]:
            (directory/name).write_bytes(raw)
        bundle = directory/'bundle'
        with bundle.open('wb') as stream:
            stream.truncate(F.RAW_MAX+1)
        argv = ['gate_m3','--gate-digest',C.identity(self.desc)]
        for name in ('descriptor','package','inventory','context','bundle'):
            argv += ['--'+name,str(directory/name)]
        argv += ['--reference',str(self.path),'--tools-root','synthetic','--counter-driver','synthetic',
                 '--work',str(directory/'work')]
        stdout=io.StringIO()
        with patch.object(sys,'argv',argv), patch.object(A.D.Tools,'load',return_value=object()), \
             patch.object(A,'verify_tools'), contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
            code=main.main()
        self.assertEqual(code,0)
        value=C.loads(stdout.getvalue()[len(A.MARKER):])
        self.assertEqual((value['outcome'],value['reason']),('REJECT',0x0108))

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

    def test_trusted_comparator_disagreements_are_class_i_after_front_validation(self):
        for index, reason in enumerate(('comparator-statement-mismatch', 'certified-base-mismatch', 'disallowed-axiom')):
            for phase in ('first', 'retry', 'replay'):
                with self.subTest(reason=reason, phase=phase):
                    native = dict(verdict='INVALID', reason=reason, rc=2, comparator_status=[
                        dict(version=1, stage='policy', reason=reason), dict(version=1, stage='final', rc=2)])
                    result, compare, context = self.invoke('disagreement-'+str(index)+'-'+phase, native, phase=phase)
                    compare.assert_called_once()
                    self.assertEqual((result['outcome'], result['failure_class'], result['row']),
                                     ('DEFER', 'I', 'trusted_disagreement'))
                    A.decode_reply(C.canonical(result), self.desc, context)

    def test_authenticated_malformed_package_uses_void_only_fingerprint(self):
        result, compare, context = self.invoke('bad-package', package_bytes=b'{}')
        self.assertEqual((result['outcome'], result['reason']), ('VOID', 0x0801))
        self.assertEqual(result['statement_id'], M.void_statement(context)); compare.assert_not_called()
        A.decode_reply(C.canonical(result), self.desc, context)

    def test_release_inventory_or_environment_mismatch_is_node_defer(self):
        for index, key in enumerate(('inventory_sha256', 'env_id')):
            for phase in ('first', 'retry', 'replay'):
                with self.subTest(key=key, phase=phase):
                    pkg = dict(self.pkg, **{key: 'a'*64})
                    result, compare, context = self.invoke('release-'+str(index)+'-'+phase,
                        package_bytes=C.canonical(pkg), phase=phase)
                    self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
                    compare.assert_not_called(); A.decode_reply(C.canonical(result), self.desc, context)

    def test_local_authority_load_failure_is_not_a_published_cap_void(self):
        for phase in ('first', 'retry', 'replay'):
            result, compare, context = self.invoke('authority-'+phase, phase=phase,
                authority_error=A.FC.FuelCapError('fuel-authority-unavailable'))
            self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
            compare.assert_not_called(); A.decode_reply(C.canonical(result), self.desc, context)
        self.pkg['fuel_cap']['sha256'] = 'a'*64
        result, compare, context = self.invoke('invalid-published-cap')
        self.assertEqual((result['outcome'], result['reason']), ('VOID', 0x0803))
        compare.assert_not_called(); A.decode_reply(C.canonical(result), self.desc, context)

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

    def test_reply_cannot_use_an_unknown_row_with_malformed_result_fields(self):
        result,_,context=self.invoke('unknown-result-row',dict(verdict='INVALID',reason='unknown',rc=2))
        self.assertEqual(result['row'],'malformed_result')
        for row in ('unknown-row',[],None):
            with self.subTest(row=row),self.assertRaises(ValueError):
                A.decode_reply(C.canonical(dict(result,row=row)),self.desc,context)

    def test_unrepresentable_target_rejects_without_a_node_defer(self):
        with self.assertRaisesRegex(F.Refusal, 'name_grammar'):
            F.fqn([['str', 'goal.with.dot']])
        self.rows[4]['str']['str'] = 'goal.with.dot'
        self.write(self.rows)
        export = F.read_export(self.path)
        self.pkg.update(target=[['str', 'goal.with.dot']], reference_sha256=export.raw_sha256)
        result, compare, _ = self.invoke('target-representation', bundle=F.pack(self.path.read_bytes()))
        self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0103))
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
    ('name_grammar', 'REJECT', 0x0103, 'apply', ''),
    ('unknown_constant', 'REJECT', 0x0106, 'apply', ''),
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
    ('trusted_disagreement', 'DEFER', 0, 'retry', 'I'),
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
