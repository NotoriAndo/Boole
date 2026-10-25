"""Deterministic closed-local registry catalog, commit/reveal intake and rechecked replay.

This service has no reward, signature, network, consensus or activation path.
Package validators and per-kind production checkers remain the acceptance authority.
"""

from __future__ import annotations

from collections import Counter
from contextlib import contextmanager
import argparse
import copy
import fcntl
import hashlib
import json
import os
from zk_registry import safe_fs as FS
from .safe_fs import Path
import re
import shutil
import sys
import tempfile
import time

from . import package as P
from . import spec_problem as SP
from . import error_policy as E

RATCHET_KINDS = {
    P.RATCHET_SCHEMA_VERSION: 'circom',
    P.RATCHET_NOIR_SCHEMA_VERSION: 'noir',
    P.RATCHET_GNARK_SCHEMA_VERSION: 'gnark',
    P.RATCHET_AIR_SCHEMA_VERSION: 'zkvm-air',
    P.RATCHET_ZOKRATES_SCHEMA_VERSION: 'zokrates',
    P.RATCHET_HALO2_SCHEMA_VERSION: 'halo2',
}
UNSUPPORTED_SPEC = 'tracked tooling has no general reference-differential/mutant/challenger rerunner'
COMMIT_DOMAIN = 'boole-zk-registry-commit/v1'
EVENT_DOMAIN = b'boole-zk-registry-event/v1\0'
ZERO_HASH = '0' * 64
SERVICE_SOURCES = ('service.py', 'service_check.py', 'catalog_v2.py', 'error_policy.py', 'data/error_policy_v1.json', 'data/error_policy_v2.json')


class RegistryError(ValueError):
    """Malformed or unavailable local registry input; no acceptance follows."""


def canonical(value) -> bytes:
    """One UTF-8 serialization for identities, event hashes and state comparison."""
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False,
                      allow_nan=False).encode('utf-8')


def digest(value) -> str:
    """SHA256 of canonical metadata."""
    return hashlib.sha256(canonical(value)).hexdigest()


def source_info() -> dict:
    """Informational run provenance for this service, separate from package-generator identities."""
    here = Path(__file__).resolve().parent
    return dict(name='closed-local-registry-service/v1', sources_sha256=digest({
        name: P.sha256_file(str(here / name)) for name in SERVICE_SOURCES}))


def read_json(path: Path) -> dict:
    """Read one JSON object; reject arrays and nonstandard numeric values."""
    def constant(_):
        raise RegistryError('nonstandard JSON number')

    def pairs(rows):
        result = {}
        for key, value in rows:
            if key in result:
                raise RegistryError('duplicate JSON object key')
            result[key] = value
        return result

    value = json.loads(path.read_text(encoding='utf-8'), parse_constant=constant, object_pairs_hook=pairs)
    if not isinstance(value, dict):
        raise RegistryError('JSON object required')
    return value


def request_statement(problem: dict) -> bytes:
    """The public request bytes; R's exact Lean theorem is generated per candidate."""
    schema = problem['schema_version']
    if schema in RATCHET_KINDS:
        return canonical(dict(kind='R', reference=problem['reference']['model'], record=problem['record'],
                              relation='candidate and immutable reference have equal input/output relations',
                              schema=schema)) + b'\n'
    if schema == P.CORRECTNESS_GNARK_SCHEMA_VERSION:
        return problem['statement']['text'].encode('utf-8')
    return canonical({key: problem[key] for key in
                      ('family', 'standard', 'parameters', 'reference', 'mutants', 'protocol')}) + b'\n'


def catalog_entry(problem: dict, directory: str) -> dict | None:
    """Filter before reading private contents, then validate every admitted package."""
    schema, status = problem.get('schema_version'), problem.get('status')
    if schema in RATCHET_KINDS and status == 'OPEN':
        kind, framework = 'R', RATCHET_KINDS[schema]
    elif schema == P.CORRECTNESS_GNARK_SCHEMA_VERSION and status == 'OPEN':
        if problem.get('trivial', {}).get('structural') or problem.get('trivial', {}).get('battery'):
            return None
        kind, framework = 'C', 'gnark'
    elif schema == P.SPEC_SCHEMA_VERSION and status in ('OPEN', 'ACCEPTED-LOCAL'):
        kind, framework = 'S', 'lean-spec'
    else:
        return None
    try:
        errors = P.validate_problem(problem, directory)
    except (OSError, ValueError, KeyError, TypeError, IndexError) as ex:
        raise RegistryError('invalid package: ' + type(ex).__name__) from ex
    if errors:
        raise RegistryError('invalid package: ' + '; '.join(errors[:5]))
    if kind == 'S' and SP.authoring_prerequisites(problem, directory):
        return None
    entry = dict(id=problem['package_id'], kind=kind, framework=framework, status=status,
                statement_sha256=hashlib.sha256(request_statement(problem)).hexdigest(),
                statement_closure_sha256=problem.get('statement_closure_sha256'),
                statement=request_statement(problem).decode('utf-8'),
                record=copy.deepcopy(problem.get('record')), spec=copy.deepcopy(problem.get('spec')),
                submittable=kind != 'S', unavailable_reason=UNSUPPORTED_SPEC if kind == 'S' else None,
                rung=0, root_id=problem['package_id'])
    from . import acceptance as A
    try:
        binding = A.read(directory, problem)
    except (OSError, ValueError, KeyError, TypeError) as ex:
        raise RegistryError('invalid acceptance binding: ' + str(ex)) from ex
    if binding is not None:
        entry['acceptance'] = A.public(binding)
        entry['statement_closure_sha256'] = bound_statement_identity(problem, directory, binding, framework)
    return entry


def bound_statement_identity(problem: dict, directory: str, binding, framework: str) -> str:
    """Hash the authenticated reference export; request text alone is not a Lean type."""
    import tempfile
    from . import closure as D

    theorem = problem.get('checker', {}).get('theorem_fqn')
    if not theorem:
        from . import ratchet, ratchet_air, ratchet_gnark, ratchet_noir, ratchet_native
        adapter = {'circom': ratchet, 'gnark': ratchet_gnark, 'zkvm-air': ratchet_air,
                   'noir': ratchet_noir, 'halo2': ratchet_native, 'zokrates': ratchet_native}.get(framework)
        if adapter is None:
            raise RegistryError('reference theorem unavailable')
        theorem = adapter.namespaces(problem)[0] + '.equiv'
    export = binding.manifest['challenge_export']
    with tempfile.TemporaryDirectory(prefix='boole-catalog-identity-') as temporary:
        semantic = D.statement_identity(str(Path(directory) / export['path']), theorem, temporary, 1800,
                                        expected_sha256=export['sha256'])
    if semantic['verdict'] != 'PASS':
        raise RegistryError('reference identity unavailable: ' + semantic['reason'])
    value = semantic['statement_closure_sha256']
    advertised = problem.get('statement_closure_sha256')
    if advertised is not None and advertised != value:
        raise RegistryError('statement-closure-mismatch')
    return value


