"""Bounded data grammar and authenticated declaration ownership, before kernels."""
from dataclasses import dataclass
import os
import re
import stat
from .. import statement_identity as I
from . import codec as C

MAGIC = b'BOOLE-M3/EXPORT-BUNDLE/v1\0'
RAW_MAX = 134217728
ALLOWED_AXIOMS = frozenset({(('str', 'Classical'), ('str', 'choice')),
                           (('str', 'Quot'), ('str', 'sound')), (('str', 'propext'),)})
PACKAGE_KEYS = ('version', 'mode', 'problem', 'env_id', 'target', 'statement',
                'reference_sha256', 'inventory_sha256', 'fuel_cap')


class Refusal(ValueError):
    def __init__(self, row, detail=''):
        super().__init__(row)
        self.row, self.detail = row, detail


def name_key(name):
    if not isinstance(name, (list, tuple)) or not name or len(name) > 256:
        raise ValueError('structured name required')
    for part in name:
        if not isinstance(part, (list, tuple)) or len(part) != 2:
            raise ValueError('name component')
        kind, value = part
        if kind == 'str' and type(value) is str and value:
            continue
        if kind == 'num':
            C.uint(value)
            continue
        raise ValueError('name component')
    return C.canonical(name).decode()


def as_name(key):
    value = C.loads(key)
    if name_key(value) != key:
        raise ValueError('noncanonical structured name')
    return tuple(tuple(part) for part in value)


def fqn(name):
    # The native target CLI splits Strings on dots. Quoting would change the
    # selected name; a valid structured target outside this subset must defer.
    if any(kind != 'str' or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_']*", value) for kind, value in name):
        raise Refusal('node_fault', 'nonportable target selector')
    return '.'.join(value for _, value in name)


def selector(name):
    """Injective portable subset of the native Name.toString membership key.

    Numeric components are supported by that existing opaque String selector.
    String components that could collide/require Lean-specific escaping defer;
    they are never flattened into a different structured name.
    """
    parts = []
    for kind, value in name:
        if kind == 'num':
            C.uint(value); parts.append(str(value))
        elif kind == 'str' and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_']*", value):
            parts.append(value)
        else:
            raise Refusal('node_fault', 'nonportable own selector')
    return '.'.join(parts)


def pack(export):
    if type(export) is not bytes or not export or len(export) + len(MAGIC) + 8 > RAW_MAX:
        raise Refusal('structural_cap', 'bundle size')
    return MAGIC + len(export).to_bytes(8, 'big') + export


def unpack(raw):
    if type(raw) is bytes and len(raw) > RAW_MAX:
        raise Refusal('structural_cap', 'bundle size')
    if type(raw) is not bytes or not raw.startswith(MAGIC):
        raise Refusal('front_invalid', 'bundle grammar')
    size = int.from_bytes(raw[len(MAGIC):len(MAGIC)+8], 'big')
    export = raw[len(MAGIC)+8:]
    if not size or size != len(export):
        raise Refusal('front_invalid', 'bundle length')
    return export


@dataclass(frozen=True)
class Inventory:
    raw: bytes
    sha256: str

    def __post_init__(self):
        if C.sha(self.raw) != C.digest(self.sha256):
            raise Refusal('node_fault', 'inventory binding')
        try:
            obj = C.loads(self.raw)
            C.fields(obj, ('version', 'environment', 'base', 'library'))
            if type(obj['version']) is not int or obj['version'] != 1:
                raise ValueError('inventory version')
            C.digest(obj['environment'])
            for group in ('base', 'library'):
                entries = obj[group]
                if type(entries) is not dict or list(entries) != sorted(entries):
                    raise ValueError('sorted inventory required')
                for name, digest in entries.items():
                    as_name(name); C.digest(digest)
            if set(obj['base']) & set(obj['library']) or C.canonical(obj) != self.raw:
                raise ValueError('ambiguous inventory')
        except (ValueError, TypeError, KeyError, RecursionError) as ex:
            raise Refusal('node_fault', 'invalid authenticated inventory') from ex

    def data(self):
        return C.loads(self.raw)  # callers never mutate a retained authenticated object


def package(raw, expected_sha256, inventory):
    if type(raw) is not bytes or C.sha(raw) != expected_sha256:
        raise Refusal('input_integrity', 'package address')
    try:
        if len(raw) > 1048576:
            raise ValueError('package size')
        obj = C.loads(raw); C.fields(obj, PACKAGE_KEYS)
        if type(obj['version']) is not int or obj['version'] != 1 or obj['mode'] not in ('full', 'certified-own'):
            raise ValueError('package version/mode')
        if type(obj['problem']) is not str or not 0 < len(obj['problem'].encode()) <= 1024:
            raise ValueError('problem id')
        for key in ('env_id', 'statement', 'reference_sha256', 'inventory_sha256'):
            C.digest(obj[key])
        name_key(obj['target'])
        if obj['inventory_sha256'] != inventory.sha256 or obj['env_id'] != inventory.data()['environment']:
            raise ValueError('environment binding')
        if C.canonical(obj) != raw:
            raise ValueError('noncanonical package')
        return obj
    except (ValueError, TypeError, KeyError, RecursionError) as ex:
        raise Refusal('package_invalid', 'authenticated package grammar') from ex


class DataExport(I.Export):
    """Reuse the complete intern/DAG validator; defer safety policy to ownership.

    Unsafe/partial library material must not masquerade as an own violation.
    Validation still checks original field types and restores every payload.
    """
    def validate_transport(self):
        saved = []
        try:
            for kind, value in self.declarations.values():
                for key in ('isUnsafe', 'safety'):
                    if key in value:
                        old = value[key]
                        if key == 'isUnsafe' and type(old) is not bool or key == 'safety' and old not in ('safe', 'unsafe', 'partial'):
                            raise I.IdentityError('invalid safety field')
                        saved.append((value, key, old))
                        value[key] = False if key == 'isUnsafe' else 'safe'
            super().validate_transport()
        finally:
            for value, key, old in saved:
                value[key] = old


def read_export(path, role='submission'):
    try:
        # Strict scalar parsing also covers metadata and unreachable records.
        with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW), 'rb') as stream:
            metadata = os.fstat(stream.fileno())
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > RAW_MAX:
                raise I.IdentityCap('export-byte-limit')
            for index, line in enumerate(iter(lambda: stream.readline(16*1024**2+1), b''), 1):
                if len(line) > 16*1024**2 or index > I.MAX_RECORDS:
                    raise I.IdentityCap('export-record-limit')
                if not line.endswith(b'\n'):
                    raise I.IdentityError('unterminated record')
                C.loads(line)
        return DataExport(path)
    except (I.IdentityUnavailable, OSError, MemoryError):
        raise Refusal('node_fault', 'export unavailable') from None
    except I.IdentityCap:
        raise Refusal('structural_cap' if role == 'submission' else 'node_fault', 'export limit') from None
    except (I.IdentityError, ValueError, TypeError, KeyError, RecursionError):
        raise Refusal('front_invalid' if role == 'submission' else 'node_fault', 'export grammar') from None


