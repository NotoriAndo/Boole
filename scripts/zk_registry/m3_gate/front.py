"""Bounded data grammar and authenticated declaration ownership, before kernels."""
from dataclasses import dataclass
from contextlib import contextmanager
import os
import hashlib
import re
import stat
from .. import statement_identity as I
from . import codec as C

MAGIC = b'BOOLE-M3/EXPORT-BUNDLE/v1\0'
RAW_MAX = 134217728
LITERAL_BYTES = 4096  # frozen literal_bytes in testnet_caps_v2.json, both kernels
NAT_DECIMAL_MAX_DIGITS = (8 * LITERAL_BYTES * 30103) // 100000 + 1
ALLOWED_AXIOMS = frozenset({(('str', 'Classical'), ('str', 'choice')),
                           (('str', 'Quot'), ('str', 'sound')), (('str', 'propext'),)})
PACKAGE_KEYS = ('version', 'mode', 'problem', 'env_id', 'target', 'statement',
                'reference_sha256', 'inventory_sha256', 'fuel_cap')


class Refusal(ValueError):
    def __init__(self, row, detail=''):
        super().__init__(row)
        self.row, self.detail = row, detail


class SubmissionTypeError(Exception):
    """An explicit type/dependency failure discovered from submitted data."""


# Exact types only: broad ValueError/KeyError/TypeError handlers could turn a
# trusted implementation bug into a miner rejection. Physical deadlines are
# deliberately absent; IdentityUnavailable remains a node/availability fault.
SUBMISSION_ERRORS = {I.IdentityError: 'front_invalid', I.IdentityCap: 'structural_cap',
                     SubmissionTypeError: 'input_type', UnicodeEncodeError: 'front_invalid',
                     UnicodeDecodeError: 'front_invalid'}