def build_catalog(directories: list[str]) -> dict:
    """Build a sorted validated catalog; exclusions disclose counts, never private identities."""
    entries, packages, excluded = {}, {}, Counter()
    for directory in sorted(set(str(Path(path).resolve()) for path in directories)):
        problem = read_json(Path(directory) / 'problem.json')
        entry = catalog_entry(problem, directory)
        if entry is None:
            excluded['private-or-nonissuable'] += 1
            continue
        ident = entry['id']
        metadata_sha = P.sha256_file(str(Path(directory) / 'problem.json'))
        if ident in entries:
            if packages[ident]['metadata_sha256'] != metadata_sha:
                raise RegistryError('conflicting package identity')
            excluded['duplicate'] += 1
            continue
        entries[ident] = entry
        packages[ident] = dict(directory=directory, metadata_sha256=metadata_sha)
    return dict(entries=[entries[key] for key in sorted(entries)], packages=packages,
                excluded=dict(sorted(excluded.items())))


def catalog_from_roots(roots: list[str], workspace: str | None = None) -> dict:
    """Read active wave indexes, excluding aliases/inventory before publishing any identity."""
    directories, excluded = [], Counter()
    for value in roots:
        root = Path(value).resolve()
        if (root / 'problem.json').is_file():
            directories.append(str(root))
            continue
        waves = [root] if (root / 'INDEX.jsonl').is_file() else [
            path for path in sorted(root.iterdir()) if path.is_dir()
            and (path.name.startswith('ratchet-') or path.name in ('correctness-cg1', 'spec-sg1'))]
        if not waves:
            raise RegistryError('no supported package wave found')
        for wave in waves:
            index = wave / 'INDEX.jsonl'
            if not index.is_file():
                raise RegistryError('wave index unavailable')
            for line in index.read_text(encoding='utf-8').splitlines():
                row = json.loads(line)
                if (row.get('active') is False or row.get('duplicate_model') is True
                        or row.get('eligible_after_local_floor') is False
                        or row.get('status', 'OPEN') not in ('OPEN', 'ACCEPTED-LOCAL')):
                    excluded['inactive-or-noncanonical'] += 1
                    continue
                rel = row.get('dir') or row.get('path')
                safe_relative(rel)
                if not rel.startswith('local-docs/'):
                    directory = wave / rel
                elif workspace:
                    directory = Path(workspace).resolve() / rel
                else:
                    base = next((parent for parent in wave.parents if parent.name == 'local-docs'), None)
                    if base is None or not rel.startswith('local-docs/'):
                        raise RegistryError('workspace root required for indexed path')
                    directory = base.parent / rel
                permitted = Path(workspace).resolve() if workspace else root
                try:
                    directory.resolve().relative_to(permitted)
                except ValueError as ex:
                    raise RegistryError('indexed package escapes supplied scope') from ex
                metadata = directory / 'problem.json'
                if row.get('metadata_sha256') and P.sha256_file(str(metadata)) != row['metadata_sha256']:
                    raise RegistryError('indexed metadata digest differs')
                directories.append(str(directory))
    catalog = build_catalog(directories)
    excluded.update(catalog['excluded'])
    catalog['excluded'] = dict(sorted(excluded.items()))
    return catalog


def safe_relative(value: str) -> str:
    """No absolute/escaped paths or ambiguous separators in an object manifest."""
    if (not isinstance(value, str) or not value or '\\' in value or '\0' in value
            or Path(value).is_absolute() or any(part in ('', '.', '..') for part in value.split('/'))):
        raise RegistryError('unsafe manifest path')
    return value


def file_manifest(directory: str, package=False) -> list[dict]:
    """Snapshot regular files only; compiled Lean/cache files are never checker inputs."""
    root = Path(directory)
    if not root.is_dir() or root.is_symlink():
        raise RegistryError('regular input directory required')
    manifest = []
    total = 0
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raise RegistryError('symbolic links are not admitted')
        if path.is_dir():
            continue
        rel = path.relative_to(root).as_posix()
        safe_relative(rel)
        if package and (path.suffix in ('.olean', '.ilean') or '__pycache__' in path.parts):
            continue
        if not path.is_file():
            raise RegistryError('non-regular input file')
        size = path.stat().st_size
        total += size
        if not package and (size > 16 * 1024 * 1024 or total > 64 * 1024 * 1024 or len(manifest) >= 1000):
            raise RegistryError('submission size limit exceeded')
        manifest.append(dict(path=rel, sha256=P.sha256_file(str(path)), size=size))
    if not manifest:
        raise RegistryError('empty input directory')
    return manifest


def submission_digest(directory: str) -> str:
    """Bind every proof/candidate file, relative filename and byte length."""
    return digest(file_manifest(directory))


def commitment(problem_id: str, solver_id: str, submission_sha256: str, salt: str) -> str:
    """Canonical, domain-separated opaque-id commit; identities are not signatures."""
    for value in (problem_id, solver_id, salt):
        if not isinstance(value, str) or not value or len(value.encode('utf-8')) > 4096:
            raise RegistryError('nonempty bounded commit fields required')
    if not isinstance(submission_sha256, str) or not re.fullmatch(r'[0-9a-f]{64}', submission_sha256):
        raise RegistryError('submission SHA256 required')
    return digest(dict(domain=COMMIT_DOMAIN, problem=problem_id, solver=solver_id,
                       submission=submission_sha256, salt=salt))


def event_hash(event: dict) -> str:
    """Effect-0 receipt identity, including predecessor and all exact event inputs."""
    return hashlib.sha256(EVENT_DOMAIN + canonical(event)).hexdigest()