def payload(export, name):
    kind, value = export.declarations[name]
    flags = {key: value[key] for key in ('isUnsafe', 'safety') if key in value}
    try:
        for key in flags:
            value[key] = False if key == 'isUnsafe' else 'safe'
        result, _ = export.declaration(name)
        # Statement identity omits theorem proofs. A cached payload identity MUST
        # include them, as well as reducibility and safety, to prevent replacement.
        if kind == 'thm':
            result['proof'] = export.expression(value['value'], export.context(value['levelParams']))
        if 'hints' in value:
            result['hints'] = value['hints']
        result['flags'] = flags
        return I.hashed(result)
    finally:
        value.update(flags)


@dataclass(frozen=True)
class Checked:
    own: tuple
    base: tuple
    library: tuple


def reserved(name):
    if name[0] in (('str', 'Leaf'), ('str', 'BooleCS')):
        return True
    return (len(name) > 1 and name[0] == ('str', 'Boole') and name[1][0] == 'str'
            and re.fullmatch(r'R[0-9]*', name[1][1]) is not None)


def own_input_caps(export, starts):
    # Traverse only the supplied own expressions, not imported library bodies.
    # Decimal digit length is bounded BEFORE converting a natural literal.
    stack = [(root, 1) for root in starts]; depths = {}
    while stack:
        index, depth = stack.pop()
        if depth > 65536:
            raise Refusal('structural_cap', 'own DAG depth')
        if depths.get(index, 0) >= depth:
            continue
        depths[index] = depth; tag, value = export.exprs[index]
        if tag == 'natVal' and (len(value) > 10000 or (int(value).bit_length()+7)//8 > 4096):
            raise Refusal('structural_cap', 'own natural literal')
        if tag == 'strVal' and len(value.encode()) > 4096:
            raise Refusal('structural_cap', 'own string literal')
        children = ([value['type'], value['body']] if tag in ('lam', 'forallE') else
                    [value[k] for k in ('type', 'value', 'body')] if tag == 'letE' else
                    [value['fn'], value['arg']] if tag == 'app' else
                    [value['struct']] if tag == 'proj' else [])
        stack.extend((child, depth+1) for child in children)


def partition(export, inventory, pkg):
    table = inventory.data(); own, base, library = [], [], []
    # Inventory digest is verified before any miner data is inspected.
    for name, (kind, value) in export.declarations.items():
        key = name_key(name)
        if key in table['base'] or key in table['library']:
            group = 'base' if key in table['base'] else 'library'
            if payload(export, name) != table[group][key]:
                raise Refusal('front_invalid', 'changed authenticated declaration')
            (base if group == 'base' else library).append(name)
        else:
            own.append(name)
    for name in own:
        kind, value = export.declarations[name]
        starts = [value['type']] + ([value['value']] if 'value' in value else [])
        own_input_caps(export, starts)
        if reserved(name) or any(reserved(dep) for dep in export.dependencies(starts, set())):
            raise Refusal('reserved_name', 'reserved-name')
        if value.get('isUnsafe') or value.get('safety', 'safe') != 'safe':
            raise Refusal('front_invalid', 'own unsafe/partial')
        if pkg['mode'] == 'certified-own' and kind not in ('def', 'thm'):
            raise Refusal('front_invalid', 'own declaration profile')
        if kind == 'axiom' and name not in ALLOWED_AXIOMS:
            raise Refusal('axiom_rejected', 'own axiom')
    target = tuple(tuple(part) for part in pkg['target'])
    if target not in own or export.declarations[target][0] != 'thm':
        raise Refusal('front_invalid', 'target not own theorem')
    # Unsupported allowed library content is a later S outcome; the adapter still
    # runs independently decidable statement/axiom comparisons before applying it.
    return Checked(tuple(sorted(own)), tuple(sorted(base)), tuple(sorted(library)))