@contextmanager
def submission_boundary():
    try:
        yield
    except Refusal:
        raise
    except Exception as error:
        raise Refusal(SUBMISSION_ERRORS.get(type(error), 'node_fault'), 'submission computation') from None


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
    # selected name; targets outside this release's grammar are rejected.
    if any(kind != 'str' or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_']*", value) for kind, value in name):
        raise Refusal('name_grammar', 'nonportable target selector')
    return '.'.join(value for _, value in name)


def selector(name):
    """Injective portable subset of the native Name.toString membership key.

    Numeric components are supported by that existing opaque String selector.
    String components that could collide/require Lean-specific escaping reject;
    they are never flattened into a different structured name.
    """
    parts = []
    for kind, value in name:
        if kind == 'num':
            C.uint(value); parts.append(str(value))
        elif kind == 'str' and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_']*", value):
            parts.append(value)
        else:
            raise Refusal('name_grammar', 'nonportable own selector')
    return '.'.join(parts)


def pack(export):
    if type(export) is not bytes or not export or len(export) + len(MAGIC) + 8 > RAW_MAX:
        raise Refusal('structural_cap', 'bundle size')
    return MAGIC + len(export).to_bytes(8, 'big') + export


@dataclass(frozen=True)
class BundleSource:
    """Node-local sealed artifact path; never a field in miner metadata."""
    path: str


@dataclass(frozen=True)
class BoundBundle:
    size: int
    digest: str
    data: object


def bind_bundle(source, expected, *, retention_limit=None):
    """Hash through EOF before admission; retain at most the framing allowance.

    The retention bound controls memory only. An oversized input is not rejected
    until its WHOLE content address has been checked, including the discarded tail.
    """
    limit = RAW_MAX if retention_limit is None else retention_limit
    hashed=hashlib.sha256();size=0;kept=[]
    def consume(chunk):
        nonlocal size,kept
        hashed.update(chunk);size+=len(chunk)
        if size<=limit:
            kept.append(bytes(chunk))
        else:
            kept=[]
    try:
        if type(source) is bytes:
            view=memoryview(source)
            for start in range(0,len(view),1024**2):consume(view[start:start+1024**2])
        elif type(source) is BundleSource:
            with os.fdopen(os.open(source.path,os.O_RDONLY|os.O_NOFOLLOW),'rb') as stream:
                before=os.fstat(stream.fileno())
                if not stat.S_ISREG(before.st_mode):raise Refusal('input_failure','bundle kind')
                for chunk in iter(lambda:stream.read(1024**2),b''):consume(chunk)
                after=os.fstat(stream.fileno())
                if (before.st_size,before.st_mtime_ns,before.st_ino)!=(after.st_size,after.st_mtime_ns,after.st_ino):
                    raise Refusal('input_integrity','bundle changed')
        else:
            raise Refusal('input_failure','bundle source unavailable')
    except OSError:
        raise Refusal('input_failure','bundle read unavailable') from None
    actual=hashed.hexdigest()
    if actual!=expected:
        raise Refusal('input_integrity','bundle content address')
    return BoundBundle(size,actual,b''.join(kept) if size<=limit else None)


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


@dataclass(frozen=True)
class PackageSource:
    """A node-local package whose complete address precedes its size decision."""
    path: str


def package(raw, expected_sha256, inventory):
    if type(raw) is PackageSource:
        try:
            bound = bind_bundle(BundleSource(raw.path), expected_sha256, retention_limit=1048576)
        except Refusal as error:
            if error.row == 'input_integrity':
                raise
            raise Refusal('node_fault', 'local package unavailable') from None
        if bound.size > 1048576:
            raise Refusal('package_invalid', 'authenticated package size')
        raw = bound.data
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
        if C.canonical(obj) != raw:
            raise ValueError('noncanonical package')
    except (ValueError, TypeError, KeyError, RecursionError) as ex:
        raise Refusal('package_invalid', 'authenticated package grammar') from ex
    if obj['inventory_sha256'] != inventory.sha256 or obj['env_id'] != inventory.data()['environment']:
        raise Refusal('node_fault', 'package not in descriptor inventory/environment')
    return obj


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
    except (I.IdentityCap,C.JsonDepthError):
        raise Refusal('structural_cap' if role == 'submission' else 'node_fault', 'export limit') from None
    except I.IdentityError as ex:
        row = 'unknown_constant' if str(ex) == 'missing-constant' else 'front_invalid'
        raise Refusal(row if role == 'submission' else 'node_fault', 'export grammar') from None
    except (ValueError, TypeError, KeyError, RecursionError):
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


def public_name(name):
    """Lean 4.34.1 Lean/PrivateName.lean:38-64 privateToUserNameAux.

    isPrivatePrefix removes the first numeric component only when it is zero;
    an anonymous main module is legal. Later numeric components remain user data.
    """
    if name and name[0] == ('str','_private'):
        index=next((i for i,part in enumerate(name[1:],1) if part[0]=='num'),None)
        if index is not None and name[index][1]==0:return name[index+1:]
    return name


def reserved(name, *, reference=False):
    # Package/environment namespaces forbid new declarations, not references
    # to their authenticated definitions. Admitted Boole.R<k> stays disjoint.
    candidates=[public_name(name)]
    if name and name[0]==('str','_private'):
        # Also reserve namespace tails in noncanonical private wrappers, e.g.
        # _private.M.0.Foo.1.BooleCS.X. This policy view does not alias that
        # original Name to the Lean user Name Foo.1.BooleCS.X.
        candidates += [name[i+1:] for i,part in enumerate(name) if part[0]=='num' and i+1<len(name)]
    for current in candidates:
        if not current:continue
        if not reference and (current[0] in (('str','Leaf'),('str','BooleCS')) or
                len(current)>1 and current[:2]==(('str','Boole'),('str','Leaf'))):return True
        if (len(current)>1 and current[0]==('str','Boole') and current[1][0]=='str'
                and re.match(r'R(?:[0-9]+|$)',current[1][1]) is not None):return True
    return False


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
        if tag == 'natVal':
            if len(value) > NAT_DECIMAL_MAX_DIGITS:
                raise Refusal('structural_cap', 'own natural literal digit limit')
            # Nine-digit conversions are below every supported Python integer
            # string limit; no interpreter setting changes the admission rule.
            natural = 0
            for start in range(0, len(value), 9):
                chunk = value[start:start+9]
                natural = natural * 10**len(chunk) + int(chunk)
            if natural.bit_length() > 8 * LITERAL_BYTES:
                raise Refusal('structural_cap', 'own natural literal')
        if tag == 'strVal' and len(value.encode()) > LITERAL_BYTES:
            raise Refusal('structural_cap', 'own string literal')
        children = ([value['type'], value['body']] if tag in ('lam', 'forallE') else
                    [value[k] for k in ('type', 'value', 'body')] if tag == 'letE' else
                    [value['fn'], value['arg']] if tag == 'app' else
                    [value['struct']] if tag == 'proj' else [])
        stack.extend((child, depth+1) for child in children)


def partition(export, inventory, pkg):
    table = inventory.data(); own, base, library = [], [], []; mismatched = []
    # Inventory digest is verified before any miner data is inspected.
    for name, (kind, value) in export.declarations.items():
        try:
            key = name_key(name)
        except ValueError:
            raise Refusal('name_grammar', 'empty or invalid own name') from None
        if key in table['base'] or key in table['library']:
            group = 'base' if key in table['base'] else 'library'
            if payload(export, name) != table[group][key]:
                # An identical authenticated payload is cached input, not a new
                # declaration. Changed reserved payloads take the policy cause.
                if reserved(name):
                    raise Refusal('reserved_name', 'reserved redeclaration')
                mismatched.append(name)
            (base if group == 'base' else library).append(name)
        else:
            own.append(name)
    # Declarations precede reference policy, independent of export row order.
    if any(reserved(name) for name in own):
        raise Refusal('reserved_name', 'reserved declaration')
    own_expressions = {}
    own_dependencies = {}
    for name in own:
        _, value = export.declarations[name]
        starts = [value['type']] + ([value['value']] if 'value' in value else [])
        dependencies = export.dependencies(starts, set())
        if any(reserved(dep, reference=True) for dep in dependencies):
            raise Refusal('reserved_name', 'reserved reference')
        own_expressions[name], own_dependencies[name] = starts, dependencies
    if mismatched:
        raise Refusal('front_invalid', 'changed authenticated declaration')
    authenticated = set(base) | set(library)
    for name in own:
        selector(name)
        kind, value = export.declarations[name]
        starts = own_expressions[name]
        own_input_caps(export, starts)
        dependencies = own_dependencies[name]
        if any(dep[0] in (('str', 'Leaf'), ('str', 'BooleCS')) and dep not in authenticated
               for dep in dependencies):
            raise Refusal('unknown_constant', 'unauthenticated package reference')
        if value.get('isUnsafe') or value.get('safety', 'safe') != 'safe':
            raise Refusal('front_invalid', 'own unsafe/partial')
        if pkg['mode'] == 'certified-own' and kind not in ('def', 'thm'):
            raise Refusal('front_invalid', 'own declaration profile')
        if kind == 'axiom':
            raise Refusal('axiom_rejected', 'own axiom')
    target = tuple(tuple(part) for part in pkg['target'])
    if target not in own or export.declarations[target][0] != 'thm':
        raise Refusal('front_invalid', 'target not own theorem')
    # Unsupported allowed library content is a later S outcome; the adapter still
    # runs independently decidable statement/axiom comparisons before applying it.
    return Checked(tuple(sorted(own)), tuple(sorted(base)), tuple(sorted(library)))
