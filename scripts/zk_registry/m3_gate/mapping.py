"""One closed M3 mapping table, bound to the release's v2 policy on every path."""
from dataclasses import dataclass
from . import codec as C
from .. import error_policy as E


@dataclass(frozen=True)
class Row:
    outcome: str
    reason: int
    action: str
    error: str = ''
    native_followup: str = ''


# These are descriptor labels in already-reserved M3 categories; only 0306 and
# 0801..0804 have fold semantics. No fold/wire implementation is imported.
ROWS = {
    'accept': Row('ACCEPT', 0, 'apply'),
    'front_invalid': Row('REJECT', 0x0106, 'apply'),
    'name_grammar': Row('REJECT', 0x0103, 'apply'),
    'unknown_constant': Row('REJECT', 0x0106, 'apply'),
    'structural_cap': Row('REJECT', 0x0108, 'apply'),
    'reserved_name': Row('REJECT', 0x0201, 'apply'),
    'statement_mismatch': Row('REJECT', 0x0107, 'apply'),
    'axiom_rejected': Row('REJECT', 0x0305, 'apply'),
    'kernel_reject': Row('REJECT', 0x0301, 'apply'),
    'input_type': Row('REJECT', 0x0301, 'apply'),
    'kernel_disagreement': Row('REJECT', 0x0302, 'apply'),
    'resource_cap': Row('REJECT', 0x0303, 'apply'),
    'depth_cause_missing': Row('DEFER', 0, 'repair', 'environment-native-cause-unavailable', 'distinct-depth-cause'),
    'cap_cause_missing': Row('DEFER', 0, 'repair', 'environment-native-cause-unavailable', 'typed-cap-origin'),
    'kernel_panic_cause_missing': Row('DEFER', 0, 'repair', 'environment-native-cause-unavailable', 'explicit-kernel-panic-cause'),
    'output_terminal': Row('REJECT', 0x0109, 'terminal', 'output-cap'),
    'library_terminal': Row('REJECT', 0x010a, 'terminal', 'library-outside-certified-base'),
    'transient': Row('DEFER', 0, 'retry', 'resource-unavailable'),
    'input_failure': Row('DEFER', 0, 'retry', 'replay-unavailable'),
    'node_fault': Row('DEFER', 0, 'repair', 'environment-tool-unavailable'),
    'input_availability': Row('STALL', 0, 'wait'),
    'input_integrity': Row('STOP', 0, 'preserve', 'replay-unavailable'),
    'package_invalid': Row('VOID', 0x0801, 'apply'),
    'fuel_binding_invalid': Row('VOID', 0x0803, 'apply'),
    'generator_unavailable': Row('VOID', 0x0804, 'apply'),
    'malformed_result': Row('DEFER', 0, 'repair', 'environment-result-unavailable'),
    'trusted_disagreement': Row('DEFER', 0, 'retry', 'replay-unavailable'),
    'quarantine_void': Row('VOID', 0x0802, 'fork-only'),
}
NATIVE_FOLLOWUPS = tuple(sorted({r.native_followup for r in ROWS.values() if r.native_followup}
                               | {'typed-pre-kernel-cap-origin', 'nonportable-structured-own-selection'}))


@dataclass(frozen=True)
class Observation:
    row: str
    stage: str = 'comparator'
    role: str = 'submission'
    detail: str = ''
    physical: bool = False
    fork_authorized: bool = False


