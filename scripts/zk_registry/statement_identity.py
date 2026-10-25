"""Versioned semantic statement closure from the gate's pinned lean4export 3.1 export.

Intern IDs and binder labels are transport details. Expression DAGs use Merkle hashes;
constant cycles are visited as a set, and theorem proof terms are never part of identity.
"""
from __future__ import annotations
import hashlib
import json
import os
import re
import stat
import time

VERSION = 'boole-statement-closure/v1'
SUFFIX = 'statement_closure_sha256'
MAX_BYTES = 2 * 1024**3
MAX_RECORDS = 3_000_000


class IdentityError(ValueError):
    pass


class IdentityCap(IdentityError):
    """A deterministic descriptor limit, distinct from host availability."""


class IdentityUnavailable(IdentityError):
    """A deadline or missing node input never decides a submission."""


def encoded(value):
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(',', ':')).encode()


def hashed(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def unique(pairs):
    out = {}
    for key, value in pairs:
        if key in out: raise IdentityError('duplicate-json-key')
        out[key] = value
    return out


def nat(value):
    if type(value) is not int or value < 0: raise IdentityError('natural-number-required')
    return value


def fields(value, keys):
    if not isinstance(value, dict) or set(value) != set(keys.split()):
        raise IdentityError('unexpected-export-fields')


def graph_hash(root, describe, memo, watch=lambda: None):
    """Iterative postorder; no exponential tree expansion or Python recursion limit."""
    active, stack = set(), [(root, False)]
    while stack:
        watch()
        if len(memo) > 6_000_000: raise IdentityCap('hash-work-limit')
        key, ready = stack.pop()
        if key in memo: continue
        children, make = describe(key)
        if ready:
            memo[key] = hashed(make([memo[c] for c in children]))
            active.remove(key)
            continue
        if key in active: raise IdentityError('cyclic-intern-graph')
        active.add(key)
        stack.append((key, True))
        for child in reversed(children):
            if child in active: raise IdentityError('cyclic-intern-graph')
            if child not in memo: stack.append((child, False))
    return memo[root]


DECL_FIELDS = {
    'axiom': 'name levelParams type isUnsafe',
    'def': 'name levelParams type value hints safety all',
    'opaque': 'name levelParams type value all isUnsafe',
    'thm': 'name levelParams type value all',
    'quot': 'name levelParams type kind',
    'types': 'name levelParams type numParams numIndices all ctors numNested isRec isReflexive isUnsafe',
    'ctors': 'name levelParams type induct cidx numParams numFields isUnsafe',
    'recs': 'name levelParams type all numParams numIndices numMotives numMinors rules k isUnsafe',
}


class Export:
    def __init__(self, path, timeout=1800):
        self.deadline = time.monotonic() + max(0, timeout)
        self.operations = 0
        self.blocks = []
        self.names, self.levels, self.exprs = {}, {}, {}
        self.declarations, self.groups = {}, {}
        self.name_cache, self.expr_cache, self.level_cache = {0: ()}, {}, {}
        self.pending = []
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, 'rb') as stream:
            before = os.fstat(stream.fileno())
            if not stat.S_ISREG(before.st_mode):
                raise IdentityUnavailable('export-kind')
            if before.st_size > MAX_BYTES:
                raise IdentityCap('export-byte-limit')
            raw = hashlib.sha256()
            meta = False
            total = 0
            for count, line in enumerate(iter(lambda: stream.readline(16*1024**2+1), b''), 1):
                self.watch()
                total += len(line)
                if total > MAX_BYTES: raise IdentityCap('export-byte-limit')
                if count > MAX_RECORDS or len(line) > 16*1024**2:
                    raise IdentityCap('export-record-limit')
                raw.update(line)
                try: row = json.loads(line, object_pairs_hook=unique)
                except (json.JSONDecodeError, UnicodeDecodeError) as error:
                    raise IdentityError('malformed-export-json') from error
                if not isinstance(row, dict): raise IdentityError('export-object-required')
                if set(row) == {'meta'}:
                    if (meta or count != 1 or not isinstance(row['meta'],dict)
                            or not isinstance(row['meta'].get('format'),dict)
                            or row['meta']['format'].get('version') != '3.1.0'):
                        raise IdentityError('export-format')
                    meta = True
                elif 'in' in row: self.put(self.names, row, 'in', {'str', 'num'})
                elif 'il' in row: self.put(self.levels, row, 'il', {'param', 'succ', 'max', 'imax'})
                elif 'ie' in row:
                    self.put(self.exprs, row, 'ie', {'bvar','sort','const','app','lam','forallE','letE','proj','natVal','strVal'})
                elif len(row) == 1 and next(iter(row)) in {'axiom','def','opaque','thm','quot'}:
                    kind = next(iter(row)); self.pending.append((kind, row[kind], None))
                elif set(row) == {'inductive'}:
                    group = row['inductive']; fields(group, 'types ctors recs')
                    self.blocks.append(group)
                    members = []
                    for kind in ['types', 'ctors', 'recs']:
                        if not isinstance(group[kind], list): raise IdentityError('inductive-members')
                        for value in group[kind]:
                            fields(value, DECL_FIELDS[kind]); members.append(value['name'])
                    for kind in ['types', 'ctors', 'recs']:
                        self.pending.extend((kind, value, members) for value in group[kind])
                else: raise IdentityError('unknown-export-record')
            after = os.fstat(stream.fileno())
            if not meta: raise IdentityError('export-format')
            if (before.st_size,before.st_mtime_ns,before.st_ino) != (after.st_size,after.st_mtime_ns,after.st_ino):
                raise IdentityError('changed-export')
            self.raw_sha256 = raw.hexdigest()
        for index in self.names: self.name(index)
        for kind, value, group in self.pending:
            fields(value, DECL_FIELDS[kind])
            name = self.name(value['name'])
            if not name or name in self.declarations: raise IdentityError('duplicate-declaration')
            self.declarations[name] = (kind, value)
            if group is not None: self.groups[name] = [self.name(n) for n in group]
        del self.pending
        self.validate_transport()

    @staticmethod
    def put(table, row, index_key, variants):
        index = nat(row[index_key]); keys = set(row) - {index_key}
        if index in table or len(keys) != 1 or not keys <= variants:
            raise IdentityError('duplicate-id-or-unknown-form')
        if index_key in ['in', 'il'] and index == 0: raise IdentityError('reserved-intern-id')
        table[index] = (next(iter(keys)), row[next(iter(keys))])

    def name(self, index):
        nat(index)
        chain, active = [], set()
        while index not in self.name_cache:
            if index in active or index not in self.names: raise IdentityError('bad-name-graph')
            if len(chain) >= 256: raise IdentityCap('name-depth-limit')
            self.watch()
            active.add(index); kind, body = self.names[index]
            fields(body, 'pre str' if kind == 'str' else 'pre i')
            value = body['str'] if kind == 'str' else nat(body['i'])
            if kind == 'str' and not isinstance(value, str): raise IdentityError('name-string')
            chain.append((index,kind,value)); index = nat(body['pre'])
        result = self.name_cache[index]
        for index,kind,value in reversed(chain):
            result = (*result,(kind,value))
            if len(result) > 256: raise IdentityCap('name-depth-limit')
            self.name_cache[index] = result
        return result

    def watch(self):
        self.operations += 1
        if self.operations % 1024 == 0 and time.monotonic() >= self.deadline:
            raise IdentityUnavailable('identity-deadline')

    def validate_transport(self):
        """Validate ignored proof transport too; this is separate from kernel soundness."""
        def ref(table, index, zero=False):
            nat(index)
            if not (zero and index == 0) and index not in table: raise IdentityError('dangling-intern-reference')
        def name_list(value):
            if not isinstance(value,list): raise IdentityError('name-list')
            return [self.name(n) for n in value]
        def acyclic(edges):
            done = set()
            for root in edges:
                active,stack = set(),[(root,False)]
                while stack:
                    self.watch(); node,ready = stack.pop()
                    if node in done: continue
                    if ready: active.remove(node);done.add(node);continue
                    if node in active: raise IdentityError('cyclic-intern-graph')
                    active.add(node);stack.append((node,True))
                    for child in edges[node]:
                        if child in active: raise IdentityError('cyclic-intern-graph')
                        if child not in done: stack.append((child,False))
        levels = {0:[]}
        for index,(tag,value) in self.levels.items():
            self.watch()
            if tag == 'param': self.name(value); children=[]
            elif tag == 'succ': children=[value]
            else:
                if not isinstance(value,list) or len(value)!=2: raise IdentityError('universe-arity')
                children=value
            for child in children: ref(self.levels,child,True)
            levels[index]=children
        acyclic(levels)
        expressions = {}
        for index,(tag,value) in self.exprs.items():
            self.watch();children=[]
            if tag == 'bvar': nat(value)
            elif tag == 'sort': ref(self.levels,value,True)
            elif tag == 'const':
                fields(value,'name us'); name=self.name(value['name'])
                if name not in self.declarations: raise IdentityError('missing-constant')
                if not isinstance(value['us'],list): raise IdentityError('universe-arguments')
                for level in value['us']: ref(self.levels,level,True)
            elif tag in ['lam','forallE']:
                fields(value,'name type body binderInfo');self.name(value['name'])
                if value['binderInfo'] not in ['default','implicit','strictImplicit','instImplicit']: raise IdentityError('binder-info')
                children=[value['type'],value['body']]
            elif tag == 'letE':
                fields(value,'name type value body nondep');self.name(value['name'])
                if value['nondep'] is not False: raise IdentityError('let-nondep')
                children=[value[k] for k in ['type','value','body']]
            elif tag == 'app': fields(value,'fn arg');children=[value['fn'],value['arg']]
            elif tag == 'proj':
                fields(value,'typeName idx struct');name=self.name(value['typeName']);nat(value['idx'])
                if name not in self.declarations or self.declarations[name][0]!='types': raise IdentityError('projection-type')
                children=[value['struct']]
            elif tag == 'natVal':
                if not isinstance(value,str) or not re.fullmatch('0|[1-9][0-9]*',value): raise IdentityError('nat-literal')
            elif tag == 'strVal':
                if not isinstance(value,str): raise IdentityError('string-literal')
            for child in children: ref(self.exprs,child)
            expressions[index]=children
        acyclic(expressions)
        for kind,value in self.declarations.values():
            self.watch();self.context(value['levelParams']);ref(self.exprs,value['type'])
            if 'value' in value: ref(self.exprs,value['value'])
            for key in ['all','ctors']:
                if key in value: name_list(value[key])
            for key in ['isUnsafe','isRec','isReflexive','k']:
                if key in value and type(value[key]) is not bool: raise IdentityError('boolean-field')
            if value.get('isUnsafe',False) or value.get('safety','safe') != 'safe': raise IdentityError('unsafe-declaration')
            if kind=='def':
                hints=value['hints']
                if isinstance(hints,dict): fields(hints,'regular');nat(hints['regular'])
                elif hints not in ['opaque','abbrev']: raise IdentityError('reducibility-hints')
            if kind=='quot' and value['kind'] not in ['type','ctor','lift','ind']: raise IdentityError('quot-kind')
            if 'induct' in value: self.name(value['induct'])
            for key in ['numParams','numIndices','numNested','numFields','cidx','numMotives','numMinors']:
                if key in value: nat(value[key])
            if 'rules' in value:
                if not isinstance(value['rules'],list): raise IdentityError('recursor-rules')
                for rule in value['rules']:
                    fields(rule,'ctor nfields rhs');self.name(rule['ctor']);nat(rule['nfields']);ref(self.exprs,rule['rhs'])
        for block in self.blocks:
            if not block['types']: raise IdentityError('empty-inductive-block')
            types=[self.name(v['name']) for v in block['types']]
            ctors={self.name(v['name']):v for v in block['ctors']}
            for value in block['types']:
                name=self.name(value['name'])
                if name_list(value['all'])!=types: raise IdentityError('inductive-family')
                declared=name_list(value['ctors'])
                actual=[self.name(v['name']) for v in block['ctors'] if self.name(v['induct'])==name]
                if declared!=actual: raise IdentityError('inductive-constructors')
                for i,n in enumerate(actual):
                    if ctors[n]['cidx']!=i: raise IdentityError('constructor-index')
            for value in block['ctors']:
                if self.name(value['induct']) not in types: raise IdentityError('constructor-family')
            for value in block['recs']:
                if name_list(value['all'])!=types: raise IdentityError('recursor-family')
                seen=set()
                for rule in value['rules']:
                    name=self.name(rule['ctor'])
                    constructor = self.declarations.get(name)
                    if (constructor is None or constructor[0]!='ctors' or name in seen
                            or rule['nfields']!=constructor[1]['numFields']):
                        raise IdentityError('recursor-constructor')
                    seen.add(name)

    def context(self, value):
        if not isinstance(value, list): raise IdentityError('universe-parameters')
        context = tuple(self.name(v) for v in value)
        if len(set(context)) != len(context): raise IdentityError('duplicate-universe-parameter')
        return context

    def level(self, index, context):
        nat(index)
        def describe(key):
            i,ctx = key
            if i == 0: return [], lambda _: ['zero']
            if i not in self.levels: raise IdentityError('missing-level')
            tag,value = self.levels[i]
            if tag == 'param':
                name = self.name(value)
                if name not in ctx: raise IdentityError('unbound-universe-parameter')
                return [], lambda _: ['param',ctx.index(name)]
            refs = [nat(value)] if tag == 'succ' else [nat(v) for v in value]
            if tag != 'succ' and (not isinstance(value,list) or len(refs)!=2):
                raise IdentityError('universe-arity')
            return [(v,ctx) for v in refs], lambda children: [tag,*children]
        return graph_hash((index,context), describe, self.level_cache, self.watch)

    def expression(self, index, context):
        def describe(key):
            i,ctx = key; nat(i)
            if i not in self.exprs: raise IdentityError('missing-expression')
            tag,value = self.exprs[i]
            refs, data = self.expression_shape(tag,value,ctx)
            return [(r,ctx) for r in refs], lambda children: [tag,data,*children]
        return graph_hash((index,context), describe, self.expr_cache, self.watch)

    def expression_shape(self, tag, value, context):
        if tag == 'bvar': return [], nat(value)
        if tag == 'sort': return [], self.level(value,context)
        if tag == 'natVal':
            if not isinstance(value,str) or not re.fullmatch('0|[1-9][0-9]*',value): raise IdentityError('nat-literal')
            return [], value
        if tag == 'strVal':
            if not isinstance(value,str): raise IdentityError('string-literal')
            return [], value
        if tag == 'const':
            fields(value,'name us')
            if not isinstance(value['us'],list): raise IdentityError('universe-arguments')
            return [], [self.name(value['name']),[self.level(v,context) for v in value['us']]]
        if tag == 'app':
            fields(value,'fn arg'); return [nat(value['fn']),nat(value['arg'])], None
        if tag in ['lam','forallE']:
            fields(value,'name type body binderInfo'); self.name(value['name'])
            if value['binderInfo'] not in ['default','implicit','strictImplicit','instImplicit']: raise IdentityError('binder-info')
            return [nat(value['type']),nat(value['body'])], value['binderInfo']
        if tag == 'letE':
            fields(value,'name type value body nondep'); self.name(value['name'])
            if value['nondep'] is not False: raise IdentityError('let-nondep')
            return [nat(value[k]) for k in ['type','value','body']], None
        if tag == 'proj':
            fields(value,'typeName idx struct')
            return [nat(value['struct'])], [self.name(value['typeName']),nat(value['idx'])]
        raise IdentityError('unsupported-expression')

    def dependencies(self, starts, seen):
        names, stack = set(), list(starts)
        while stack:
            self.watch()
            index = nat(stack.pop())
            if index in seen: continue
            seen.add(index)
            if index not in self.exprs: raise IdentityError('missing-expression')
            tag,value = self.exprs[index]
            if tag == 'const': names.add(self.name(value['name']))
            elif tag == 'proj': names.add(self.name(value['typeName'])); stack.append(value['struct'])
            elif tag in ['lam','forallE']: stack.extend([value['type'],value['body']])
            elif tag == 'letE': stack.extend(value[k] for k in ['type','value','body'])
            elif tag == 'app': stack.extend([value['fn'],value['arg']])
            elif tag == 'natVal': names.add((('str','Nat'),))
            elif tag == 'strVal': names.update([(('str','Char'),('str','ofNat')), (('str','String'),('str','ofList'))])
        return names

    def declaration(self, name):
        if name not in self.declarations: raise IdentityError('missing-constant')
        kind,value = self.declarations[name]; ctx = self.context(value['levelParams'])
        if value.get('isUnsafe',False) is not False or value.get('safety','safe') != 'safe':
            raise IdentityError('unsafe-declaration')
        out = dict(kind=kind, name=name, universe_parameters=len(ctx), type=self.expression(value['type'],ctx))
        exprs = [value['type']]
        if kind in ['def','opaque']:
            out['value'] = self.expression(value['value'],ctx); exprs.append(value['value'])
        if kind in ['types','ctors','recs']:
            for field in ['all','ctors']:
                if field in value: out[field] = [self.name(n) for n in value[field]]
            if 'induct' in value: out['induct'] = self.name(value['induct'])
            for field in ['numParams','numIndices','numNested','numFields','cidx','numMotives','numMinors']:
                if field in value: out[field] = nat(value[field])
            for field in ['isRec','isReflexive','k']:
                if field in value:
                    if type(value[field]) is not bool: raise IdentityError('boolean-field')
                    out[field] = value[field]
            if 'rules' in value:
                out['rules'] = []
                for rule in value['rules']:
                    fields(rule,'ctor nfields rhs'); exprs.append(rule['rhs'])
                    out['rules'].append(dict(ctor=self.name(rule['ctor']),nfields=nat(rule['nfields']),rhs=self.expression(rule['rhs'],ctx)))
        if kind == 'quot': out['quot_kind'] = value['kind']
        return out,exprs

    def identity(self, theorem):
        # Public FQNs use string components; quoted components remain distinct from dotted names.
        parts = re.findall(r'«[^»]*»|[^.]+',theorem)
        if '.'.join(parts) != theorem: raise IdentityError('target-name')
        root = tuple(('str',p[1:-1] if p.startswith('«') and p.endswith('»') else p) for p in parts)
        if root not in self.declarations or self.declarations[root][0] != 'thm':
            raise IdentityError('statement-theorem-missing')
        _,value = self.declarations[root]; ctx = self.context(value['levelParams'])
        root_type = self.expression(value['type'],ctx)
        seen_exprs, visited = set(), {}
        queue = list(self.dependencies([value['type']],seen_exprs))
        while queue:
            name = queue.pop()
            if name in visited: continue
            payload,exprs = self.declaration(name); visited[name] = payload
            queue.extend(self.dependencies(exprs,seen_exprs))
            queue.extend(self.groups.get(name,[]))
            if self.declarations[name][0] == 'recs':
                queue.extend(self.name(rule['ctor']) for rule in self.declarations[name][1]['rules'])
            if self.declarations[name][0] == 'quot':
                queue.extend([(('str','Quot'),),*( (('str','Quot'),('str',p)) for p in ['mk','lift','ind']), (('str','Eq'),)])
        closure = [visited[name] for name in sorted(visited,key=encoded)]
        digest = hashed(dict(schema=VERSION,universe_parameters=len(ctx),statement_type=root_type,constants=closure))
        return {SUFFIX:digest,'statement_closure_version':VERSION,'root_type_sha256':root_type,
                'semantic_constants':len(closure),'export_sha256':self.raw_sha256}


def identity(path, theorem, timeout=1800):
    if timeout <= 0:
        raise IdentityUnavailable('statement-identity-timeout')
    return Export(path,timeout).identity(theorem)


if __name__ == '__main__':
    import argparse
    import sys
    parser = argparse.ArgumentParser(description='Hash a gate-owned Lean semantic statement closure')
    parser.add_argument('export')
    parser.add_argument('theorem')
    parser.add_argument('--timeout', type=float, default=1800)
    args = parser.parse_args()
    try:
        print(json.dumps(identity(args.export, args.theorem, args.timeout), sort_keys=True))
    except (IdentityError, OSError, ValueError, TypeError, KeyError, OverflowError, MemoryError) as exc:
        # Only this gate-owned worker chooses the code. Diagnostic text is not authority.
        print(type(exc).__name__, file=sys.stderr)
        sys.exit(3 if isinstance(exc, IdentityCap) else
                 70 if isinstance(exc, (IdentityUnavailable, OSError, MemoryError)) else 4)