class Registry:
    """One locked append-only log plus immutable objects; state is derived, never authoritative cache."""

    def __init__(self, directory: str, verifier=None, *, gate=None, gates=None, clock=None):
        self.root = Path(directory).resolve()
        self.verifier = verifier
        self.clock = clock or time.time
        config = getattr(verifier, 'config', {})
        resource_keys = {'wall_s','rss_mb','memory_max','worker_id','host_id','canary','attempt_index','gate_sha256'}
        self.gate = gate or config.get('gate_sha256') or digest(dict(service=source_info(), generator=P.generator_info(),
                                       configuration={key:value for key,value in config.items() if key not in resource_keys}))
        if not isinstance(self.gate, str) or not re.fullmatch(r'[0-9a-f]{64}', self.gate):
            raise RegistryError('invalid gate digest')
        self.gates = dict(gates or {})
        self.gates[self.gate] = verifier

    @classmethod
    def create(cls, directory: str, catalog: dict, reveal_window=3600, challenge_window=86400, verifier=None, *, error_policy='current', gate=None):
        registry = cls(directory, verifier, gate=gate)
        for value in (reveal_window, challenge_window):
            if type(value) is not int or value < 1:
                raise RegistryError('positive integer windows required')
        registry._validate_catalog(dict(catalog, reveal_window=reveal_window, challenge_window=challenge_window))
        registry.root.mkdir(parents=True, exist_ok=True)
        with registry._locked():
            if registry.log.exists():
                raise RegistryError('store already initialized')
            packages = {}
            for entry in catalog['entries']:
                source = catalog['packages'][entry['id']]
                source_dir = source['directory']
                if P.sha256_file(str(Path(source_dir) / 'problem.json')) != source['metadata_sha256']:
                    raise RegistryError('catalog input changed')
                manifest = registry._capture(source_dir, package=True)
                with registry._materialize(manifest) as bundle:
                    problem = read_json(Path(bundle) / 'problem.json')
                    if catalog_entry(problem, bundle) != entry:
                        raise RegistryError('catalog snapshot differs from validated input')
                packages[entry['id']] = manifest
            payload = dict(entries=catalog['entries'], packages=packages, excluded=catalog['excluded'],
                           reveal_window=reveal_window, challenge_window=challenge_window,
                           submission_policy=dict(version=5, sha256=P.sha256_file(str(Path(__file__).parent / 'data/submission_policy_v5.json'))))
            if error_policy is not None:
                payload['error_policy'] = (dict(version=E.RULES['version'], rules_sha256=P.sha256_file(str(E.POLICY_PATH)))
                                           if error_policy == 'current' else copy.deepcopy(error_policy))
                payload['gate'] = registry.gate
                registry._policy(payload)
            registry._append('catalog', payload, 0, [])
        return registry

    @property
    def log(self):
        return self.root / 'events.jsonl'

    @contextmanager
    def _locked(self):
        if not self.root.is_dir():
            raise RegistryError('store is unavailable')
        with (self.root / 'lock').open('a+b') as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(stream, fcntl.LOCK_UN)

    def _capture(self, directory, package=False):
        manifest = file_manifest(directory, package)
        objects = self.root / 'objects'
        objects.mkdir(exist_ok=True)
        for row in manifest:
            data = (Path(directory) / row['path']).read_bytes()
            if hashlib.sha256(data).hexdigest() != row['sha256'] or len(data) != row['size']:
                raise RegistryError('input changed during snapshot')
            target = objects / row['sha256']
            try:
                with target.open('xb') as stream:
                    stream.write(data)
                    stream.flush()
                    os.fsync(stream.fileno())
            except FileExistsError:
                if target.is_symlink() or target.read_bytes() != data:
                    raise RegistryError('content object corrupted')
        if file_manifest(directory, package) != manifest:
            raise RegistryError('input changed during snapshot')
        return manifest

    @contextmanager
    def _materialize(self, manifest):
        work = self.root / 'scratch'
        work.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='bundle-', dir=work) as directory:
            seen = set()
            for row in manifest:
                rel = safe_relative(row['path'])
                if rel in seen:
                    raise RegistryError('duplicate manifest path')
                seen.add(rel)
                if not re.fullmatch(r'[0-9a-f]{64}', row['sha256']):
                    raise RegistryError('invalid object digest')
                source = self.root / 'objects' / row['sha256']
                if not source.is_file() or source.is_symlink():
                    raise RegistryError('content object unavailable')
                data = source.read_bytes()
                if hashlib.sha256(data).hexdigest() != row['sha256'] or len(data) != row['size']:
                    raise RegistryError('content object corrupted')
                target = Path(directory) / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
            yield directory

    def _events(self):
        if not self.log.is_file() or self.log.is_symlink():
            raise RegistryError('event log unavailable')
        raw = self.log.read_bytes()
        if not raw.endswith(b'\n'):
            raise RegistryError('incomplete event log; source preserved')
        events, previous = [], ZERO_HASH
        for line in raw.splitlines():
            try:
                event = json.loads(line)
                claimed = event.pop('hash')
                if (event['sequence'] != len(events) + 1 or event['previous'] != previous
                        or event_hash(event) != claimed or canonical(dict(event, hash=claimed)) != line):
                    raise RegistryError('event hash chain differs')
            except (ValueError, KeyError, TypeError, AttributeError) as ex:
                raise RegistryError('invalid event log') from ex
            previous = claimed
            events.append(dict(event, hash=claimed))
        return events

    def _append(self, kind, payload, logical_time, events):
        event = dict(sequence=len(events) + 1, previous=events[-1]['hash'] if events else ZERO_HASH,
                     type=kind, time=logical_time, payload=payload)
        event['hash'] = event_hash(event)
        with self.log.open('ab') as stream:
            stream.write(canonical(event) + b'\n')
            stream.flush()
            os.fsync(stream.fileno())
        return event

    @staticmethod
    def _empty_state():
        return dict(problems={}, commits={}, last_time=0, last_event=ZERO_HASH, packages={},
                    reveal_window=0, challenge_window=0, excluded={})

    @staticmethod
    def _validate_catalog(payload):
        """Genesis configuration is executable replay input, not a trusted historical label."""
        if any(type(payload.get(key)) is not int or payload[key] < 1
               for key in ('reveal_window', 'challenge_window')):
            raise RegistryError('invalid catalog windows')
        Registry._policy(payload)
        entries = payload.get('entries')
        if not isinstance(entries, list) or any(not isinstance(row, dict) or not isinstance(row.get('id'), str)
                                                for row in entries):
            raise RegistryError('invalid catalog entries')
        ids = [row['id'] for row in entries]
        if len(set(ids)) != len(ids) or not isinstance(payload.get('packages'), dict):
            raise RegistryError('duplicate or invalid catalog identities')
        if set(payload['packages']) != set(ids):
            raise RegistryError('catalog package identities differ')

    @staticmethod
    def _policy(state):
        descriptor = state.get('error_policy')
        if descriptor is None:
            return None
        try:
            rules = E.load(descriptor)
        except (OSError, ValueError, KeyError, TypeError) as ex:
            raise RegistryError('error-policy-mismatch') from ex
        return rules

    @staticmethod
    def _terminal(verdict, rules):
        if verdict['verdict'] != 'ERROR':
            return verdict['verdict'] != 'PASS'
        return bool(rules and verdict.get('reason') in rules['terminal_reasons']
                    and verdict.get('attempt', {}).get('class') in rules['terminal_groups'])

    @staticmethod
    def _advance(state, logical_time):
        if type(logical_time) is not int or logical_time < state['last_time']:
            raise RegistryError('logical time must not move backwards')
        state['last_time'] = logical_time
        for commit in state['commits'].values():
            if commit['status'] == 'COMMITTED' and logical_time >= commit['deadline']:
                commit['status'] = 'EXPIRED'
        for problem in state['problems'].values():
            if (problem['status'] == 'ACCEPTED-PENDING-CHALLENGE'
                    and logical_time >= problem['challenge_deadline']):
                problem['status'] = 'ACCEPTED'
        Registry._settle(state)

    @staticmethod
    def _settle(state):
        for key in sorted(state['commits'], key=int):
            commit = state['commits'][key]
            if commit['status'] != 'VALID-REVEAL':
                continue
            problem = state['problems'][commit['problem']]
            if 'winner' in problem:
                commit['status'] = 'DUPLICATE'
                continue
            earlier = [row for seq, row in state['commits'].items()
                       if int(seq) < int(key) and row['problem'] == commit['problem']
                       and row['status'] in ('COMMITTED', 'VALID-REVEAL', 'CHECKING', 'QUARANTINED')]
            if earlier:
                continue
            commit['status'] = 'ACCEPTED'
            problem.update(winner=int(key), receipt=commit['receipt'])
            if problem['kind'] == 'C':
                problem['status'] = 'VERIFIED'
            elif problem['kind'] == 'S':
                problem.update(status='ACCEPTED-PENDING-CHALLENGE',
                               challenge_deadline=state['last_time'] + state['challenge_window'])
            else:
                problem['status'] = 'ACCEPTED'
                record = commit['verdict']['record']
                problem['accepted_record'] = record
                next_problem = copy.deepcopy(problem)
                for field in ('winner', 'receipt', 'accepted_record'):
                    next_problem.pop(field, None)
                rung = problem['rung'] + 1
                next_id = problem['root_id'] + '@' + str(rung)
                next_problem.update(id=next_id, rung=rung, status='OPEN', record=record)
                statement = json.loads(next_problem['statement'])
                statement['record'] = record
                next_problem['statement'] = (canonical(statement) + b'\n').decode('utf-8')
                next_problem['statement_sha256'] = hashlib.sha256(next_problem['statement'].encode()).hexdigest()
                # The next abstract request has no candidate-specific compiled theorem yet.
                next_problem['statement_closure_sha256'] = None
                state['problems'][next_id] = next_problem

    @staticmethod
    def _apply(state, event):
        payload, kind = event['payload'], event['type']
        Registry._advance(state, event['time'])
        if kind == 'catalog':
            Registry._validate_catalog(payload)
            if state['problems'] or event['sequence'] != 1:
                raise RegistryError('catalog must initialize an empty store')
            state.update(problems={row['id']: copy.deepcopy(row) for row in payload['entries']},
                         packages=payload['packages'], excluded=payload['excluded'],
                         reveal_window=payload['reveal_window'], challenge_window=payload['challenge_window'])
            if 'error_policy' in payload:
                state.update(error_policy=copy.deepcopy(payload['error_policy']), gate=payload['gate'])
            if 'submission_policy' in payload:
                state['submission_policy'] = copy.deepcopy(payload['submission_policy'])
        elif kind == 'commit':
            sequence = payload['sequence']
            if sequence != len(state['commits']) + 1:
                raise RegistryError('commit sequence differs')
            state['commits'][str(sequence)] = dict(payload, status='COMMITTED',
                                                   deadline=event['time'] + state['reveal_window'])
        elif kind in ('reveal', 'recheck'):
            if kind == 'recheck' or payload['matched']:
                commit = state['commits'][str(payload['sequence'])]
                verdict = payload['verdict']
                rules = Registry._policy(state)
                if rules is None:
                    if kind == 'recheck':
                        raise RegistryError('legacy recheck unavailable')
                    if verdict['verdict'] == 'ERROR':
                        commit['last_check_error'] = dict(verdict=verdict, receipt=event['hash'])
                    else:
                        commit.update(verdict=verdict, submission=payload['submission'], receipt=event['hash'],
                                      status='VALID-REVEAL' if verdict['verdict'] == 'PASS' else 'REJECTED')
                else:
                    if kind == 'recheck' and commit['status'] not in ('CHECKING', 'QUARANTINED'):
                        raise RegistryError('recheck requires unresolved reveal')
                    prior = commit['status']
                    if kind == 'reveal':
                        commit.update(submission=payload['submission'], salt=payload['salt'],
                                      initial_gate=payload['gate'], checking_since=event['time'])
                    commit.update(verdict=copy.deepcopy(verdict), receipt=event['hash'], deadline=None,
                                  gate=payload['gate'])
                    if verdict['verdict'] == 'ERROR':
                        attempt = copy.deepcopy(payload.get('attempt') or verdict.get('attempt'))
                        if not isinstance(attempt, dict) or attempt.get('class') not in ('T','D-node','D-input','I','S'):
                            raise RegistryError('invalid logged attempt')
                        commit.setdefault('attempts', []).append(attempt)
                        if attempt['class'] == 'I':
                            commit['suspected_input_attempts'] = commit.get('suspected_input_attempts', 0)+1
                    if verdict['verdict'] == 'PASS':
                        commit['status'] = 'VALID-REVEAL'
                    elif Registry._terminal(verdict, rules):
                        commit['status'] = 'REJECTED'
                    else:
                        commit['status'] = 'CHECKING'
                        trigger = E.quarantine_trigger([row for row in commit['attempts'] if row.get('gate') == payload['gate']], rules=rules)
                        if prior == 'QUARANTINED' or trigger:
                            commit['status'] = 'QUARANTINED'
                            if trigger:
                                commit['quarantine'] = trigger
                            if kind == 'recheck' and payload['gate'] != commit['initial_gate']:
                                commit['fixed_gate_unresolved'] = payload['gate']
        elif kind == 'void':
            commit = state['commits'].get(str(payload['sequence']))
            if (not commit or commit['status'] != 'QUARANTINED'
                    or not commit.get('fixed_gate_unresolved') or payload['gate'] != commit['fixed_gate_unresolved']):
                raise RegistryError('void requires unresolved fixed-gate recheck')
            commit.update(status='VOID', cause='void', receipt=event['hash'], deadline=None)
        elif kind != 'advance':
            raise RegistryError('unknown event type')
        Registry._settle(state)
        state['last_event'] = event['hash']

    def _load(self):
        events = self._events()
        state = self._empty_state()
        for event in events:
            self._apply(state, event)
        return events, state

    def status(self):
        """Canonical private state, including recorded failures and duplicate outcomes."""
        with self._locked():
            state = self._load()[1]
            self._alerts(state)
            return copy.deepcopy(state)

    def list(self):
        """Only public problem fields, sorted by stable problem identity."""
        state = self.status()
        return [{key: value for key, value in state['problems'][ident].items() if key != 'statement'}
                for ident in sorted(state['problems'])]

    def show(self, problem_id):
        """Read one public entry; no private object or proof bytes."""
        for row in self.list():
            if row['id'] == problem_id:
                return row
        raise RegistryError('unknown public problem')

    def commit(self, problem_id, solver_id, commit_sha256, logical_time):
        """Record an opaque solver's commitment and return its monotonically assigned sequence."""
        commitment(problem_id, solver_id, commit_sha256, 'validation')
        with self._locked():
            events, state = self._load()
            self._advance(state, logical_time)
            problem = state['problems'].get(problem_id)
            if not problem or problem['status'] != 'OPEN' or not problem['submittable']:
                raise RegistryError('problem is not open and submittable')
            sequence = len(state['commits']) + 1
            payload = dict(sequence=sequence, problem=problem_id, solver=solver_id, commitment=commit_sha256)
            event = self._append('commit', payload, logical_time, events)
            return dict(sequence=sequence, receipt=event['hash'])

    def _check(self, state, problem_id, manifest, verifier=None):
        problem = state['problems'][problem_id]
        with self._materialize(state['packages'][problem['root_id']]) as package_dir:
            with self._materialize(manifest) as submission_dir:
                checks = self.root / 'checks'
                checks.mkdir(exist_ok=True)
                work_dir = tempfile.mkdtemp(prefix='check-', dir=checks)
                if work_dir:
                    metadata_path = Path(package_dir) / 'problem.json'
                    metadata = read_json(metadata_path)
                    if problem['kind'] == 'R' and problem['rung']:
                        metadata['record'] = copy.deepcopy(problem['record'])
                        metadata_path.write_bytes(canonical(metadata) + b'\n')
                    selected = verifier if verifier is not None else self.verifier
                    if selected is None:
                        raise RegistryError('production checker configuration required')
                    from . import check as C
                    with C.submission_policy(state.get('submission_policy')):
                        try:
                            verdict = selected(package_dir, submission_dir, work_dir)
                        except Exception:
                            verdict = dict(verdict='ERROR', reason='checker-internal-error')
                    if not isinstance(verdict, dict) or verdict.get('verdict') not in (
                            'PASS', 'INVALID', 'FAIL', 'ERROR', 'REJECTED'):
                        verdict = dict(verdict='ERROR',reason='checker-worker-error')
                    if problem['kind'] == 'R' and verdict['verdict'] == 'PASS' and not verdict.get('record'):
                        verdict = dict(verdict='ERROR',reason='checker-worker-error')
                    if 'process_receipts' in verdict:
                        (Path(work_dir) / 'process-receipts.json').write_bytes(canonical(dict(
                            process_receipts=verdict['process_receipts'])) + b'\n')
                    self._cleanup_check(Path(work_dir), verdict)
                    return {key: value for key, value in verdict.items()
                            if key in ('verdict', 'reason', 'assurance', 'record', 'artifact', 'attempt')}

    @staticmethod
    def _cleanup_check(root, verdict):
        """Keep bounded diagnostics; remove disposable worker payloads after the attempt."""
        from .bounded_capture import RawSink
        preserve = verdict.get('reason') == 'export-integrity'
        records = []
        budget = 64*1024*1024
        for parent, dirs, files in os.walk(root, topdown=False, followlinks=False):
            for name in files:
                path = Path(parent)/name
                if path.is_symlink():
                    path.unlink()
                    continue
                if preserve and path.suffix == '.ndjson':
                    continue
                diagnostic = path.suffix in ('.json','.jsonl','.log','.stdout','.stderr') or name == 'compile.stderr'
                if diagnostic and budget > 0 and len(records) < 256:
                    sink = RawSink(head=min(512*1024,budget),tail=min(512*1024,max(0,budget-512*1024)),guard=None)
                    with path.open('rb') as stream:
                        for block in iter(lambda:stream.read(65536),b''):
                            sink.feed(block)
                    records.append(dict(path=str(path.relative_to(root)), **sink.receipt()))
                    retained = sink.retained
                    if sink.bytes != len(retained):
                        with path.open('wb') as stream:
                            stream.write(retained)
                    budget -= len(retained)
                else:
                    path.unlink()
            for name in dirs:
                path = Path(parent)/name
                if path.is_symlink():
                    path.unlink()
                elif not any(path.iterdir()):
                    path.rmdir()
        (root/'artifact-digests.json').write_bytes(canonical(records)+b'\n')

    def _history(self, sequence):
        path = self.root / 'attempts.jsonl'
        if not path.exists():
            return []
        starts = {}
        for row in (json.loads(line) for line in path.read_text().splitlines()):
            if row['sequence'] != sequence:
                continue
            if row['event'] == 'start':
                starts[row['index']] = row
            elif row['index'] in starts:
                starts[row['index']]['finished_time'] = row['time']
                starts[row['index']].update(row.get('context', {}))
        return list(starts.values())

    def _attempt_record(self, row):
        with (self.root / 'attempts.jsonl').open('ab') as stream:
            stream.write(canonical(row)+b'\n')
            stream.flush()
            os.fsync(stream.fileno())

    @staticmethod
    def _healthy(context):
        canary = context.get('canary')
        return (context.get('profile') == 'production' and
                (canary is True or isinstance(canary, dict) and canary.get('before') == 'PASS' and canary.get('after') == 'PASS'))

    def _context(self, verifier, supplied=None):
        config = getattr(verifier, 'config', {})
        context = dict(host=config.get('host_id', 'local'), worker=config.get('worker_id', 'local/worker'),
                       profile='production' if config else 'unknown', canary=config.get('canary'),
                       envelope='escalated' if config.get('wall_s') == 3600 else 'standard')
        context.update(supplied or {})
        return context

    @staticmethod
    def _escalated(context):
        envelope = context.get('envelope')
        return envelope == 'escalated' or isinstance(envelope, dict) and envelope.get('wall_s') == 3600

    @staticmethod
    def _resource_envelope(context, rules):
        envelope = context.get('envelope')
        if isinstance(envelope, dict):
            return {key:envelope.get(key) for key in ('wall_s','rss_mb','memory_max')}
        return rules.get(envelope) if envelope in ('standard','escalated') else None

    def _execute(self, state, sequence, manifest, gate, context=None):
        verifier = self.verifier if gate == self.gate else self.gates.get(gate)
        if verifier is None:
            raise RegistryError('replay-gate-unavailable')
        commit = state['commits'][str(sequence)]
        rules = self._policy(state) or E.RULES
        supplied = context is not None
        context = self._context(verifier, context)
        history = self._history(sequence)
        index = len(history)+1
        start = dict(event='start', sequence=sequence, index=index, time=self.clock(), gate=gate,
                     submission_sha256=digest(manifest), **context)
        self._attempt_record(start)
        if hasattr(verifier, 'config'):
            from .service_check import ProductionVerifier
            config = copy.deepcopy(verifier.config)
            profile = rules['escalated' if self._escalated(context) else 'standard']
            config.update(profile, worker_id=context['worker'], host_id=context['host'],
                          canary=context.get('canary'), attempt_index=index)
            verifier = ProductionVerifier(config, rules=rules)
        verdict = self._check(state, commit['problem'], manifest, verifier)
        if verdict['verdict'] == 'ERROR':
            observed = dict(context)
            observed.update(copy.deepcopy(verdict.get('attempt') or {}))
            mismatch = supplied and (any(observed.get(key) != context.get(key) for key in ('worker','host')) or
                                     self._resource_envelope(observed, rules) != self._resource_envelope(context, rules))
            observed.update(index=index, gate=gate, reason=verdict.get('reason', 'checker-unavailable'))
            observed.setdefault('stage', 'worker')
            observed.setdefault('detail', '')
            observed.setdefault('resource_events', [])
            observed.setdefault('evidence_sha256', digest(verdict))
            observed['class'] = E.classify(observed['reason'], observed['stage'], observed['detail'], rules=rules)
            if mismatch or index > 1 and not self._healthy(observed):
                observed['class'] = 'D-node'
            if E.quarantine_trigger([row for row in commit.get('attempts', []) if row.get('gate') == gate]+[observed], rules=rules) and observed['class'] == 'T':
                observed['class'] = 'I'
            verdict['attempt'] = observed
        self._attempt_record(dict(event='finish', sequence=sequence, index=index, time=self.clock(),
                                  verdict=verdict['verdict'], group=verdict.get('attempt', {}).get('class'),
                                  context={key:verdict.get('attempt',context).get(key) for key in ('worker','host','envelope','profile','canary')}))
        return verdict

    def _alerts(self, state):
        if 'error_policy' not in state:
            return
        path = self.root / 'alerts.jsonl'
        prior = {row['id'] for row in (json.loads(line) for line in path.read_text().splitlines())} if path.exists() else set()
        rows = []
        for sequence, commit in state['commits'].items():
            attempts = commit.get('attempts', [])
            notices = []
            if attempts and commit['status'] in ('CHECKING', 'QUARANTINED'):
                notices.append(('checking', 'info'))
            if any(row['class'] == 'D-node' for row in attempts):
                notices.append(('worker-unhealthy', 'page'))
            counted = [row for row in attempts if row['class'] != 'D-node']
            if len(counted) >= 3 or len(counted) >= 2 and counted[0]['class'] == counted[1]['class']:
                notices.append(('ladder-exhausted', 'page'))
            if commit['status'] == 'QUARANTINED':
                notices.append(('quarantine', 'page'))
            if commit['status'] in ('CHECKING','QUARANTINED') and self.clock()-commit['checking_since'] >= 86400:
                notices.append(('checking-age', 'page'))
            for kind, level in notices:
                ident = digest([sequence,kind])
                if ident not in prior:
                    rows.append(dict(id=ident, sequence=int(sequence), kind=kind, level=level, time=self.clock()))
        if rows:
            with path.open('ab') as stream:
                for row in rows:
                    stream.write(canonical(row)+b'\n')

    def _retry(self, commit, sequence, context, rules):
        history = self._history(sequence)
        if not history:
            raise RegistryError('retry-ledger-unavailable')
        counted = [row for row in commit.get('attempts', []) if row['class'] != 'D-node']
        wait = rules['retry']['a2_backoff_s']
        if len(counted) >= 2:
            a3 = len(counted) == 2 and (counted[0]['class'] != counted[1]['class'] or
                  any(not self._healthy(row) for row in counted))
            wait = rules['retry']['a3_backoff_s'] if a3 else rules['retry']['slow_backoff_s']
        if self.clock()-history[-1].get('finished_time',history[-1]['time']) < wait:
            raise RegistryError('retry-backoff')
        if not self._healthy(context) or context.get('worker') == history[0]['worker']:
            raise RegistryError('retry-worker-unhealthy')
        if self._escalated(context):
            if any(self._escalated(row) for row in history):
                raise RegistryError('retry-escalation-used')
            a1 = counted[0] if counted else {}
            allowed = (len(counted) == 1 and (a1.get('reason') == 'resource-unavailable' or a1.get('resource_events'))
                       or len(counted) == 2 and counted[0]['class'] != counted[1]['class'])
            if not allowed or a1.get('class') == 'D-input':
                raise RegistryError('retry-escalation-unavailable')

    def recheck(self, sequence, logical_time, gate=None, *, attempt=None):
        """Operator-only check of the immutable consumed reveal at its original sequence."""
        with self._locked():
            events, state = self._load()
            self._advance(state, logical_time)
            rules = self._policy(state)
            commit = state['commits'].get(str(sequence))
            if rules is None or not commit or commit['status'] not in ('CHECKING','QUARANTINED'):
                raise RegistryError('recheck requires unresolved reveal')
            gate = gate or commit['gate']
            selected = self.verifier if gate == self.gate else self.gates.get(gate)
            if not isinstance(gate,str) or not re.fullmatch(r'[0-9a-f]{64}',gate) or selected is None:
                raise RegistryError('replay-gate-unavailable')
            context = self._context(selected, attempt)
            self._retry(commit, sequence, context, rules)
            verdict = self._execute(state, sequence, commit['submission'], gate, context)
            payload = dict(sequence=sequence, verdict=verdict, gate=gate)
            if 'attempt' in verdict:
                payload['attempt'] = verdict['attempt']
            event = self._append('recheck', payload, logical_time, events)
            self._apply(state, event)
            self._alerts(state)
            return dict(outcome=state['commits'][str(sequence)]['status'], reason=verdict.get('reason'), receipt=event['hash'])

    def void(self, sequence, logical_time):
        """Operator-only release after a fixed gate still leaves a quarantine unresolved."""
        with self._locked():
            events, state = self._load()
            self._advance(state, logical_time)
            commit = state['commits'].get(str(sequence))
            if not commit or commit['status'] != 'QUARANTINED' or not commit.get('fixed_gate_unresolved'):
                raise RegistryError('void requires unresolved fixed-gate recheck')
            payload = dict(sequence=sequence, gate=commit['fixed_gate_unresolved'], cause='void')
            event = self._append('void', payload, logical_time, events)
            self._apply(state, event)
            return dict(outcome='VOID', receipt=event['hash'])

    def reveal(self, sequence, problem_id, solver_id, submission_dir, salt, logical_time):
        """Snapshot and recheck a bound reveal; wrong bindings never consume the intended commit."""
        with self._locked():
            events, state = self._load()
            self._advance(state, logical_time)
            manifest = self._capture(submission_dir)
            value = commitment(problem_id, solver_id, digest(manifest), salt)
            commit = state['commits'].get(str(sequence))
            matched = bool(commit and commit['problem'] == problem_id and commit['solver'] == solver_id
                           and commit['commitment'] == value and commit['status'] == 'COMMITTED')
            if matched:
                if 'error_policy' in state:
                    verdict = self._execute(state, sequence, manifest, state['gate'])
                else:
                    verdict = self._check(state, problem_id, manifest)
            else:
                reason = self._unmatched(commit, 'error_policy' in state)
                verdict = dict(verdict='REJECTED', reason=reason)
            payload = dict(sequence=sequence, problem=problem_id, solver=solver_id, salt=salt,
                           submission=manifest, matched=matched, verdict=verdict)
            if matched and 'error_policy' in state:
                payload['gate'] = state['gate']
                if 'attempt' in verdict:
                    payload['attempt'] = verdict['attempt']
            event = self._append('reveal', payload, logical_time, events)
            self._apply(state, event)
            status = state['commits'][str(sequence)]['status'] if matched else 'REJECTED'
            self._alerts(state)
            outcome = 'ERROR' if matched and verdict['verdict'] == 'ERROR' and 'error_policy' not in state else status
            return dict(outcome='PENDING' if outcome == 'VALID-REVEAL' else outcome,
                        reason=verdict.get('reason'), receipt=event['hash'])

    @staticmethod
    def _unmatched(commit, current):
        if commit and commit['status'] == 'EXPIRED':
            return 'expired-window'
        if current and commit and ('submission' in commit or commit['status'] in ('CHECKING','QUARANTINED','VOID')):
            return 'already-revealed'
        return 'commitment-mismatch'

    def advance(self, logical_time):
        """Explicitly expire commitments/finalize supported S windows without hidden wall-clock reads."""
        with self._locked():
            events, state = self._load()
            self._advance(state, logical_time)
            event = self._append('advance', {}, logical_time, events)
            self._apply(state, event)
            self._alerts(state)
            return copy.deepcopy(state)

    def replay(self, directory: str, verifier=None, *, gates=None):
        """Rebuild an empty second store, revalidate packages and independently recheck every matched reveal."""
        target = Path(directory).resolve()
        if target == self.root or target.exists():
            raise RegistryError('replay requires a new empty destination')
        with self._locked():
            source_events, source_state = self._load()
            replica = Registry(str(target), verifier if verifier is not None else self.verifier, gate=self.gate, gates=gates)
            target.mkdir(parents=True)
            (target / 'objects').mkdir()
            state = self._empty_state()
            output_events = []
            for event in source_events:
                payload = event['payload']
                manifests = []
                if event['type'] == 'catalog':
                    manifests = list(payload['packages'].values())
                elif event['type'] == 'reveal':
                    manifests = [payload['submission']]
                for manifest in manifests:
                    for row in manifest:
                        if not re.fullmatch(r'[0-9a-f]{64}', row['sha256']):
                            raise RegistryError('invalid object digest')
                        source = self.root / 'objects' / row['sha256']
                        dest = target / 'objects' / row['sha256']
                        if source.is_symlink() or not source.is_file():
                            raise RegistryError('source content object unavailable')
                        data = source.read_bytes()
                        if hashlib.sha256(data).hexdigest() != row['sha256'] or len(data) != row['size']:
                            raise RegistryError('source content object corrupted')
                        if not dest.exists():
                            dest.write_bytes(data)
                    with replica._materialize(manifest):
                        pass
                self._advance(state, event['time'])
                if event['type'] == 'catalog':
                    if set(payload['packages']) != {entry['id'] for entry in payload['entries']}:
                        raise RegistryError('replay catalog/package identities differ')
                    for entry in payload['entries']:
                        with replica._materialize(payload['packages'][entry['id']]) as bundle:
                            if catalog_entry(read_json(Path(bundle) / 'problem.json'), bundle) != entry:
                                raise RegistryError('replay catalog qualification differs')
                elif event['type'] == 'commit':
                    entry = state['problems'].get(payload['problem'])
                    commitment(payload['problem'], payload['solver'], payload['commitment'], 'validation')
                    if not entry or not entry['submittable'] or entry['status'] != 'OPEN':
                        raise RegistryError('replay commit admission differs')
                elif event['type'] in ('reveal','recheck'):
                    commit = state['commits'].get(str(payload['sequence']))
                    current = 'error_policy' in state
                    if event['type'] == 'reveal':
                        value = commitment(payload['problem'], payload['solver'], digest(payload['submission']), payload['salt'])
                        matched = bool(commit and commit['problem'] == payload['problem']
                                       and commit['solver'] == payload['solver'] and commit['commitment'] == value
                                       and commit['status'] == 'COMMITTED')
                        if matched != payload['matched']:
                            raise RegistryError('replay commitment binding differs')
                    else:
                        matched = bool(commit and commit['status'] in ('CHECKING','QUARANTINED'))
                        if not matched:
                            raise RegistryError('replay recheck admission differs')
                    selected = replica.verifier
                    if matched and current:
                        selected = replica.gates.get(payload['gate'])
                        if selected is None:
                            raise RegistryError('replay-gate-unavailable')
                    logged = payload['verdict']
                    rules = self._policy(state)
                    if matched and logged['verdict'] == 'ERROR' and not self._terminal(logged, rules):
                        verdict = logged
                    elif matched:
                        manifest = payload['submission'] if event['type'] == 'reveal' else commit['submission']
                        problem = payload['problem'] if event['type'] == 'reveal' else commit['problem']
                        verdict = replica._replay_check(state, problem, manifest, selected, logged, rules)
                    else:
                        verdict = dict(verdict='REJECTED', reason=self._unmatched(commit, current))
                    if verdict != logged:
                        raise RegistryError('replayed verdict differs')
                produced = replica._append(event['type'], payload, event['time'], output_events)
                if produced != event:
                    raise RegistryError('replayed event differs')
                self._apply(state, produced)
                output_events.append(produced)
            if canonical(state) != canonical(source_state) or replica.log.read_bytes() != self.log.read_bytes():
                raise RegistryError('replayed final state differs')
            return replica

    def _replay_check(self, state, problem, manifest, verifier, logged, rules):
        rules = rules or E.RULES
        failures = []
        for index in range(3):
            selected = verifier
            if index and hasattr(verifier,'config'):
                from .service_check import ProductionVerifier
                config = copy.deepcopy(verifier.config)
                context = self._context(verifier)
                if not self._healthy(context):
                    raise RegistryError('replay-unavailable')
                config.update({key:value for key,value in E.envelope(index+1,failures[0],rules=rules).items()
                               if key in ('wall_s','rss_mb','memory_max')})
                if index == 2 and failures[1].get('envelope') == 'escalated':
                    config.update(rules['standard'])
                config['worker_id'] = context['worker']+'/replay-'+str(index+1)
                selected = ProductionVerifier(config, rules=rules)
            verdict = self._check(state, problem, manifest, selected)
            if verdict['verdict'] == 'ERROR':
                attempt = dict(verdict.get('attempt') or {})
                attempt['reason'] = verdict.get('reason', 'checker-unavailable')
                attempt.setdefault('stage', 'worker')
                attempt.setdefault('detail', '')
                attempt['class'] = E.classify(attempt['reason'], attempt['stage'], attempt['detail'], rules=rules)
                verdict['attempt'] = attempt
            if verdict['verdict'] != 'ERROR' or self._terminal(verdict, rules):
                if logged['verdict'] == 'ERROR' and verdict['verdict'] == 'ERROR':
                    if verdict.get('reason') == logged.get('reason'):
                        return logged
                return verdict
            failure = dict(verdict['attempt'])
            failure.setdefault('reason', verdict.get('reason'))
            failures.append(failure)
            if index == 1 and (failures[0]['class'] == failures[1]['class'] and
                    all(row.get('canary') is not False for row in failures)):
                break
            if index < 2:
                time.sleep(rules['retry']['a2_backoff_s' if index == 0 else 'a3_backoff_s'])
        raise RegistryError('replay-unavailable')

    def export_public(self, directory: str):
        """New-directory-only allowlisted request/model export; never export log, evidence or submissions."""
        output = Path(directory).resolve()
        if output.exists() or output == self.root or self.root in output.parents:
            raise RegistryError('public export requires a separate new destination')
        with self._locked():
            _, state = self._load()
            output.mkdir(parents=True)
            entries = []
            for ident in sorted(state['problems']):
                entry = state['problems'][ident]
                public = {key: value for key, value in entry.items() if key != 'statement'}
                folder = digest(ident)
                dest = output / 'statements' / folder
                dest.mkdir(parents=True)
                filename = 'ProofStatement.lean' if entry['kind'] == 'C' else 'Request.json'
                text = entry['statement'].encode('utf-8')
                if hashlib.sha256(text).hexdigest() != entry['statement_sha256']:
                    raise RegistryError('public statement digest differs')
                (dest / filename).write_bytes(text)
                public['statement_file'] = 'statements/' + folder + '/' + filename
                with self._materialize(state['packages'][entry['root_id']]) as package_dir:
                    problem = read_json(Path(package_dir) / 'problem.json')
                    if entry['kind'] == 'R':
                        files = [problem['reference']['model']]
                    elif entry['kind'] == 'C':
                        files = [row for row in problem['checker']['files'] if row['role'] == 'import']
                    else:
                        accepted = problem.get('specification')
                        files = [dict(file=accepted['file'], sha256=accepted['sha256'])] if accepted else []
                    imports = []
                    for row in files:
                        rel = safe_relative(row.get('path') or row.get('file'))
                        source = Path(package_dir) / rel
                        if P.sha256_file(str(source)) != row['sha256']:
                            raise RegistryError('public import digest differs')
                        target = dest / rel
                        target.parent.mkdir(parents=True, exist_ok=True)
                        FS.copyfile(source, target)
                        imports.append(dict(file='statements/' + folder + '/' + rel, sha256=row['sha256']))
                    public['imports'] = imports
                entries.append(public)
            (output / 'catalog.json').write_bytes(canonical(entries) + b'\n')
            return dict(problems=len(entries), catalog_sha256=P.sha256_file(str(output / 'catalog.json')))