def classify(observation, descriptor, phase='first'):
    C.validate(descriptor)
    if phase not in ('first', 'retry', 'replay'):
        raise ValueError('unknown classification phase')
    rules = E.load(descriptor['policy'])
    o = observation
    if (type(o) is not Observation or type(o.physical) is not bool or type(o.fork_authorized) is not bool
            or o.row not in ROWS or o.role not in ('submission', 'reference', 'gate-generated', 'operator')):
        o = Observation('malformed_result')
    if o.physical:
        o = Observation('transient', o.stage, o.role)
    if o.row == 'quarantine_void' and not o.fork_authorized:
        o = Observation('malformed_result')
    row = ROWS[o.row]
    # Submission-specific typed faults/limits are never proof verdicts when a
    # trusted reference or a generated export was the failed input.
    if o.role != 'submission' and o.row in ('front_invalid', 'name_grammar', 'unknown_constant', 'reserved_name', 'statement_mismatch',
            'axiom_rejected', 'input_type', 'kernel_reject', 'kernel_disagreement', 'resource_cap', 'structural_cap',
            'library_terminal', 'output_terminal'):
        o = Observation('node_fault', o.stage, o.role)
        row = ROWS[o.row]
    detail = 'malformed-record' if o.row == 'trusted_disagreement' else o.detail
    group = E.classify(row.error, o.stage, detail, rules=rules) if row.error else ''
    if row.outcome == 'DEFER' and group == 'D-node':
        action = 'repair'
    else:
        action = row.action
    return dict(row=o.row, outcome=row.outcome, reason=row.reason, action=action,
                failure_class=group, detail='malformed-record' if detail == 'malformed-record' else '',
                gate_error=row.error, native_followup=row.native_followup)


VOID_DOMAIN = b'BOOLE-M3/VOID-STATEMENT/v1\0'


def void_statement(context):
    C.uint(context['chain_id'], True)
    parts = [VOID_DOMAIN, context['chain_id'].to_bytes(8, 'big')]
    for key in ('problem_id', 'env_id', 'package_sha256'):
        parts.append(bytes.fromhex(C.digest(context[key])))
    return C.sha(b''.join(parts) + (0x0801).to_bytes(2, 'big'))


def check_statement(outcome, reason, statement, context):
    fingerprint = void_statement(context)
    if outcome == 'VOID' and reason == 0x0801:
        if statement != fingerprint or statement == '0'*64:
            raise ValueError('0801 requires its VOID-only fingerprint')
    elif statement == fingerprint:
        raise ValueError('VOID fingerprint used outside 0801')
    elif outcome in ('ACCEPT', 'VOID'):
        C.digest(statement)
    elif statement != '0'*64:
        C.digest(statement)


def kernel_states(events, code, verdict, caps, weights):
    """Exact raw debits plus bounded charges; absence never means zero work.

    A well-formed early nanoda cap/fault proves Lean has not run because DET3
    executes external nanoda first. All other missing executed records fail closed.
    """
    stages = {e['stage']: e for e in events}
    if len(stages) != len(events) or stages.get('final') != dict(version=1, stage='final', rc=code):
        raise ValueError('invalid final counter record')
    early = (code in (3, 4) and 'nanoda-fuel' in stages and 'fuel' not in stages
             and stages.get('policy', {}).get('reason') in ('resource-cap', 'tool-fault:replay'))
    result = {}; total = 0
    for kernel, tag in (('lean', 'fuel'), ('nanoda', 'nanoda-fuel')):
        event = stages.get(tag)
        if event is None:
            if kernel == 'lean' and early:
                result[kernel] = dict(state='not-run', debit=None, charged=0)
                continue
            raise ValueError('missing executed kernel counter')
        debit = C.uint(event.get('debit')); cap = C.uint(caps[kernel], True)
        if event.get('kernel') != kernel:
            raise ValueError('counter kernel mismatch')
        if debit > cap and not (code == 3 and verdict == 'INVALID'):
            raise ValueError('over-budget non-cap result')
        charged = min(debit, cap) if code == 3 else debit
        total += charged * C.uint(weights[kernel+'_weight'], True)
        C.uint(total)
        result[kernel] = dict(state='executed', debit=debit, charged=charged)
    return result, total


def not_run():
    return {k: dict(state='not-run', debit=None, charged=0) for k in ('lean', 'nanoda')}


def unknown():
    return {k: dict(state='unknown', debit=None, charged=None) for k in ('lean', 'nanoda')}


def output_limit():
    """Terminal S has zero intake charge; actual unreported work stays unknown."""
    return {k:dict(state='unknown',debit=None,charged=0) for k in ('lean','nanoda')}
