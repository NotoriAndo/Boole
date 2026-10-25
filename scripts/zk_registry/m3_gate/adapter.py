"""Development data-only gate_m3 entrypoint over the unchanged DET3 tools.

There is no controller, production isolation, activation or implicit EENV5 release
here. The caller authenticates the job context; the adapter binds every byte to it.
"""
from pathlib import Path
import hashlib
import json
import platform
from . import codec as C, front as F, mapping as M
from . import decoder as DC
from .. import closure as D, fuel_caps as FC, lean_runner as L
from .. import counter_identity as CI
from .. import artifact_pins
from ..sandbox import Sandbox

CONTEXT_KEYS = ('chain_id', 'problem_id', 'commit_hash', 'solver', 'env_id',
                'export_digest', 'package_sha256', 'leaf_kind', 'lane', 'slot_fuel')
MARKER = 'BOOLE-M3/GATE/v1 '


def source_release():
    """Conservative Python dependency tree, with portable relative names."""
    root = Path(__file__).resolve().parents[1]
    digest = hashlib.sha256(b'BOOLE-M3/REAL-GATE-SOURCE/v1\0')
    for path in sorted(root.rglob('*.py')):
        name = path.relative_to(root).as_posix().encode(); raw = path.read_bytes()
        digest.update(len(name).to_bytes(4, 'big')+name+len(raw).to_bytes(8, 'big')+raw)
    return digest.hexdigest()


def context_check(context):
    C.fields(context, CONTEXT_KEYS); C.uint(context['chain_id'], True); C.uint(context['slot_fuel'], True)
    if type(context['lane']) is not int or context['lane'] not in (1, 2):
        raise ValueError('lane')
    if type(context['leaf_kind']) is not int or context['leaf_kind'] not in (1, 2):
        raise ValueError('leaf kind')
    for key in ('problem_id', 'commit_hash', 'env_id', 'export_digest', 'package_sha256'):
        C.digest(context[key])
    solver = context['solver']
    if type(solver) is not str or len(solver) != 40 or any(c not in '0123456789abcdef' for c in solver):
        raise ValueError('solver')


def host_profile():
    """The single injectable host boundary; production uses the actual platform."""
    return platform.system()+'-'+platform.machine()


def tool_pins(tools, counter):
    profile = host_profile()
    table = C.loads((Path(D.__file__).parent/'data/acceptance_tools_v2.json').read_bytes())
    environment = L.production_descriptor()['artifact_profiles'][profile]
    return dict(comparator=D.digest(tools.comparator), nanoda=D.digest(tools.nanoda),
        lean_replay=D.digest(tools.lean_replay), counter_replay=D.digest(counter), exporter=D.digest(tools.exporter),
        lean_runtime=C.sha(C.canonical(dict(binary=environment['lean_binary_sha256'], runtime=environment['runtime_libraries']))),
        imports=C.sha(C.canonical(environment['olean_roots'])), sandbox=C.sha(C.canonical(table['sandbox'])),
        codec=C.sha((Path(__file__).parent/'codec.py').read_bytes()))


def verify_tools(descriptor, tools, counter):
    C.validate(descriptor)
    profile = host_profile()
    if profile != 'Darwin-arm64':
        raise F.Refusal('node_fault', 'A1 has no production controller')
    entry = descriptor['profiles'].get(profile)
    if entry is None or entry['assurance'] != 'development':
        raise F.Refusal('node_fault', 'Mac development profile required')
    expected = L.production_descriptor()['artifact_profiles'][profile]
    actual = artifact_pins.snapshot(L.LeanEnv(tools.kernel_prefix, [], '', {}, '', ''))
    if actual != dict(expected, olean_roots=expected['olean_roots'][:1]):
        raise F.Refusal('node_fault', 'runtime/import artifact mismatch')
    if (tool_pins(tools, counter) != entry['tools'] or descriptor['source_sha256'] != source_release()
            or descriptor['caps_sha256'] != D.digest(Path(L.__file__).parent/'data/testnet_caps_v2.json')
            or descriptor['fuel_authority_sha256'] != D.digest(Path(L.__file__).parent/'data/fuel_cap_authority_v1.json')
            or descriptor['acceptance_environment_sha256'] != D.digest(Path(L.__file__).parent/'data/acceptance_environment_v2.json')
            or descriptor['acceptance_tools_sha256'] != D.digest(Path(L.__file__).parent/'data/acceptance_tools_v2.json')):
        raise F.Refusal('node_fault', 'release pin mismatch')


