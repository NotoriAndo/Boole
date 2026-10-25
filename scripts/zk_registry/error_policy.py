"""Pure, versioned error classification and resource-only retry rules."""
import hashlib
import json
from pathlib import Path

LEGACY_POLICY_PATH=Path(__file__).parent/'data/error_policy_v1.json'
LEGACY_POLICY_SHA256=hashlib.sha256(LEGACY_POLICY_PATH.read_bytes()).hexdigest()
POLICY_PATH=Path(__file__).parent/'data/error_policy_v2.json'
POLICY_BYTES=POLICY_PATH.read_bytes()
RULES=json.loads(POLICY_BYTES)
RULES_SHA256=hashlib.sha256(POLICY_BYTES).hexdigest()


def load(descriptor):
    """New registries bind v2; existing v1 registries keep their exact rules."""
    version=descriptor.get('version')
    if set(descriptor)!={'version','rules_sha256'} or type(version) is not int or version not in (1,2):
        raise ValueError('unsupported error policy')
    path,expected=(LEGACY_POLICY_PATH,LEGACY_POLICY_SHA256) if version==1 else (POLICY_PATH,RULES_SHA256)
    raw=path.read_bytes()
    if hashlib.sha256(raw).hexdigest()!=descriptor['rules_sha256'] or descriptor['rules_sha256']!=expected:
        raise ValueError('error policy digest mismatch')
    rules=json.loads(raw)
    if rules['version']!=version:raise ValueError('error policy version mismatch')
    return rules


def classify(reason,stage='',detail='',*,rules=None):
    rules = RULES if rules is None else rules
    if reason=='output-cap' and str(stage).startswith(('reference','challenge','import','pristine')):return 'D-node'
    if reason=='replay-unavailable' and detail=='malformed-record':return 'I'
    if reason=='checker-worker-error' and detail=='verdict-oversized':return 'D-node'
    if any(str(reason).startswith(prefix) for prefix in rules['node_prefixes']):return 'D-node'
    return rules['groups'].get(reason,'T')


def envelope(attempt,a1,*,rules=None):
    """Select one escalated attempt; all deterministic gate settings are fixed."""
    rules = RULES if rules is None else rules
    resource=a1.get('reason')=='resource-unavailable' or bool(a1.get('resource_events'))
    escalated=attempt==(2 if resource else 3)
    from .bounded_capture import caps
    from .lean_runner import testnet_caps
    return dict(rules['escalated' if escalated else 'standard'],output_caps=caps(),
                cpus=4,pids=256,lean_jobs=4,max_exported_constants=1000000,worker_stack_bytes=testnet_caps()['lean']['worker_stack_bytes'])


def _healthy(attempt):
    canary=attempt.get('canary')
    return canary is True or (isinstance(canary,dict) and canary.get('before')=='PASS' and canary.get('after')=='PASS')


def quarantine_trigger(attempts,*,rules=None):
    """Return a reproducible trigger only for independently healthy observations."""
    rules = RULES if rules is None else rules
    groups={}
    components={}
    for a in attempts:
        reason,stage,detail=a.get('reason'),a.get('stage',''),a.get('detail','')
        group=a.get('class')
        if (group not in ('T','I','D-input')
                or reason in rules['terminal_reasons'] or not _healthy(a) or not a.get('host')
                or a.get('profile')!='production'):
            continue
        key=(stage,reason)
        groups.setdefault(key,[]).append(a)
        if (stage in rules['triggers'].get('component_stages', []) and
                reason in rules['triggers'].get('component_reasons', []) and not a.get('resource_events')):
            components.setdefault(stage,[]).append(a)
    for (stage,reason),rows in groups.items():
        if len({a['host'] for a in rows})<rules['triggers']['independent_hosts']:continue
        if reason=='resource-unavailable' and not any(a.get('envelope')=='escalated' or
                isinstance(a.get('envelope'),dict) and a['envelope'].get('wall_s')==3600 for a in rows):continue
        return dict(stage=stage,reason=reason,hosts=sorted({a['host'] for a in rows}))
    for stage,rows in components.items():
        if len({a['host'] for a in rows})>=rules['triggers']['independent_hosts']:
            return dict(stage=stage,reasons=sorted({a['reason'] for a in rows}),hosts=sorted({a['host'] for a in rows}),kind='component-failure')
    return None
