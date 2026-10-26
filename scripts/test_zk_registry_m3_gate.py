"""M3 real-release contracts, using synthetic inventories only."""
import copy
import contextlib
import io
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
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
    value = dict(version=1, adapter='boole-m3-real/v1', reference_interpreter=dict(implementation='CPython',version='3.9.6'), source_sha256='1'*64,
        kernel_set_digest='2'*64, policy=dict(version=2, rules_sha256=E.RULES_SHA256),
        caps_sha256='3'*64, fuel_authority_sha256='4'*64,
        acceptance_environment_sha256=C.sha((Path(A.L.__file__).parent/'data/acceptance_environment_v2.json').read_bytes()),
        acceptance_tools_sha256=C.sha((Path(A.L.__file__).parent/'data/acceptance_tools_v2.json').read_bytes()),
        artifact_table_sha256='5'*64,
        grammar=1, json_nesting_limit=127, fuel=dict(lean_weight=1, nanoda_weight=1, cap_charge='bounded-debit/v2'),
        profiles={name: dict(assurance=assurance, tools={key: '6'*64 for key in tools})
                  for name, assurance in [('Darwin-arm64', 'development'), ('Linux-aarch64', 'production')]})
    value['kernel_set_digest'] = C.kernel_set_identity(value)
    return value


DEFAULT_BUNDLE = object()


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
    def test_silent_exit_classifies_as_i(self):
        observed=A.observation(dict(verdict='ERROR',reason='silent-exit',detail='silent-exit:71'))
        for phase in ('first','retry','replay'):
            self.assertEqual(M.classify(observed,descriptor(),phase)['failure_class'],'I')
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


class MacProfile(unittest.TestCase):
    """Contract fixtures describe the Mac development release on every test host."""
    def setUp(self):
        super().setUp()
        profile = patch.object(A, 'host_profile', return_value='Darwin-arm64')
        profile.start()
        self.addCleanup(profile.stop)


class HostProfiles(unittest.TestCase):
    def assert_linux_refused(self, machine):
        host = SimpleNamespace(system=lambda: 'Linux', machine=lambda: machine)
        with patch.object(A, 'platform', host), \
             patch.object(A.artifact_pins, 'snapshot', side_effect=AssertionError('unexpected SDK access')):
            self.assertEqual(A.host_profile(), 'Linux-'+machine)
            with self.assertRaises(F.Refusal) as refusal:
                A.verify_tools(descriptor(), object(), 'synthetic')
            self.assertEqual(refusal.exception.row, 'node_fault')
            mapped = M.classify(M.Observation(refusal.exception.row), descriptor())
            self.assertEqual((mapped['outcome'], mapped['failure_class']), ('DEFER', 'D-node'))

    def test_real_linux_aarch64_profile_refuses(self):
        self.assert_linux_refused('aarch64')

    def test_real_linux_x86_64_profile_refuses(self):
        self.assert_linux_refused('x86_64')

    def test_host_profile_reads_both_platform_parts(self):
        for system, machine in (('Darwin', 'arm64'), ('Linux', 'aarch64'), ('Linux', 'x86_64')):
            with self.subTest(system=system, machine=machine), \
                 patch.object(A, 'platform', SimpleNamespace(system=lambda: system, machine=lambda: machine)):
                self.assertEqual(A.host_profile(), system+'-'+machine)


class Descriptor(MacProfile):
    def test_tool_pins_uses_injected_profile_without_native_tools(self):
        tools = SimpleNamespace(comparator='synthetic', nanoda='synthetic',
                                lean_replay='synthetic', exporter='synthetic')
        environment = A.L.production_descriptor()['artifact_profiles']['Darwin-arm64']
        with patch.object(A, 'platform', SimpleNamespace(system=lambda: 'Linux', machine=lambda: 'x86_64')), \
             patch.object(A.D, 'digest', return_value='6'*64):
            pins = A.tool_pins(tools, 'synthetic')
        expected_runtime = dict(binary=environment['lean_binary_sha256'], runtime=environment['runtime_libraries'])
        self.assertEqual(pins['lean_runtime'], C.sha(C.canonical(expected_runtime)))
        self.assertEqual(pins['imports'], C.sha(C.canonical(environment['olean_roots'])))

    def test_reference_interpreter_is_recorded_in_descriptor(self):
        value=descriptor()
        self.assertEqual(value.get('reference_interpreter'),dict(implementation='CPython',version='3.9.6'))
        self.assertEqual(value.get('json_nesting_limit'),127)
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


class Front(MacProfile):
    def test_strict_native_utf8_json_grammar(self):
        for raw in (b'\xef\xbb\xbf{}',b'\xff\xfe{\x00}\x00',b'{"x":"\\ud800"}',b'{"x":"\xed\xa0\x80"}'):
            with self.subTest(raw=raw),self.assertRaises(ValueError):C.loads(raw)
        self.assertEqual(C.loads(b'{"x":"\\ud834\\udd1e"}'),{'x':'\U0001d11e'})

    def test_json_depth_is_bounded_before_the_interpreter_parser(self):
        raw=b'{"meta":{"format":{"version":"3.1.0"},"data":'+b'['*130+b'0'+b']'*130+b'}}'
        with patch.object(C.json,'loads',side_effect=AssertionError('parser called before explicit depth guard')):
            with self.assertRaises(ValueError):C.loads(raw)
        self.path.write_bytes(raw+b'\n')
        with self.assertRaisesRegex(F.Refusal,'structural_cap'):F.read_export(self.path)
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
        MacProfile.setUp(self)  # This fixture is also reused by ReservedNames.
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
        with self.assertRaises(ValueError):C.loads(b'{"ie":-0}')
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