def main(argv=None):
    """Local command-line API; operator checker paths are explicit, never read from solver submissions."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--store', required=True)
    parser.add_argument('--config', help='local pinned checker/toolchain configuration JSON')
    parser.add_argument('--time', type=int, default=None, help='monotonic logical UTC seconds; default current UTC')
    commands = parser.add_subparsers(dest='command', required=True)
    catalog = commands.add_parser('catalog')
    actions = catalog.add_subparsers(dest='action', required=True)
    build = actions.add_parser('build')
    build.add_argument('--root', action='append', required=True)
    build.add_argument('--workspace')
    build.add_argument('--reveal-window', type=int, default=3600)
    build.add_argument('--challenge-window', type=int, default=86400)
    actions.add_parser('list')
    show = actions.add_parser('show')
    show.add_argument('problem')
    commit = commands.add_parser('commit')
    commit.add_argument('--problem', required=True)
    commit.add_argument('--solver', required=True)
    binding = commit.add_mutually_exclusive_group(required=True)
    binding.add_argument('--commitment')
    binding.add_argument('--submission')
    commit.add_argument('--salt')
    reveal = commands.add_parser('reveal')
    reveal.add_argument('--sequence', type=int, required=True)
    reveal.add_argument('--problem', required=True)
    reveal.add_argument('--solver', required=True)
    reveal.add_argument('--submission', required=True)
    reveal.add_argument('--salt', required=True)
    recheck = commands.add_parser('recheck')
    recheck.add_argument('--sequence', type=int, required=True)
    recheck.add_argument('--gate')
    void = commands.add_parser('void')
    void.add_argument('--sequence', type=int, required=True)
    commands.add_parser('status')
    commands.add_parser('advance')
    replay = commands.add_parser('replay')
    replay.add_argument('--out', required=True)
    public = commands.add_parser('export-public')
    public.add_argument('--out', required=True)
    args = parser.parse_args(argv)
    try:
        verifier = None
        if args.config:
            from .service_check import ProductionVerifier
            verifier = ProductionVerifier(read_json(Path(args.config)))
        registry = Registry(args.store, verifier, gate=(verifier.config.get('gate_sha256') if verifier else None))
        if args.command == 'recheck' and args.gate and args.gate != registry.gate:
            raise RegistryError('replay-gate-unavailable')
        logical_time = int(time.time()) if args.time is None else args.time
        if args.command == 'catalog':
            if args.action == 'build':
                registry = Registry.create(args.store, catalog_from_roots(args.root, args.workspace),
                                           args.reveal_window, args.challenge_window, verifier)
                result = dict(counts=dict(Counter(row['kind'] for row in registry.list())),
                              frameworks=dict(Counter(row['framework'] for row in registry.list())),
                              excluded=registry.status()['excluded'], service=source_info())
            elif args.action == 'list':
                result = registry.list()
            else:
                result = registry.show(args.problem)
        elif args.command == 'commit':
            value = args.commitment
            if args.submission:
                value = commitment(args.problem, args.solver, submission_digest(args.submission), args.salt)
            result = registry.commit(args.problem, args.solver, value, logical_time)
        elif args.command == 'reveal':
            result = registry.reveal(args.sequence, args.problem, args.solver, args.submission,
                                     args.salt, logical_time)
        elif args.command == 'recheck':
            result = registry.recheck(args.sequence, logical_time, gate=args.gate)
        elif args.command == 'void':
            result = registry.void(args.sequence, logical_time)
        elif args.command == 'status':
            result = registry.status()
        elif args.command == 'advance':
            result = registry.advance(logical_time)
        elif args.command == 'replay':
            replica = registry.replay(args.out)
            result = dict(state_sha256=digest(replica.status()), byte_identical=True)
        else:
            result = registry.export_public(args.out)
        print(canonical(result).decode('utf-8'))
        return 0
    except (RegistryError, OSError, ValueError, KeyError, TypeError) as ex:
        print(canonical(dict(error=type(ex).__name__, message=str(ex))).decode('utf-8'), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
