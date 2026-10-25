"""Exit protocol of gate-owned drivers; resource/boundary failures take precedence."""
from enum import IntEnum


class Exit(IntEnum):
    ACCEPT = 0
    REJECT = 2
    CAP = 3
    FAULT = 4
    UNAVAILABLE = 70
    LEGACY_KERNEL = 71


def outcome(code, stage, input_role):
    """Interpret only the trusted driver's code, never its diagnostic streams."""
    if input_role not in ('submission', 'reference', 'challenge'):
        raise ValueError('unknown trusted input role')
    trusted_stage = stage if input_role == 'submission' else input_role + ':' + stage
    if code == Exit.ACCEPT:
        return dict(stage=trusted_stage, verdict='PASS', reason=None, tool_exit=int(code))
    if code == Exit.UNAVAILABLE:
        return dict(stage=trusted_stage, verdict='ERROR', reason='environment-tool-unavailable:' + stage, tool_exit=int(code))
    reason = {Exit.REJECT: 'tool-reject:' + stage, Exit.CAP: 'resource-cap',
              Exit.FAULT: 'tool-fault:' + stage}.get(code)
    if reason is not None:
        if input_role != 'submission':
            reason = ('challenge-' if input_role == 'challenge' else 'statement-') + reason
        return dict(stage=trusted_stage, verdict='INVALID' if input_role == 'submission' else 'ERROR', reason=reason, tool_exit=int(code))
    return dict(stage=trusted_stage, verdict='ERROR', reason='checker-input-unavailable')
