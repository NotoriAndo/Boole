"""Canonical release bytes. Content addresses and domain identities are distinct."""
import hashlib
import json
import re
from .. import error_policy as E

DOMAIN = b'BOOLE-M3/REAL-GATE-DESCRIPTOR/v1\0'
FIELDS = ('version', 'adapter', 'source_sha256', 'kernel_set_digest', 'policy',
          'caps_sha256', 'fuel_authority_sha256', 'acceptance_environment_sha256', 'acceptance_tools_sha256',
          'artifact_table_sha256', 'grammar', 'fuel', 'profiles')
TOOLS = ('comparator', 'nanoda', 'lean_replay', 'counter_replay', 'exporter',
         'lean_runtime', 'imports', 'sandbox', 'codec')
PROFILES = {'Darwin-arm64': 'development', 'Linux-aarch64': 'production'}


def kernel_set_identity(value):
    material = dict(acceptance_environment_sha256=value['acceptance_environment_sha256'],
                    acceptance_tools_sha256=value['acceptance_tools_sha256'], profiles=value['profiles'])
    return sha(b'BOOLE-M3/KERNEL-SET/v1\0' + canonical(material))


def fields(value, keys):
    if type(value) is not dict or tuple(value) != tuple(keys):
        raise ValueError('noncanonical fields')


def digest(value):
    if type(value) is not str or not re.fullmatch('[0-9a-f]{64}', value) or value == '0'*64:
        raise ValueError('nonzero SHA-256 required')
    return value


def uint(value, positive=False):
    if type(value) is not int or not (int(positive) <= value < 2**64):
        raise ValueError('u64 required')
    return value


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def loads(raw):
    return json.loads(raw, object_pairs_hook=unique,
                      parse_int=json_integer,
                      parse_float=lambda _: (_ for _ in ()).throw(ValueError('float forbidden')),
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite forbidden')))


def json_integer(token):
    # JSON numeric fields are u64, not unbounded Lean natural literal Strings.
    # Bound digits before conversion, independently of CPython's configurable
    # integer string limit (the minimum nonzero setting is 640 digits).
    if len(token) > 20:
        raise ValueError('JSON integer exceeds u64')
    return uint(int(token))


def validate(value):
    fields(value, FIELDS)
    if type(value['version']) is not int or value['version'] != 1 or value['adapter'] != 'boole-m3-real/v1':
        raise ValueError('unsupported descriptor')
    for key in ('source_sha256', 'kernel_set_digest', 'caps_sha256', 'fuel_authority_sha256',
                'acceptance_environment_sha256', 'acceptance_tools_sha256', 'artifact_table_sha256'):
        digest(value[key])
    fields(value['policy'], ('version', 'rules_sha256'))
    if value['policy']['version'] != 2:
        raise ValueError('real gate requires policy v2')
    E.load(value['policy'])
    if type(value['grammar']) is not int or value['grammar'] != 1:
        raise ValueError('unsupported grammar')
    fields(value['fuel'], ('lean_weight', 'nanoda_weight', 'cap_charge'))
    for key in ('lean_weight', 'nanoda_weight'):
        uint(value['fuel'][key], True)
    if value['fuel']['cap_charge'] != 'bounded-debit/v1':
        raise ValueError('unsupported cap charge')
    profiles = value['profiles']
    if type(profiles) is not dict or list(profiles) != sorted(profiles) or set(profiles) != set(PROFILES):
        raise ValueError('sorted qualified profile map required')
    for profile, entry in profiles.items():
        fields(entry, ('assurance', 'tools'))
        if entry['assurance'] != PROFILES[profile]:
            raise ValueError('invalid profile assurance')
        fields(entry['tools'], TOOLS)
        for pin in entry['tools'].values():
            digest(pin)
    if value['kernel_set_digest'] != kernel_set_identity(value):
        raise ValueError('kernel-set digest mismatch')
    return value


def canonical(value):
    return json.dumps(value, ensure_ascii=True, separators=(',', ':'), allow_nan=False).encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    return canonical(validate(value))


def decode(raw, expected=None):
    if type(raw) is not bytes or len(raw) > 65536:
        raise ValueError('bounded descriptor bytes required')
    try:
        value = loads(raw)
        if encode(value) != raw:
            raise ValueError('noncanonical encoding')
        if expected is not None and identity(value) != digest(expected):
            raise ValueError('descriptor identity mismatch')
        return value
    except (TypeError, KeyError, RecursionError, UnicodeError) as exc:
        raise ValueError('invalid descriptor') from exc


def identity(value):
    return sha(DOMAIN + encode(value))


def for_host(raw, expected, profile):
    """Select local pins without changing the descriptor used in consensus."""
    value = decode(raw, expected)
    if profile not in value['profiles']:
        raise ValueError('profile not admitted')
    return dict(gate_rule_digest=identity(value), descriptor_sha256=sha(raw),
                profile=profile, release=value['profiles'][profile])