def observation(record):
    verdict, reason = record.get('verdict'), record.get('reason')
    if verdict == 'PASS':
        return M.Observation('accept')
    if verdict == 'INVALID':
        row = {'kernel-rejection': 'kernel_reject', 'kernel-disagreement': 'kernel_disagreement',
               'resource-cap': 'resource_cap', 'tool-fault:replay': 'kernel_panic_cause_missing',
               'disallowed-axiom': 'trusted_disagreement', 'comparator-statement-mismatch': 'trusted_disagreement',
               'certified-base-mismatch': 'trusted_disagreement'}.get(reason, 'malformed_result')
        return M.Observation(row, detail='malformed-record' if row == 'trusted_disagreement' else '')
    if verdict == 'ERROR':
        if reason == 'library-outside-certified-base':
            return M.Observation('library_terminal')
        # Raw transport truncation is not the structured submission output cap.
        if reason == 'output-cap':
            return M.Observation('output_terminal')
        if reason in ('resource-unavailable', 'capture-failed'):
            return M.Observation('transient')
        if reason in ('replay-unavailable', 'silent-exit'):
            return M.Observation('input_failure', detail='malformed-record' if reason == 'silent-exit' else record.get('detail', ''))
        return M.Observation('node_fault')
    return M.Observation('malformed_result')


def axioms(export, own):
    """Pure closure traversal, including proof terms, before a terminal S exit."""
    pending = list(own); seen = set()
    while pending:
        name = pending.pop()
        if name in seen:
            continue
        seen.add(name)
        if name not in export.declarations:
            raise F.SubmissionTypeError('implicit type/dependency absent')
        kind, value = export.declarations[name]
        if kind == 'axiom' and name not in F.ALLOWED_AXIOMS:
            raise F.Refusal('axiom_rejected', 'axiom closure')
        starts = [value['type']] + ([value['value']] if 'value' in value else [])
        if 'rules' in value:
            starts.extend(rule['rhs'] for rule in value['rules'])
        pending.extend(export.dependencies(starts, set()))
        pending.extend(export.groups.get(name, ()))


def reply(descriptor, context, mapped, statement, states, fuel, counter=None, *,
          fork_authorized=False, authenticated_statement=None):
    if mapped['outcome'] in ('ACCEPT', 'REJECT', 'VOID'):
        M.check_statement(mapped['outcome'], mapped['reason'], statement, context,
                          fork_authorized=fork_authorized, authenticated_statement=authenticated_statement)
        C.uint(fuel)
        intake=(mapped['row']=='output_terminal' and states==M.output_limit() and fuel==0)
        if mapped['row']=='output_terminal' and not intake:
            raise ValueError('invalid terminal intake execution state')
        if fuel > context['slot_fuel'] or any(s['state'] == 'unknown' for s in states.values()) and not intake:
            raise ValueError('unbound result fuel')
    value = dict(version=1, gate_rule_digest=C.identity(descriptor), descriptor_sha256=C.sha(C.encode(descriptor)),
        input_digest=C.sha(C.canonical(context)),
        outcome=mapped['outcome'], reason=mapped['reason'], action=mapped['action'],
        failure_class=mapped['failure_class'], detail=mapped['detail'], gate_error=mapped['gate_error'], row=mapped['row'],
        native_followup=mapped['native_followup'], statement_id=statement, fuel=fuel,
        kernels=states, ext=[], assurance='development-only', counter_identity=counter)
    return value


def compare_exports(*args, public_counter=False, **kwargs):
    """Keep full-corpus qualification distinct from certified own selection.

    Refuse an explicit selection instead of silently dropping it in the legacy
    unbudgeted executor. Existing registry/generator source identities stay frozen.
    """
    if 'candidate_declarations' in kwargs and (public_counter or kwargs.get('fuel_budget') is None):
        raise ValueError('public counter mode refuses candidate_declarations')
    return D.compare_exports(*args, **kwargs)


