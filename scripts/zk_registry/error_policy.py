"""Pure, versioned error classification and resource-only retry rules."""
import hashlib
import json
from pathlib import Path

POLICY_PATH=Path(__file__).parent/'data/error_policy_v1.json'
POLICY_BYTES=POLICY_PATH.read_bytes()
RULES=json.loads(POLICY_BYTES)
RULES_SHA256=hashlib.sha256(POLICY_BYTES).hexdigest()
terminal_reasons=frozenset(RULES['terminal_reasons'])


def classify(reason,stage='',detail=''):
    if reason=='output-cap' and str(stage).startswith(('reference','challenge','import','pristine')):return 'D-node'
    if reason=='replay-unavailable' and detail=='malformed-record':return 'I'
    if reason=='checker-worker-error' and detail=='verdict-oversized':return 'D-node'
    if any(str(reason).startswith(prefix) for prefix in RULES['node_prefixes']):return 'D-node'
    return RULES['groups'].get(reason,'T')


def envelope(attempt,a1):
    """Select one escalated attempt; all deterministic gate settings are fixed."""
    resource=a1.get('reason')=='resource-unavailable' or bool(a1.get('resource_events'))
    escalated=attempt==(2 if resource else 3)
    from .bounded_capture import caps
    return dict(RULES['escalated' if escalated else 'standard'],output_caps=caps(),
                cpus=4,pids=256,lean_jobs=4,max_exported_constants=1000000,worker_stack_bytes=67108864)


def _healthy(attempt):
    canary=attempt.get('canary')
    return canary is True or (isinstance(canary,dict) and canary.get('before')=='PASS' and canary.get('after')=='PASS')


def quarantine_trigger(attempts):
    """Return a reproducible trigger only for independently healthy observations."""
    groups={}
    components={}
    for a in attempts:
        reason,stage,detail=a.get('reason'),a.get('stage',''),a.get('detail','')
        group=a.get('class')
        if (group not in ('T','I','D-input')
                or reason in terminal_reasons or not _healthy(a) or not a.get('host')
                or a.get('profile')!='production'):
            continue
        key=(stage,reason)
        groups.setdefault(key,[]).append(a)
        if (stage in RULES['triggers'].get('component_stages', []) and
                reason in RULES['triggers'].get('component_reasons', []) and not a.get('resource_events')):
            components.setdefault(stage,[]).append(a)
    for (stage,reason),rows in groups.items():
        if len({a['host'] for a in rows})<RULES['triggers']['independent_hosts']:continue
        if reason=='resource-unavailable' and not any(a.get('envelope')=='escalated' or
                isinstance(a.get('envelope'),dict) and a['envelope'].get('wall_s')==3600 for a in rows):continue
        return dict(stage=stage,reason=reason,hosts=sorted({a['host'] for a in rows}))
    for stage,rows in components.items():
        if len({a['host'] for a in rows})>=RULES['triggers']['independent_hosts']:
            return dict(stage=stage,reasons=sorted({a['reason'] for a in rows}),hosts=sorted({a['host'] for a in rows}),kind='component-failure')
    return None
