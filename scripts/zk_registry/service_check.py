"""Explicit production-checker adapter for the no-rewards local registry.

The worker runs with a clean environment. It delegates to existing per-kind final
checks and returns only deterministic verdict/record fields to the event log.
Detailed diagnostics remain in the private store's checker directory.
"""

from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
import subprocess
import sys

from . import correctness_gnark as CG
from . import correctness_runtime as CR
from . import lean_runner as L
from . import package as P
from . import ratchet as RT
from .service import RegistryError, canonical, digest, read_json, safe_relative, RATCHET_KINDS, UNSUPPORTED_SPEC


class ProductionVerifier:
    """One isolated checker process, selected solely by validated registry metadata."""

    def __init__(self, config: dict):
        self.config = copy.deepcopy(config)
        rss = config.get('rss_mb', 12000)
        if type(rss) is not int or rss not in (12000, 20000):
            raise RegistryError('checker guard must be 12000 or production-final 20000 MB')
        self.config['rss_mb'] = rss

    def __call__(self, package_dir, submission_dir, work_dir):
        config_path = Path(work_dir) / 'checker-config.json'
        config_path.write_bytes(canonical(self.config) + b'\n')
        task_env = {
            'PATH': self.config.get('native_path', L.SYSTEM_PATH),
            'PYTHONPATH': str(Path(__file__).resolve().parent.parent),
            'PYTHONDONTWRITEBYTECODE': '1',
            'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_SYSTEM': '/dev/null',
            'GIT_TERMINAL_PROMPT': '0', 'GIT_CONFIG_NOSYSTEM': '1',
            'npm_config_userconfig': '/dev/null',
            'npm_config_cache': str(Path(work_dir) / 'npm-cache'),
            'PIP_CONFIG_FILE': '/dev/null',
            'TMPDIR': str(Path(work_dir) / 'tmp'),
            'XDG_CACHE_HOME': str(Path(work_dir) / 'cache'),
            'LANG': 'C.UTF-8',
        }
        for key in ('CARGO_HOME', 'RUSTUP_HOME'):
            if key in self.config.get('native_environment', {}):
                task_env[key] = self.config['native_environment'][key]
        Path(task_env['TMPDIR']).mkdir()
        argv = [sys.executable, '-m', 'zk_registry.service_check', '--config', str(config_path),
                '--package', package_dir, '--submission', submission_dir, '--work', work_dir]
        result = subprocess.run(argv, env=task_env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (Path(work_dir) / 'worker.stdout').write_text(result.stdout)
        if result.returncode != 0:
            return dict(verdict='ERROR', reason='checker-worker-error')
        return read_json(Path(work_dir) / 'verdict.json')


def resolve_snapshot(problem, config):
    """Resolve independently provisioned snapshot by its package-pinned identity."""
    ident = problem['snapshot']['id']
    explicit = config.get('snapshots', {}).get(ident)
    paths = [Path(explicit)] if explicit else [Path(root) / ident for root in config.get('snapshot_roots', [])]
    found = [path for path in paths if path.is_dir()]
    if not found:
        raise RegistryError('pinned snapshot unavailable')
    return str(found[0].resolve())


def _ratchet_record(problem, candidate):
    """Keep schema-owned cost fields from the pipeline; provenance lives outside the metric record."""
    measured = candidate.get('cost', candidate)
    record = copy.deepcopy(problem['record'])
    for key in record:
        if key in measured:
            record[key] = copy.deepcopy(measured[key])
    if 'rung' in record:
        record['rung'] += 1
    if 'source' in record:
        record['source'] = 'accepted candidate ' + candidate['source_sha256']
    return record


def ratchet_artifact(problem, report):
    """Copy pipeline provenance; older families bind model SHA in their final checker-package manifest."""
    candidate = report['candidate']
    artifact = {key: candidate[key] for key in (
        'source_sha256', 'model_sha256', 'r1cs_sha256', 'ir_content_sha256') if key in candidate}
    if 'model_sha256' not in artifact:
        package = Path(report['checker']['package'])
        generated = read_json(package / 'problem.json')
        imports = [row for row in generated['checker']['files'] if row['role'] == 'import'
                   and row['path'] != problem['reference']['model']['file']]
        if len(imports) != 1:
            raise RegistryError('generated checker must bind one candidate model')
        row = imports[0]
        if P.sha256_file(str(package / safe_relative(row['path']))) != row['sha256']:
            raise RegistryError('generated candidate model digest differs')
        artifact['model_sha256'] = row['sha256']
    return artifact


def run(config, package_dir, submission_dir, work_dir):
    """Execute existing final pipeline/core, never a reimplementation of admissibility or proof checks."""
    problem = read_json(Path(package_dir) / 'problem.json')
    errors = P.validate_problem(problem, package_dir)
    if errors:
        return dict(verdict='ERROR', reason='invalid-package')
    schema = problem['schema_version']
    if schema == P.SPEC_SCHEMA_VERSION:
        return dict(verdict='REJECTED', reason=UNSUPPORTED_SPEC)
    proof = str(Path(submission_dir) / 'Solution.lean')
    if not Path(proof).is_file():
        return dict(verdict='REJECTED', reason='missing-proof')
    env = L.load_env(config['lean_env'])
    env.scratch = str(Path(work_dir) / 'lean')
    rss = config.get('rss_mb', 12000)
    CR.install(str(Path(work_dir) / 'processes'), total_timeout=1800)
    guarded = L.run_process

    def bounded(cmd, env, cwd, timeout, rss_limit_mb=None, watch=None, poll_s=0.25):
        return guarded(cmd, env, cwd, timeout, min(rss, rss_limit_mb or rss), watch, poll_s)

    L.run_process = bounded
    L.set_lean_limits(slots=1, rss_mb=rss)
    if schema == P.CORRECTNESS_GNARK_SCHEMA_VERSION:
        report = CG.check_solution(package_dir, proof, env, str(Path(work_dir) / 'core'), timeout=1800)
        verdict = report['verdict']
    elif schema in RATCHET_KINDS:
        snapshot = resolve_snapshot(problem, config)
        framework = RATCHET_KINDS[schema]
        out = str(Path(work_dir) / 'pipeline')
        Path(out).mkdir()
        tools = config.get('tools', {}).get(framework)
        filenames = dict(circom='Candidate.circom', noir='Candidate.nr', gnark='Candidate.go',
                         zokrates='Candidate.zok', halo2='Candidate', **{'zkvm-air': 'Candidate'})
        candidate = str(Path(submission_dir) / filenames[framework])
        if not Path(candidate).exists():
            return dict(verdict='REJECTED', reason='missing-candidate')
        build_root = Path(config.get('build_root', str(Path(work_dir) / 'native-build')))
        build_root.mkdir(parents=True, exist_ok=True)
        build = str(build_root / digest(problem['package_id']))
        if framework == 'circom':
            compilers = read_json(Path(tools))
            verdict, report = RT.run_check(package_dir, candidate, proof, out, compilers, env, snapshot,
                                           node=config.get('node'))
        elif framework == 'noir':
            from . import ratchet_noir as RN
            verdict, report = RN.run_check(package_dir, candidate, proof, out, tools, env, snapshot)
        elif framework == 'gnark':
            from . import ratchet_gnark as RG
            verdict, report = RG.run_check(package_dir, candidate, proof, out, tools, env, snapshot,
                                           gocache=str(build_root / 'go-cache'))
        elif framework == 'zkvm-air':
            from . import ratchet_air as RA
            verdict, report = RA.run_check(package_dir, candidate, proof, out, build, env, snapshot)
        else:
            from . import ratchet_native as N, ratchet_zokrates as RZ, ratchet_halo2 as RH
            module = RZ if framework == 'zokrates' else RH
            verdict, report = N.run_check(module, package_dir, candidate, proof, out, tools, env, snapshot, build)
    else:
        return dict(verdict='REJECTED', reason='unsupported-kind')
    (Path(work_dir) / 'production-report.json').write_bytes(canonical(report) + b'\n')
    result = dict(verdict=verdict, reason=None if verdict == 'PASS' else {
        'INVALID': 'invalid-proof', 'FAIL': 'proof-failed', 'ERROR': 'checker-unavailable',
        'REJECTED': 'candidate-rejected'}[verdict])
    if schema in RATCHET_KINDS and verdict == 'PASS':
        result['record'] = _ratchet_record(problem, report['candidate'])
        result['artifact'] = ratchet_artifact(problem, report)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('config', 'package', 'submission', 'work'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    try:
        result = run(read_json(Path(args.config)), args.package, args.submission, args.work)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, RT.Reject) as ex:
        (Path(args.work) / 'error.json').write_bytes(canonical(dict(type=type(ex).__name__, message=str(ex))) + b'\n')
        result = dict(verdict='ERROR', reason='checker-unavailable')
    (Path(args.work) / 'verdict.json').write_bytes(canonical(result) + b'\n')
    return 0


if __name__ == '__main__':
    sys.exit(main())