def check_reference(reference, export, package, target):
    """Trusted work inside the submission phase keeps its node provenance."""
    try:
        reference_path = Path(reference)
        if D.digest(reference_path) != package['reference_sha256']:
            raise F.Refusal('node_fault', 'reference address')
        trusted = export if export.raw_sha256 == package['reference_sha256'] else F.read_export(reference_path, 'reference')
        if trusted.identity(target)[F.I.SUFFIX] != package['statement']:
            raise F.Refusal('node_fault', 'reference statement binding')
        return reference_path
    except F.Refusal:
        raise
    except Exception:
        raise F.Refusal('node_fault', 'trusted reference computation') from None


def run(bundle, package_bytes, inventory_bytes, context, descriptor_bytes, gate_digest,
        reference, tools, counter_driver, work, *, certified_root=None,
        phase='first', public_counter=False, decoder_status=None):
    """All caller-supplied paths are node-local bindings, never bundle fields."""
    descriptor = C.decode(descriptor_bytes, gate_digest)
    context_check(context)
    directory = Path(work); directory.mkdir(parents=True, exist_ok=False)
    states = M.not_run(); fuel = 0; statement = '0'*64; counters = None
    def finish(row):
        mapped = M.classify(row if type(row) is M.Observation else M.Observation(row), descriptor, phase)
        target = M.void_statement(context) if mapped['outcome'] == 'VOID' and mapped['reason'] == 0x0801 else statement
        return reply(descriptor, context, mapped, target, states, fuel, counters)
    try:
        verify_tools(descriptor, tools, counter_driver)
        inventory = F.Inventory(inventory_bytes, descriptor['artifact_table_sha256'])
        pkg = F.package(package_bytes, context['package_sha256'], inventory)
        statement = pkg['statement']
        decoded = DC.read(decoder_status, context) if decoder_status is not None else None
        bound=F.bind_bundle(bundle,context['export_digest'])
        if decoded is not None and (decoded['raw_sha256'] != bound.digest or decoded['raw_bytes'] != bound.size):
            raise F.Refusal('node_fault', 'decoder output binding')
        if pkg['env_id'] != context['env_id']:
            raise F.Refusal('front_invalid', 'OPEN environment differs from package')
        if bound.size > F.RAW_MAX:
            raise F.Refusal('structural_cap', 'bound bundle byte limit')
        if context['leaf_kind'] == 2:
            raise F.Refusal('generator_unavailable')
        if context['lane'] != 1:
            raise F.Refusal('node_fault', 'lane-2 metric gate not qualified in A1')
        raw = F.unpack(bound.data)
        source = directory/'solution.ndjson'
        with source.open('xb') as stream:
            stream.write(raw)
        source.chmod(0o400)
        export = F.read_export(source)
        with F.submission_boundary():
            checked = F.partition(export, inventory, pkg)
            target = F.fqn(pkg['target'])
            reference_path = check_reference(reference, export, pkg, target)
            if export.identity(target)[F.I.SUFFIX] != statement:
                raise F.Refusal('statement_mismatch')
            axioms(export, checked.own)
            budget = None
            if pkg['mode'] == 'certified-own':
                try:
                    authority = FC.load_authority()
                except (FC.FuelCapError, OSError, ValueError, TypeError, KeyError):
                    raise F.Refusal('node_fault', 'local fuel authority unavailable') from None
                try:
                    resolved = FC.resolve(pkg['fuel_cap'], authority)
                    if resolved['problem'] != pkg['problem'] or resolved['statement'] != statement:
                        raise FC.FuelCapError('package cap identity')
                    if resolved['reference_export'] != pkg['reference_sha256']:
                        raise FC.FuelCapError('package measured reference binding')
                except FC.FuelCapError:
                    raise F.Refusal('fuel_binding_invalid') from None
                try:
                    budget = FC.bind(dict(package_id=pkg['problem'], fuel_cap=pkg['fuel_cap']), statement, certified_root)
                except FC.FuelCapError:
                    raise F.Refusal('node_fault', 'certified state unavailable') from None
                caps = dict(lean=budget.lean, nanoda=budget.nanoda)
            elif public_counter:
                # Explicit qualification mode is limited to the committed public
                # corpus. It cannot make an arbitrary uncapped package eligible.
                fixture = Path(__file__).resolve().parents[3]/'fixtures/public-counter-identity-v1'
                manifest = C.loads((fixture/'manifest.json').read_bytes())
                if target != 'PublicCounter.target' or export.raw_sha256 not in {u['raw_sha256'] for u in manifest['units']}:
                    raise F.Refusal('node_fault','public counter corpus mismatch')
                caps = dict(FC.STRUCTURAL)
            else:
                raise F.Refusal('fuel_binding_invalid', 'no uncapped admission')
            scalar_cap = sum(caps[k]*descriptor['fuel'][k+'_weight'] for k in caps)
            if scalar_cap > context['slot_fuel'] or scalar_cap >= 2**64:
                raise F.Refusal('fuel_binding_invalid', 'slot cap')
            if len(checked.own)*64 > min(caps.values()):
                return finish('resource_cap')
            if checked.library:
                return finish('library_terminal')
            own = [F.selector(name) for name in checked.own]
        if public_counter:
            counters = CI.one(source, directory/'counter-identity', Path(counter_driver), Path(tools.nanoda))
        result = compare_exports(str(reference_path), str(source), target, tools, Sandbox.development(),
            str(directory/'compare'), 1800, ['Classical.choice', 'Quot.sound', 'propext'],
            public_counter=public_counter,
            **(dict(fuel_budget=budget,candidate_declarations=own) if budget is not None else {}))
        (directory/'COMPARATOR.json').write_text(json.dumps(result, indent=2)+'\n')
        mapped_observation = observation(result)
        events = result.get('comparator_status', [])
        if mapped_observation.row=='output_terminal':
            # closure returns before status decoding when capture hits its bound.
            # Preserve unknown actual work; this terminal intake outcome charges
            # zero under bounded-debit/v2, without asserting that no kernel ran.
            states=M.output_limit();fuel=0
            return finish(mapped_observation)
        front_stop = (result.get('rc') == 2 and
                      any(e.get('stage')=='final' and e.get('rc')==2 for e in events) and
                      any(e.get('stage')=='policy' and e.get('reason')==result.get('reason') for e in events) and
                      not any(e.get('stage') in ('fuel','nanoda-fuel','lean','nanoda') for e in events))
        if front_stop and result.get('reason') in ('resource-cap','output-cap'):
            states = M.not_run(); fuel = 0
            if result['reason'] == 'resource-cap':
                mapped_observation = M.Observation('structural_cap')
        elif result.get('verdict') in ('PASS', 'INVALID') or mapped_observation.row == 'output_terminal':
            if (events and not any(e.get('stage') in ('fuel', 'nanoda-fuel', 'lean', 'nanoda') for e in events)
                    and any(e.get('stage') == 'policy' and e.get('reason') in
                            ('comparator-statement-mismatch', 'disallowed-axiom', 'certified-base-mismatch') for e in events)):
                states = M.not_run()  # authenticated pure pre-kernel policy stop
            else:
                try:
                    states, fuel = M.kernel_states(events, result['rc'], result['verdict'], caps, descriptor['fuel'])
                except ValueError:
                    states = M.unknown(); fuel = None
                    if mapped_observation.row == 'kernel_panic_cause_missing':
                        return finish(mapped_observation)
                    return finish('malformed_result')
                # Native exit 3 currently conflates aggregate work, per-item
                # fuel and depth. Only an actually exceeded aggregate counter
                # proves this row's fuel cause; do not invent a depth verdict.
                if mapped_observation.row == 'resource_cap' and not any(
                        state['state'] == 'executed' and state['debit'] > caps[kernel]
                        for kernel, state in states.items()):
                    mapped_observation = M.Observation('cap_cause_missing')
        else:
            states = M.unknown(); fuel = None
        return finish(mapped_observation)
    except F.Refusal as ex:
        # This exception is chosen only by the trusted front end, never parsed
        # out of miner strings or diagnostics.
        return finish(ex.row)
    except Exception:
        states = M.unknown(); fuel = None
        return finish('node_fault')