class ReservedNames(MacProfile):
    def test_anonymous_main_private_name_is_reserved(self):
        self.add_definition(('_private',0,'BooleCS','X'));self.rejected('reserved_name')

    def test_private_numeric_module_components_cannot_hide_reserved_names(self):
        self.add_definition(('_private','M',0,'Foo',1,'BooleCS','X'));self.rejected('reserved_name')
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
    def test_cli_authenticates_package_before_opening_decoder_status(self):
        from zk_registry.m3_gate import __main__ as main
        directory = Path(self.tmp.name)/'decoder-cli'
        directory.mkdir()
        for name, raw in [('descriptor', C.encode(self.desc)), ('inventory', self.inventory.raw)]:
            (directory/name).write_bytes(raw)
        for index, (package, expected) in enumerate(((b'{}', ('VOID', 0x0801)),
                (C.canonical(self.pkg), ('DEFER', 0)))):
            (directory/'package').write_bytes(package)
            context = dict(self.context, package_sha256=C.sha(package))
            (directory/'context').write_bytes(C.canonical(context))
            argv = ['gate_m3', '--gate-digest', C.identity(self.desc)]
            for name in ('descriptor', 'package', 'inventory', 'context'):
                argv += ['--'+name, str(directory/name)]
            argv += ['--bundle', str(directory/'missing-bundle'), '--reference', str(directory/'missing-reference'),
                     '--decoder-status', str(directory/'missing-status'), '--tools-root', 'synthetic',
                     '--counter-driver', 'synthetic', '--work', str(directory/('work-'+str(index)))]
            stdout = io.StringIO()
            with patch.object(sys, 'argv', argv), patch.object(A.D.Tools, 'load', return_value=object()), \
                 patch.object(A, 'verify_tools'), patch.object(A.FC, 'load_authority', return_value=self.authority), contextlib.redirect_stdout(stdout), \
                 contextlib.redirect_stderr(io.StringIO()):
                code = main.main()
            self.assertEqual(code, 0)
            result = C.loads(stdout.getvalue()[len(A.MARKER):])
            self.assertEqual((result['outcome'], result['reason']), expected)
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_cli_oversized_package_is_0801_after_whole_address_check(self):
        from zk_registry.m3_gate import __main__ as main
        directory = Path(self.tmp.name)/'package-limit-cli'
        directory.mkdir()
        package = b'P'*(1048576+4096)
        context = dict(self.context, package_sha256=C.sha(package))
        for name, raw in [('descriptor', C.encode(self.desc)), ('inventory', self.inventory.raw),
                          ('context', C.canonical(context))]:
            (directory/name).write_bytes(raw)
        for index, (raw, expected) in enumerate(((package, ('VOID', 0x0801)),
                (package[:-1]+b'X', ('STOP', 0)))):
            (directory/'package').write_bytes(raw)
            argv = ['gate_m3', '--gate-digest', C.identity(self.desc)]
            for name in ('descriptor', 'inventory', 'context', 'package'):
                argv += ['--'+name, str(directory/name)]
            argv += ['--bundle', str(directory/'absent-bundle'), '--reference', str(directory/'absent-reference'),
                     '--tools-root', 'synthetic', '--counter-driver', 'synthetic',
                     '--work', str(directory/('work-'+str(index)))]
            stdout = io.StringIO()
            with self.subTest(corrupt_tail=index), patch.object(sys, 'argv', argv), \
                 patch.object(A.D.Tools, 'load', return_value=object()), patch.object(A, 'verify_tools'), \
                 contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(main.main(), 0)
                reply = C.loads(stdout.getvalue()[len(A.MARKER):])
                self.assertEqual((reply['outcome'], reply['reason']), expected)

    def test_quarantine_preserves_a_valid_authenticated_statement(self):
        _, _, context = self.invoke('quarantine-bound-source')
        statement = self.pkg['statement']
        mapped = M.classify(M.Observation('quarantine_void', fork_authorized=True), self.desc)
        selected = M.quarantine_statement(context, statement, fork_authorized=True)
        self.assertEqual(selected, statement)
        reply = A.reply(self.desc, context, mapped, selected, M.not_run(), 0,
                        fork_authorized=True, authenticated_statement=statement)
        A.decode_reply(C.canonical(reply), self.desc, context,
                       fork_authorized=True, authenticated_statement=statement)
        fallback = M.quarantine_fingerprint(context)
        with self.assertRaises(ValueError):
            A.decode_reply(C.canonical(dict(reply, statement_id=fallback)), self.desc, context,
                           fork_authorized=True, authenticated_statement=statement)
        for authority in (False, 1, None):
            with self.subTest(authority=authority), self.assertRaises(ValueError):
                M.quarantine_statement(context, M.NO_VALID_STATEMENT, fork_authorized=authority)
        for key in ('chain_id', 'problem_id', 'commit_hash', 'env_id', 'package_sha256'):
            changed = dict(context)
            changed[key] = context[key]+1 if key == 'chain_id' else 'f'*64
            self.assertNotEqual(M.quarantine_fingerprint(changed), fallback)

    def decoder_channel(self, *, context=None, status='ok', cause='none'):
        return dict(version=2, context_sha256=C.sha(C.canonical(self.context if context is None else context)),
            gate_rule_digest=C.identity(self.desc), descriptor_sha256=C.sha(C.encode(self.desc)),
            decoder_sha256=self.decoder_pin, transport_sha256=self.transport_digest,
            status=status, cause=cause, raw_sha256=C.sha(self.bundle) if status == 'ok' else None,
            raw_bytes=len(self.bundle) if status == 'ok' else None)

    def assert_transport_route_order(self, label, expected):
        raw = b'broken bound frame'
        context = dict(self.context, export_digest=C.sha(raw))
        channel = C.canonical(self.decoder_channel(context=context, status='invalid-data', cause='frame'))
        for typed in (False, True):
            with self.subTest(typed=typed):
                result, compare, _ = self.invoke(label+str(typed), bundle=raw,
                    decoder_status=channel if typed else None)
                self.assertEqual((result['row'], result['outcome'], result['reason']), expected)
                self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
                compare.assert_not_called()

    def test_kind_two_precedes_plain_and_typed_transport_errors(self):
        self.context['leaf_kind'] = 2
        self.assert_transport_route_order('kind-two-', ('generator_unavailable', 'VOID', 0x0804))

    def test_lane_two_precedes_plain_and_typed_transport_errors(self):
        self.context['lane'] = 2
        self.assert_transport_route_order('lane-two-', ('node_fault', 'DEFER', 0))

    def test_legacy_decoder_evidence_without_release_and_transport_binding_is_refused(self):
        unbound = C.canonical(dict(version=1, context_sha256=C.sha(C.canonical(self.context)),
            status='ok', cause='none', raw_sha256=C.sha(self.bundle), raw_bytes=len(self.bundle)))
        for index, source in enumerate(('1'*64, 'a'*64)):
            self.desc['source_sha256'] = source
            with self.subTest(release=index):
                result, compare, _ = self.invoke('unbound-decoder-'+str(index), decoder_status=unbound)
                self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
                compare.assert_not_called()

    def test_decoder_channel_is_strict_and_success_is_bound_to_actual_output(self):
        channel = self.decoder_channel()
        result, compare, _ = self.invoke('decoder-success', decoder_status=C.canonical(channel))
        self.assertEqual(result['outcome'], 'ACCEPT'); compare.assert_called_once()
        variants = [dict(channel, context_sha256='f'*64), dict(channel, gate_rule_digest='f'*64),
                    dict(channel, descriptor_sha256='f'*64), dict(channel, decoder_sha256='f'*64),
                    dict(channel, transport_sha256='f'*64), dict(channel, status='invalid-data', cause='write'),
                    dict(channel, raw_sha256='f'*64), dict(channel, raw_bytes=len(self.bundle)+1),
                    dict(channel, version=True), dict(channel, raw_bytes=True), dict(channel, unknown=1)]
        for index, bad in enumerate(variants):
            result, compare, _ = self.invoke('bad-decoder-'+str(index), decoder_status=C.canonical(bad))
            self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
            compare.assert_not_called()

    def test_bound_decoder_channel_cannot_cross_releases_or_omit_provenance(self):
        channel = self.decoder_channel()
        for index, field in enumerate(('gate_rule_digest', 'descriptor_sha256', 'decoder_sha256', 'transport_sha256')):
            bad = dict(channel); del bad[field]
            result, compare, _ = self.invoke('missing-decoder-binding-'+str(index), decoder_status=C.canonical(bad))
            self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
            compare.assert_not_called()
        self.desc['source_sha256'] = 'a'*64
        result, compare, _ = self.invoke('cross-release-decoder', decoder_status=C.canonical(channel))
        self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
        compare.assert_not_called()

    def test_decoder_failure_cannot_hide_real_output_address_corruption(self):
        channel = C.canonical(self.decoder_channel(status='invalid-data', cause='digest'))
        with patch.object(A, 'verify_tools'), patch.object(A.FC, 'load_authority', return_value=self.authority):
            result = A.run(b'corrupt output', C.canonical(self.pkg), self.inventory.raw, self.context,
                C.encode(self.desc), C.identity(self.desc), self.path, object(), 'synthetic',
                Path(self.tmp.name)/'decoder-corruption', decoder_status=channel,
                decoder_pin=self.decoder_pin, transport_digest=self.transport_digest)
        self.assertEqual((result['row'], result['outcome']), ('input_integrity', 'STOP'))

    def test_cli_explicit_decoder_no_output_and_required_node_bindings(self):
        from zk_registry.m3_gate import __main__ as main
        directory = Path(self.tmp.name)/'decoder-no-output-cli'; directory.mkdir()
        channel = self.decoder_channel(status='invalid-data', cause='frame')
        for name, raw in [('descriptor', C.encode(self.desc)), ('inventory', self.inventory.raw),
                          ('context', C.canonical(self.context)), ('package', C.canonical(self.pkg)),
                          ('decoder-status', C.canonical(channel))]:
            (directory/name).write_bytes(raw)
        for index, pinned in enumerate((True, False)):
            argv = ['gate_m3', '--gate-digest', C.identity(self.desc), '--no-decoded-output']
            for name in ('descriptor', 'inventory', 'context', 'package', 'decoder-status'):
                argv += ['--'+name, str(directory/name)]
            argv += ['--reference', str(directory/'absent-reference'), '--tools-root', 'synthetic',
                     '--counter-driver', 'synthetic', '--work', str(directory/('work-'+str(index)))]
            if pinned: argv += ['--decoder-pin', self.decoder_pin, '--transport-digest', self.transport_digest]
            stdout = io.StringIO()
            with patch.object(sys, 'argv', argv), patch.object(A.D.Tools, 'load', return_value=object()), \
                 patch.object(A, 'verify_tools'), patch.object(A.FC, 'load_authority', return_value=self.authority), \
                 contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(main.main(), 0)
            reply = C.loads(stdout.getvalue()[len(A.MARKER):])
            self.assertEqual((reply['outcome'], reply['reason']), ('REJECT', 0x0106) if pinned else ('DEFER', 0))

    def test_invalid_decoder_status_with_present_bound_output_is_node_defer(self):
        for index, cause in enumerate(('frame', 'checksum', 'digest')):
            channel = C.canonical(self.decoder_channel(status='invalid-data', cause=cause))
            result, compare, _ = self.invoke('contradictory-decoder-'+str(index), decoder_status=channel)
            self.assertEqual((result['row'], result['outcome'], result['failure_class']),
                             ('node_fault', 'DEFER', 'D-node'))
            compare.assert_not_called()

    def test_python_output_write_fault_stays_node_defer(self):
        original_open = Path.open
        def write_fault(path, *args, **kwargs):
            if path.name == 'solution.ndjson':
                raise OSError(28, 'synthetic host write failure')
            return original_open(path, *args, **kwargs)
        with patch.object(Path, 'open', write_fault):
            result, compare, _ = self.invoke('host-output-write')
        self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
        compare.assert_not_called()

    def test_typed_decoder_data_and_host_write_faults_are_distinct(self):
        undecoded = None  # explicit no output; present matching bytes contradict invalid-data
        context = dict(self.context)
        for index, (status, cause, row, outcome, reason) in enumerate((
                ('invalid-data', 'frame', 'front_invalid', 'REJECT', 0x0106),
                ('unavailable', 'write', 'node_fault', 'DEFER', 0),
                ('unavailable', 'read', 'node_fault', 'DEFER', 0),
                ('unavailable', 'resource', 'node_fault', 'DEFER', 0))):
            channel = C.canonical(self.decoder_channel(context=context, status=status, cause=cause))
            result, compare, bound = self.invoke('decoder-'+str(index), bundle=undecoded,
                                                 decoder_status=channel)
            self.assertEqual((result['row'], result['outcome'], result['reason']), (row, outcome, reason))
            self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
            if outcome == 'DEFER': self.assertEqual(result['failure_class'], 'D-node')
            compare.assert_not_called()
            A.decode_reply(C.canonical(result), self.desc, bound)
        # An old untyped exit1 is never enough to blame the input.
        result, compare, _ = self.invoke('decoder-untyped-exit', bundle=undecoded,
                                         decoder_status=C.canonical(dict(exit_code=1)))
        self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
        compare.assert_not_called()

    def test_quarantine_never_infers_no_valid_statement_from_omission_or_none(self):
        original, _, context = self.invoke('quarantine-explicit-source')
        mapped = M.classify(M.Observation('quarantine_void', fork_authorized=True), self.desc)
        fallback = M.quarantine_fingerprint(context)
        reply = dict(original, **{k:mapped[k] for k in
            ('row','outcome','reason','action','failure_class','detail','gate_error','native_followup')})
        reply.update(statement_id=fallback, fuel=0, kernels=M.not_run())
        for supplied in ({}, {'authenticated_statement': None}):
            with self.subTest(api='decode', supplied=supplied), self.assertRaises(ValueError):
                A.decode_reply(C.canonical(reply), self.desc, context, fork_authorized=True, **supplied)
            with self.subTest(api='reply', supplied=supplied), self.assertRaises(ValueError):
                A.reply(self.desc, context, mapped, fallback, M.not_run(), 0,
                        fork_authorized=True, **supplied)
            with self.subTest(api='selector', supplied=supplied), self.assertRaises((ValueError, TypeError)):
                M.quarantine_statement(context, fork_authorized=True, **supplied)

    def test_authorized_statementless_quarantine_has_its_own_exact_fingerprint(self):
        original, _, context = self.invoke('quarantine-source')
        material = (b'BOOLE-M3/QUARANTINE-STATEMENT/v1\0' + context['chain_id'].to_bytes(8, 'big')
            + b''.join(bytes.fromhex(context[k]) for k in ('problem_id', 'commit_hash', 'env_id', 'package_sha256'))
            + (0x0802).to_bytes(2, 'big'))
        expected = hashlib.sha256(material).hexdigest()
        mapped = M.classify(M.Observation('quarantine_void', fork_authorized=True), self.desc)
        reply = dict(original, **{k:mapped[k] for k in
            ('row','outcome','reason','action','failure_class','detail','gate_error','native_followup')})
        reply.update(statement_id=expected, fuel=0, kernels=M.not_run())
        A.decode_reply(C.canonical(reply), self.desc, context, fork_authorized=True, authenticated_statement=M.NO_VALID_STATEMENT)
        for bad in ('0'*64, M.void_statement(context), 'f'*64):
            with self.subTest(statement=bad), self.assertRaises(ValueError):
                A.decode_reply(C.canonical(dict(reply, statement_id=bad)), self.desc, context,
                               fork_authorized=True, authenticated_statement=M.NO_VALID_STATEMENT)
        with self.assertRaises(ValueError):
            A.decode_reply(C.canonical(reply), self.desc, context)
        for row in ('accept', 'front_invalid', 'package_invalid', 'fuel_binding_invalid'):
            mapped = M.classify(M.Observation(row), self.desc)
            other = dict(original, **{k:mapped[k] for k in
                ('row','outcome','reason','action','failure_class','detail','gate_error','native_followup')})
            other['statement_id'] = expected
            with self.subTest(row=row), self.assertRaises(ValueError):
                A.decode_reply(C.canonical(other), self.desc, context)

    def test_package_authentication_precedes_transport_and_reference_access(self):
        bad_target = dict(self.pkg, target=False)
        noncanonical = C.canonical(dict(reversed(list(self.pkg.items()))))
        cases = (b'{}', C.canonical(bad_target), noncanonical)
        for index, package in enumerate(cases):
            for phase in ('first', 'retry', 'replay'):
                result, compare, context = self.invoke('package-first-'+str(index)+'-'+phase,
                    package_bytes=package, bundle=b'broken transport', phase=phase,
                    reference=Path(self.tmp.name)/'absent-reference',
                    decoder_status=A.DC.Source(str(Path(self.tmp.name)/'absent-decoder-status')))
                self.assertEqual((result['row'], result['outcome'], result['reason']),
                                 ('package_invalid', 'VOID', 0x0801))
                self.assertEqual(result['statement_id'], M.void_statement(context))
                self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
                compare.assert_not_called()
                A.decode_reply(C.canonical(result), self.desc, context)
        result, compare, _ = self.invoke('valid-package-broken-transport', bundle=b'broken transport')
        self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0106))
        compare.assert_not_called()

    def test_missing_or_corrupt_bound_reference_stays_node_defer(self):
        corrupt = Path(self.tmp.name)/'corrupt-reference'
        corrupt.write_bytes(b'local corruption')
        for index, path in enumerate((Path(self.tmp.name)/'missing-reference', corrupt)):
            result, compare, context = self.invoke('unavailable-reference-'+str(index), reference=path)
            self.assertEqual((result['outcome'], result['failure_class']), ('DEFER', 'D-node'))
            compare.assert_not_called()
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_measured_reference_export_mismatch_is_a_published_cap_void(self):
        row = dict(next(iter(self.authority['references'].values())), reference_export='a'*64)
        key = A.FC.digest(row)
        self.authority['references'] = {key: row}
        self.pkg['fuel_cap']['cap']['reference'] = key
        self.pkg['fuel_cap']['sha256'] = A.FC.digest(self.pkg['fuel_cap']['cap'])
        for phase in ('first', 'retry', 'replay'):
            result, compare, context = self.invoke('wrong-measured-reference-'+phase, phase=phase)
            self.assertEqual((result['row'], result['outcome'], result['reason']),
                             ('fuel_binding_invalid', 'VOID', 0x0803))
            self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
            compare.assert_not_called()
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_measured_reference_void_precedes_absent_reference_and_submission_rejects(self):
        row = dict(next(iter(self.authority['references'].values())), reference_export='a'*64)
        key = A.FC.digest(row)
        self.authority['references'] = {key: row}
        self.pkg['fuel_cap']['cap']['reference'] = key
        self.pkg['fuel_cap']['sha256'] = A.FC.digest(self.pkg['fuel_cap']['cap'])
        absent = Path(self.tmp.name)/'absent-published-reference'
        for phase in ('first', 'retry', 'replay'):
            for index, bundle in enumerate((self.bundle, b'broken bound frame')):
                with self.subTest(phase=phase, frame=index):
                    result, compare, context = self.invoke('reference-order-'+phase+str(index),
                        bundle=bundle, phase=phase, reference=absent)
                    self.assertEqual((result['row'], result['outcome'], result['reason']),
                                     ('fuel_binding_invalid', 'VOID', 0x0803))
                    self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
                    compare.assert_not_called()
                    A.decode_reply(C.canonical(result), self.desc, context)

    def test_full_without_public_counter_void_precedes_reference_environment_and_transport(self):
        self.pkg.update(mode='full', fuel_cap=None)
        absent = Path(self.tmp.name)/'absent-full-reference'
        cases = (('reference', absent, self.bundle, '7'*64),
                 ('environment', self.path, self.bundle, 'a'*64),
                 ('transport', self.path, b'broken bound frame', '7'*64))
        for name, reference, bundle, environment in cases:
            self.context['env_id'] = environment
            for phase in ('first', 'retry', 'replay'):
                with self.subTest(case=name, phase=phase):
                    result, compare, context = self.invoke('full-order-'+name+'-'+phase,
                        reference=reference, bundle=bundle, phase=phase)
                    self.assertEqual((result['row'], result['outcome'], result['reason']),
                                     ('fuel_binding_invalid', 'VOID', 0x0803))
                    self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
                    compare.assert_not_called()
                    A.decode_reply(C.canonical(result), self.desc, context)

    def test_cap_environment_void_precedes_local_reference_access(self):
        row = dict(next(iter(self.authority['references'].values())), environment='9'*64)
        key = A.FC.digest(row)
        self.authority['references'] = {key: row}
        self.pkg['fuel_cap']['cap'].update(environment='9'*64, reference=key)
        self.pkg['fuel_cap']['sha256'] = A.FC.digest(self.pkg['fuel_cap']['cap'])
        # Resolve a valid authority/cap pair; only its package binding is wrong.
        self.assertEqual(A.FC.resolve(self.pkg['fuel_cap'], self.authority)['environment'], '9'*64)
        for index, reference in enumerate((self.path, Path(self.tmp.name)/'absent-cap-environment-reference')):
            for phase in ('first', 'retry', 'replay'):
                with self.subTest(reference=index, phase=phase):
                    result, compare, context = self.invoke('cap-env-'+str(index)+'-'+phase,
                        reference=reference, phase=phase)
                    self.assertEqual((result['row'], result['outcome'], result['reason']),
                                     ('fuel_binding_invalid', 'VOID', 0x0803))
                    self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
                    compare.assert_not_called()
                    A.decode_reply(C.canonical(result), self.desc, context)

    def test_open_environment_mismatch_is_a_terminal_front_rejection(self):
        self.context['env_id'] = 'a'*64  # another environment allowed by OPEN's caller
        for phase in ('first', 'retry', 'replay'):
            result, compare, context = self.invoke('other-open-env-'+phase, phase=phase)
            self.assertEqual((result['row'], result['outcome'], result['reason'], result['action']),
                             ('front_invalid', 'REJECT', 0x0106, 'apply'))
            self.assertEqual((result['kernels'], result['fuel']), (M.not_run(), 0))
            compare.assert_not_called()
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_unproducible_tool_fault_row_is_refused(self):
        result,_,context=self.invoke('invalid-0307')
        result.update(row='tool_fault',outcome='REJECT',reason=0x0307)
        with self.assertRaises(ValueError):A.decode_reply(C.canonical(result),self.desc,context)

    def test_public_counter_corpus_mismatch_is_node_defer(self):
        self.pkg['mode']='full'
        result,compare,context=self.invoke('wrong-public-corpus',public_counter=True)
        self.assertEqual((result['outcome'],result['failure_class']),('DEFER','D-node'))
        compare.assert_not_called();A.decode_reply(C.canonical(result),self.desc,context)
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
        self.decoder_pin = 'd'*64
        self.transport_digest = 'e'*64
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

    def invoke(self, label, native=None, bundle=DEFAULT_BUNDLE, phase='first', package_bytes=None, authority_error=None, reference=None, public_counter=False, decoder_status=None):
        context = dict(self.context)
        raw = self.bundle if bundle is DEFAULT_BUNDLE else bundle
        if raw is not None:
            context['export_digest'] = C.sha(raw)
        encoded = C.canonical(self.pkg) if package_bytes is None else package_bytes
        context['package_sha256'] = C.sha(encoded)
        with patch.object(A.Sandbox, 'development', return_value=A.Sandbox.fixture()), \
             patch.object(A, 'verify_tools'), patch.object(A.FC, 'load_authority', return_value=self.authority, side_effect=authority_error), \
             patch.object(A.FC, 'bind', return_value=A.FC.Budget(256, 256, '7'*64, 'synthetic', '8'*64, '9'*64)), \
             patch.object(A.D, 'compare_exports', return_value=self.native if native is None else native) as compare:
            result = A.run(raw, encoded, self.inventory.raw, context, C.encode(self.desc), C.identity(self.desc),
                self.path if reference is None else reference, object(), 'synthetic', Path(self.tmp.name)/label, phase=phase,public_counter=public_counter, decoder_status=decoder_status,
                decoder_pin=self.decoder_pin if decoder_status is not None else None,
                transport_digest=self.transport_digest if decoder_status is not None else None)
        return result, compare, context

    def input_reference(self):
        reference=Path(self.tmp.name)/'trusted-reference.ndjson'
        reference.write_bytes(self.path.read_bytes())
        return reference

    def assert_input_reject(self,label,reference,reason):
        self.write(self.rows)
        result,compare,context=self.invoke(label,bundle=F.pack(self.path.read_bytes()),reference=reference)
        self.assertEqual((result['outcome'],result['reason']),('REJECT',reason))
        compare.assert_not_called();A.decode_reply(C.canonical(result),self.desc,context)

    def test_submission_unbound_universe_parameter_is_malformed(self):
        reference=self.input_reference()
        self.rows += [{'in':6,'str':dict(pre=0,str='u')},{'il':1,'param':6},{'ie':99,'sort':1}]
        next(row['thm'] for row in self.rows if 'thm' in row)['type']=99
        self.assert_input_reject('unbound-universe',reference,0x0106)

    def test_submission_nat_literal_without_nat_is_a_type_rejection(self):
        reference=self.input_reference()
        self.rows.append({'ie':99,'natVal':'1'})
        next(row['thm'] for row in self.rows if 'thm' in row)['value']=99
        self.assert_input_reject('missing-nat',reference,0x0301)

    def test_submission_lone_surrogate_is_malformed(self):
        reference=self.input_reference()
        self.rows.append({'ie':99,'strVal':'\ud800'})
        next(row['thm'] for row in self.rows if 'thm' in row)['value']=99
        self.assert_input_reject('surrogate',reference,0x0106)

    def test_submission_shared_dag_hash_work_cap_is_a_size_rejection(self):
        reference=self.input_reference();previous=0
        for index in range(6000):
            expr=100+index;self.rows.append({'ie':expr,'app':dict(fn=previous,arg=0)});previous=expr
        shared=previous;target=0
        for index in range(1001):
            param=10000+index;name=20000+index;constant=30000+index;application=40000+index
            self.rows += [{'in':param,'str':dict(pre=0,str='U'+str(index))},
                          {'in':name,'str':dict(pre=0,str='D'+str(index))},
                          {'def':dict(name=name,levelParams=[param],type=0,value=shared,hints='opaque',safety='safe',all=[name])},
                          {'ie':constant,'const':dict(name=name,us=[])},
                          {'ie':application,'app':dict(fn=target,arg=constant)}]
            target=application
        next(row['thm'] for row in self.rows if 'thm' in row)['type']=target
        self.assert_input_reject('shared-dag-cap',reference,0x0108)

    def test_unknown_submission_computation_fault_is_still_node_defer(self):
        for index,error in enumerate((RuntimeError('trusted bug'),KeyError('trusted bug'),ValueError('trusted bug'))):
            with patch.object(F,'payload',side_effect=error):
                result,compare,context=self.invoke('unexpected-'+str(index))
            self.assertEqual((result['outcome'],result['failure_class']),('DEFER','D-node'))
            compare.assert_not_called();A.decode_reply(C.canonical(result),self.desc,context)

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
        reference = self.input_reference()
        original = copy.deepcopy(self.rows)
        for index, spelling in enumerate(('λ', '', 'goal.with.dot')):
            with self.subTest(spelling=spelling):
                self.rows = copy.deepcopy(original)
                self.rows += [{'in':6,'str':dict(pre=0,str=spelling)},
                              {'def':dict(name=6,levelParams=[],type=0,value=1,hints='opaque',safety='safe',all=[6])}]
                self.write(self.rows)
                result, compare, _ = self.invoke('name-grammar-'+str(index), bundle=F.pack(self.path.read_bytes()), reference=reference)
                self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0103))
                compare.assert_not_called()

    def test_pre_kernel_resource_and_output_caps_are_terminal_rejections(self):
        for index, (verdict, reason, expected) in enumerate((('INVALID','resource-cap',0x0108),)):
            native = dict(verdict=verdict, reason=reason, rc=2, comparator_status=[
                dict(version=1, stage='policy', reason=reason), dict(version=1, stage='final', rc=2)])
            result, _, context = self.invoke('pre-kernel-cap-'+str(index), native)
            self.assertEqual((result['outcome'], result['reason'], result['kernels'], result['fuel']),
                             ('REJECT', expected, M.not_run(), 0))
            A.decode_reply(C.canonical(result), self.desc, context)

    def test_real_closure_output_cap_is_terminal_without_fabricated_debits(self):
        native=dict(stage='comparator',verdict='ERROR',reason='output-cap',rc=-9,
                    process_receipt=dict(streams=dict(stdout=dict(capped=True))))
        result,_,context=self.invoke('real-output-cap',native)
        self.assertEqual((result['outcome'],result['reason'],result['failure_class'],result['fuel']),
                         ('REJECT',0x0109,'S',0))
        self.assertTrue(all(v==dict(state='unknown',debit=None,charged=0) for v in result['kernels'].values()))
        A.decode_reply(C.canonical(result),self.desc,context)

    def test_terminal_output_cap_cannot_claim_another_execution_state(self):
        result,_,context=self.invoke('output-forgery',dict(verdict='ERROR',reason='output-cap',rc=-9))
        boolean=M.output_limit();boolean['lean']['charged']=False
        for states in (M.not_run(),M.unknown(),boolean):
            with self.subTest(states=states),self.assertRaises(ValueError):
                A.decode_reply(C.canonical(dict(result,kernels=states)),self.desc,context)

    def test_cli_binding_precedes_size_and_hashes_the_tail(self):
        from zk_registry.m3_gate import __main__ as main
        import hashlib
        directory=Path(self.tmp.name)/'cli';directory.mkdir()
        for name,raw in [('descriptor',C.encode(self.desc)),('package',C.canonical(self.pkg)),('inventory',self.inventory.raw)]:
            (directory/name).write_bytes(raw)
        for index,matching in enumerate((True,False)):
            bundle=directory/'bundle'
            with bundle.open('wb') as stream:stream.truncate(F.RAW_MAX+1024)
            digest=hashlib.sha256()
            with bundle.open('rb') as stream:
                for chunk in iter(lambda:stream.read(1024**2),b''):digest.update(chunk)
            context=dict(self.context,export_digest=digest.hexdigest())
            (directory/'context').write_bytes(C.canonical(context))
            if not matching:
                with bundle.open('r+b') as stream:stream.seek(-1,2);stream.write(b'X')
            argv=['gate_m3','--gate-digest',C.identity(self.desc)]
            for name in ('descriptor','package','inventory','context','bundle'):argv+=['--'+name,str(directory/name)]
            argv+=['--reference',str(self.path),'--tools-root','synthetic','--counter-driver','synthetic',
                   '--work',str(directory/('work-'+str(index)))]
            stdout=io.StringIO()
            with patch.object(sys,'argv',argv),patch.object(A.D.Tools,'load',return_value=object()), \
                 patch.object(A,'verify_tools'), patch.object(A.FC,'load_authority',return_value=self.authority),contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(io.StringIO()):
                code=main.main()
            self.assertEqual(code,0);value=C.loads(stdout.getvalue()[len(A.MARKER):])
            expected=('REJECT',0x0108,'') if matching else ('STOP',0,'D-input')
            self.assertEqual((value['outcome'],value['reason'],value['failure_class']),expected)
            A.decode_reply(C.canonical(value),self.desc,context)

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
        self.pkg.update(target=[['str', 'goal.with.dot']])
        result, compare, _ = self.invoke('target-representation', bundle=F.pack(self.path.read_bytes()))
        self.assertEqual((result['outcome'], result['reason']), ('REJECT', 0x0103))
        compare.assert_not_called()

    def test_library_terminal_after_own_guards_in_all_three_phases(self):
        reference = self.input_reference()
        self.rows.insert(1, {'in': 6, 'str': dict(pre=0, str='allowedLibrary')})
        self.rows.append({'def': dict(name=6, levelParams=[], type=0, value=1, hints='opaque', safety='safe', all=[6])})
        self.write(self.rows); export = F.read_export(self.path)
        table = self.inventory.data(); name = (('str', 'allowedLibrary'),)
        table['library'][F.name_key(name)] = F.payload(export, name)
        raw = C.canonical(table); self.inventory = F.Inventory(raw, C.sha(raw))
        self.pkg.update(inventory_sha256=self.inventory.sha256)
        self.desc['artifact_table_sha256'] = self.inventory.sha256
        for phase in ('first', 'retry', 'replay'):
            result, compare, _ = self.invoke('library-'+phase, bundle=F.pack(self.path.read_bytes()),
                                             phase=phase, reference=reference)
            self.assertEqual((result['outcome'], result['reason'], result['failure_class']), ('REJECT', 0x010a, 'S'))
            compare.assert_not_called()
        self.rows.insert(1, {'in': 7, 'str': dict(pre=0, str='badOwn')})
        self.rows.append({'axiom': dict(name=7, levelParams=[], type=0, isUnsafe=False)})
        self.write(self.rows)
        result, compare, _ = self.invoke('own-before-library', bundle=F.pack(self.path.read_bytes()), reference=reference)
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
    ('input_type', 'REJECT', 0x0301, 'apply', ''),
    ('kernel_disagreement', 'REJECT', 0x0302, 'apply', ''),
    ('resource_cap', 'REJECT', 0x0303, 'apply', ''),
    ('depth_cause_missing', 'DEFER', 0, 'repair', 'D-node'),
    ('cap_cause_missing', 'DEFER', 0, 'repair', 'D-node'),
    ('kernel_panic_cause_missing', 'DEFER', 0, 'repair', 'D-node'),
    ('output_terminal', 'REJECT', 0x0109, 'terminal', 'S'),
    ('library_terminal', 'REJECT', 0x010a, 'terminal', 'S'),
    ('transient', 'DEFER', 0, 'retry', 'T'),
    ('input_failure', 'DEFER', 0, 'retry', 'D-input'),
    ('node_fault', 'DEFER', 0, 'repair', 'D-node'),
    ('input_availability', 'STALL', 0, 'wait', ''),
    ('input_integrity', 'STOP', 0, 'preserve', 'D-input'),
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
            for key in ('input_type', 'resource_cap', 'kernel_reject', 'kernel_disagreement', 'library_terminal', 'output_terminal'):
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

    def test_statement_validation_requires_commit_hash_for_every_outcome(self):
        context = dict(chain_id=1, problem_id='1'*64, env_id='2'*64, package_sha256='3'*64)
        for outcome, reason in (('ACCEPT', 0), ('REJECT', 0x0106), ('VOID', 0x0801), ('VOID', 0x0803)):
            statement = M.void_statement(context) if reason == 0x0801 else 'a'*64
            with self.subTest(outcome=outcome, reason=reason), self.assertRaises(ValueError):
                M.check_statement(outcome, reason, statement, context)

    def test_void_fingerprint_cannot_claim_a_proof_or_another_void(self):
        context = dict(chain_id=720926, problem_id='7'*64, commit_hash='6'*64, env_id='8'*64, package_sha256='9'*64)
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