def decode_reply(raw, descriptor, context, *, fork_authorized=False, authenticated_statement=None):
    """A strict final channel; no report/exit/marker disagreement is accepted."""
    if type(raw) is not bytes or len(raw) > 65536:
        raise ValueError('bounded reply required')
    context_check(context)
    value = C.loads(raw)
    keys = ('version', 'gate_rule_digest', 'descriptor_sha256', 'input_digest', 'outcome', 'reason', 'action',
            'failure_class', 'detail', 'gate_error', 'row', 'native_followup', 'statement_id', 'fuel',
            'kernels', 'ext', 'assurance', 'counter_identity')
    C.fields(value, keys)
    if C.canonical(value) != raw or value['version'] != 1 or type(value['version']) is not int:
        raise ValueError('noncanonical reply')
    if value['gate_rule_digest'] != C.identity(descriptor) or value['descriptor_sha256'] != C.sha(C.encode(descriptor)):
        raise ValueError('reply release mismatch')
    if value['input_digest'] != C.sha(C.canonical(context)):
        raise ValueError('reply input mismatch')
    if value['detail'] not in ('', 'malformed-record'):
        raise ValueError('unknown detail code')
    if type(value['row']) is not str or value['row'] not in M.ROWS:
        raise ValueError('unknown reply row')
    expected = M.classify(M.Observation(value['row'], detail=value['detail'],
                                      fork_authorized=fork_authorized), descriptor)
    for key in ('row', 'outcome', 'reason', 'action', 'failure_class', 'detail', 'gate_error', 'native_followup'):
        if value[key] != expected[key] or type(value[key]) is not type(expected[key]):
            raise ValueError('reply mapping mismatch')
    # VOID-only identity and scalar syntax apply even to unavailable results.
    M.check_statement(value['outcome'], value['reason'], value['statement_id'], context,
                      fork_authorized=fork_authorized, authenticated_statement=authenticated_statement)
    if value['row']=='output_terminal' and (value['kernels']!=M.output_limit() or value['fuel']!=0):
        raise ValueError('output limit requires unobserved work and zero intake charge')
    counters = value['counter_identity']
    if counters is not None:
        C.fields(counters, ('lean', 'nanoda'))
        for kernel, entry in counters.items():
            C.fields(entry, ('summary', 'vector'))
            C.fields(entry['vector'], ('sha256', 'records'))
            C.digest(entry['vector']['sha256']); C.uint(entry['vector']['records'])
            keys = (('calls', 'checked', 'code', 'debit', 'depth', 'version') if kernel == 'lean' else
                    ('version', 'code', 'checked_declarations', 'aggregate_work', 'aggregate_steps',
                     'maximum_depth', 'maximum_composed_depth', 'selected_declarations'))
            C.fields(entry['summary'], keys)
            for count in entry['summary'].values():
                C.uint(count)
            if entry['summary']['version'] != 1 or entry['summary']['code'] not in (0, 3):
                raise ValueError('unknown counter identity version/outcome')
    C.fields(value['kernels'], ('lean', 'nanoda'))
    total = 0; incomplete = False
    for kernel, state in value['kernels'].items():
        C.fields(state, ('state', 'debit', 'charged'))
        if state['state'] == 'unknown':
            if value['row']=='output_terminal' and state==dict(state='unknown',debit=None,charged=0):
                C.uint(state['charged'])
                continue
            if state['debit'] is not None or state['charged'] is not None:
                raise ValueError('fabricated unknown counter')
            incomplete = True
            continue
        C.uint(state['charged'])
        if state['state'] == 'not-run':
            if state['debit'] is not None or state['charged'] != 0:
                raise ValueError('fabricated not-run counter')
        elif state['state'] == 'executed':
            C.uint(state['debit'])
            if state['charged'] > state['debit'] or value['row'] != 'resource_cap' and state['charged'] != state['debit']:
                raise ValueError('counter charge mismatch')
        else:
            raise ValueError('unknown kernel state')
        total += state['charged'] * descriptor['fuel'][kernel+'_weight']
    if incomplete:
        if value['fuel'] is not None or value['outcome'] != 'DEFER':
            raise ValueError('missing executed counter')
    else:
        C.uint(value['fuel'])
        if total != value['fuel'] or total > context['slot_fuel']:
            raise ValueError('fuel mismatch')
    if value['outcome'] in ('ACCEPT', 'REJECT', 'VOID'):
        if value['outcome'] == 'ACCEPT' and context['lane'] != 1:
            raise ValueError('fuel or lane mismatch')
        if (value['outcome'] == 'ACCEPT' or value['row'] in ('kernel_reject', 'kernel_disagreement')) and any(
                state['state'] != 'executed' for state in value['kernels'].values()):
            raise ValueError('both executed kernels required')
        if value['outcome'] == 'VOID' and any(state['state'] != 'not-run' for state in value['kernels'].values()):
            raise ValueError('VOID must precede kernel execution')
    if value['ext'] != [] or value['assurance'] != 'development-only':
        raise ValueError('A1 reply assurance/ext')
    return value
