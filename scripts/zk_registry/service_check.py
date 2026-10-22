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
import re
import socket
from dataclasses import replace
from .safe_fs import Path
import sys

from zk_registry import safe_fs as FS

from . import correctness_gnark as CG
from . import correctness_runtime as CR
from . import lean_runner as L
from . import error_policy as E
from . import bounded_capture as BC
from . import package as P
from . import closure as D
from .sandbox import Sandbox, SandboxUnavailable, delegated_cgroup, validate_memory_profiles
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
        wall=config.get('wall_s',1800)
        if type(wall) is not int or wall not in (1800,3600):raise RegistryError('checker wall must be 1800 or 3600 seconds')
        self.config['wall_s']=wall
        self.host_id=config.get('host_id',socket.gethostname())
        self.worker_id=config.get('worker_id',str(self.host_id)+'/worker')
        if any(not isinstance(value,str) or not value for value in (self.host_id,self.worker_id)):
            raise RegistryError('checker worker and host ids must be nonempty')
        self.config.update(host_id=self.host_id,worker_id=self.worker_id)
        memory=config.get('memory_max',11534336000)
        if type(memory) is not int or memory not in (11534336000,19966445568):
            raise RegistryError('checker memory must use a production retry envelope')
        descriptor=json.loads((Path(__file__).parent/'data/acceptance_tools_v2.json').read_text())
        validate_memory_profiles(descriptor,memory)
        self.config['memory_max']=memory

    def error(self,reason,detail='',stage='worker',result=None,profile=None):
        receipt=(result.process_receipt or {}) if result else {}
        observation=(receipt.get('boundary') or {}).get('observation') or {}
        resources=[key for key in ('oom_kill_events','pids_max_events') if observation.get(key)]
        if result and result.timeout:resources.append('wall')
        if result and result.memkill:resources.append('rss')
        if not detail and result:detail=result.killed_by or ('wall' if result.timeout else 'rss-guard' if result.memkill else '')
        evidence=dict(streams=result.streams if result else {},process_receipt=receipt)
        attempt=dict(index=self.config.get('attempt_index',1),worker=self.worker_id,host=self.host_id,
            envelope={key:self.config[key] for key in ('wall_s','rss_mb','memory_max')},stage=stage,
            reason=reason,detail=detail,canary=self.config.get('canary'),resource_events=resources,
            evidence_sha256=digest(evidence),profile=profile or ((result.boundary or {}).get('assurance') if result else None),
            **{'class':E.classify(reason,stage,detail)})
        return dict(verdict='ERROR',reason=reason,attempt=attempt)

    def __call__(self, package_dir, submission_dir, work_dir):
        """Delegate the trusted parent before launching its separately preflighted checker worker."""
        try:
            delegated_cgroup()
        except (SandboxUnavailable, OSError, ValueError):
            return self.error('sandbox-unavailable')
        config_path = Path(work_dir) / 'checker-config.json'
        config_path.write_bytes(canonical(self.config) + b'\n')
        task_env = {
            'PATH': self.config.get('native_path', L.SYSTEM_PATH),
            'PYTHONPATH': str(Path(__file__).resolve().parent.parent),
            'PYTHONDONTWRITEBYTECODE': '1',
            'HOME': str(Path(work_dir) / 'home'),
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
        Path(task_env['HOME']).mkdir()
        run = CR.runner(str(Path(work_dir) / 'worker-processes'), self.config['wall_s'])
        result = run(argv, task_env, work_dir, self.config['wall_s'], self.config['rss_mb'])
        (Path(work_dir) / 'worker.stdout').write_text(result.out)
        (Path(work_dir) / 'worker.stderr').write_text(result.stderr)
        reason=D.resource_reason(result)
        if reason:
            if reason=='output-cap' and not result.boundary:reason='resource-unavailable'
            return self.error(reason,result=result)
        if result.rc not in (0,1,2,3):return self.error('checker-worker-error',result=result)
        path=Path(work_dir)/'verdict.json'
        try:
            if path.stat().st_size>BC.caps()['verdict_bytes']:
                return self.error('checker-worker-error','verdict-oversized',result=result)
            with path.open('rb') as stream:raw=stream.read(BC.caps()['verdict_bytes']+1)
            if len(raw)>BC.caps()['verdict_bytes']:return self.error('checker-worker-error','verdict-oversized',result=result)
            verdict=json.loads(raw)
            if not isinstance(verdict,dict) or {'PASS':0,'FAIL':1,'INVALID':2,'REJECTED':2,'ERROR':3}.get(verdict.get('verdict'))!=result.rc:
                return self.error('checker-worker-error','verdict-exit-mismatch',result=result)
        except (OSError,ValueError,TypeError):return self.error('checker-worker-error','verdict-missing',result=result)
        if verdict.get('verdict')=='ERROR':
            return self.error(verdict.get('reason','checker-unavailable'),verdict.get('detail',''),verdict.get('stage','worker'),result,verdict.get('assurance'))
        if verdict.get('verdict') == 'PASS' and verdict.get('assurance') != 'production':
            return self.error('nonproduction-evidence',result=result)
        return {key: value for key, value in verdict.items()
                if key in ('verdict', 'reason', 'assurance', 'record', 'artifact')}



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


@FS.guard_submission_io
def run(config, package_dir, submission_dir, work_dir, *, sandbox=None, acceptance_tools=None):
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
    try:
        env = L.load_production_env(config['lean_env'])
    except ValueError as ex:
        if str(ex) in ('environment-retired', 'environment-mismatch'):
            return dict(verdict='ERROR', reason=str(ex))
        raise
    env.scratch = str(Path(work_dir) / 'lean')
    if problem.get('env', {}).get('lean') != env.lean_version:
        return dict(verdict='ERROR', reason='environment-retired')
    from . import acceptance as A
    try:
        env.extra['acceptance_root'] = A.read(package_dir, problem, required=True)
    except OSError as ex:
        (Path(work_dir) / 'error.json').write_bytes(canonical(dict(type=type(ex).__name__, message=str(ex)[:4096])) + b'\n')
        return dict(verdict='ERROR', reason='acceptance-unavailable')
    except (ValueError, KeyError, TypeError) as ex:
        return dict(verdict='ERROR', reason=str(ex))
    try:
        root = config.get('acceptance_tools_root')
        if sandbox is None:
            data = json.loads((Path(__file__).parent / 'data/acceptance_tools_v2.json').read_text())
            relative = data.get('sandbox', {}).get('launcher_path')
            binary = str(Path(root) / relative) if root and relative else None
            sandbox = Sandbox.production(binary)
        acceptance_tools = acceptance_tools or D.Tools.load(root or '')
    except SandboxUnavailable:
        return dict(verdict='ERROR', reason='sandbox-unavailable')
    except D.ToolsUnavailable:
        return dict(verdict='ERROR', reason='kernels-unavailable')
    memory=config.get('memory_max',11534336000)
    validate_memory_profiles(json.loads((Path(__file__).parent/'data/acceptance_tools_v2.json').read_text()),memory)
    sandbox=replace(sandbox,memory_max=memory)
    env.extra['acceptance_sandbox'] = sandbox
    FS.set_submission_writers(sandbox.exposed_writers)
    env.extra['acceptance_tools'] = acceptance_tools
    rss = config.get('rss_mb', 12000)
    writable = [str(Path(work_dir) / name) for name in ('native-build', 'native-cache', 'tmp')]
    writable += [str(Path(work_dir) / 'pipeline/candidate' / name) for name in ('compile', 'build', 'extract')]
    for path in writable:
        Path(path).mkdir(parents=True, exist_ok=True)
    readable = [package_dir, submission_dir, work_dir, env.toolchain, *env.lean_path,
                str(Path(__file__).resolve().parent.parent.parent)]
    readable += list(config.get('snapshot_roots', [])) + list(config.get('snapshots', {}).values())
    readable += [value for value in config.get('tools', {}).values() if isinstance(value, str)]
    wall=config.get('wall_s',1800)
    CR.install(str(Path(work_dir) / 'processes'), total_timeout=wall, sandbox=sandbox,
               readable_paths=readable, writable_paths=writable)
    guarded = L.run_process

    def bounded(cmd, env, cwd, timeout, rss_limit_mb=None, watch=None, poll_s=0.25):
        return guarded(cmd, env, cwd, min(wall,timeout*wall/1800), min(rss, rss_limit_mb or rss), watch, poll_s)

    bounded.submission_boundary = sandbox
    bounded.step_reports = guarded.step_reports
    L.run_process = bounded
    L.set_lean_limits(slots=1, rss_mb=rss)
    if schema == P.CORRECTNESS_GNARK_SCHEMA_VERSION:
        report = CG.check_solution(package_dir, proof, env, str(Path(work_dir) / 'core'), timeout=wall)
        verdict = report['verdict']
    elif schema in RATCHET_KINDS:
        snapshot = resolve_snapshot(problem, config)
        framework = RATCHET_KINDS[schema]
        out = str(Path(work_dir) / 'pipeline')
        Path(out).mkdir(exist_ok=True)
        tools = config.get('tools', {}).get(framework)
        filenames = dict(circom='Candidate.circom', noir='Candidate.nr', gnark='Candidate.go',
                         zokrates='Candidate.zok', halo2='Candidate', **{'zkvm-air': 'Candidate'})
        candidate = str(Path(submission_dir) / filenames[framework])
        if not Path(candidate).exists():
            return dict(verdict='REJECTED', reason='missing-candidate')
        build_root = Path(work_dir) / 'native-build'
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
    report['process_receipts'] = guarded.step_reports
    (Path(work_dir) / 'production-report.json').write_bytes(canonical(report) + b'\n')
    proof_report = report.get('checker', report)
    if verdict == 'PASS' and proof_report.get('two_kernels', {}).get('verdict') != 'PASS':
        return dict(verdict='ERROR', reason='closure-unavailable')
    result = dict(verdict=verdict, reason=None if verdict == 'PASS' else {
        'INVALID': 'invalid-proof', 'FAIL': 'proof-failed', 'ERROR': 'checker-unavailable',
        'REJECTED': 'candidate-rejected'}[verdict])
    result['assurance'] = sandbox.mode
    if verdict == 'ERROR':
        result['reason'] = proof_report.get('reason') or next(iter(proof_report.get('error') or []),'checker-unavailable')
        result['stage'] = proof_report.get('stage','checker')
        result['detail'] = proof_report.get('detail','')
        if not isinstance(result['reason'],str) or not re.fullmatch(r'[a-z][a-z0-9_-]*',result['reason']):
            result['detail']=str(result['reason'])
            result['reason']='checker-input-unavailable'
    if schema in RATCHET_KINDS and verdict == 'PASS':
        result['record'] = _ratchet_record(problem, report['candidate'])
        result['artifact'] = ratchet_artifact(problem, report)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('config', 'package', 'submission', 'work'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args(argv)
    try:
        result = run(read_json(Path(args.config)), args.package, args.submission, args.work)
    except (L.ResourceUnavailable, L.BoundaryInvalid, SandboxUnavailable) as ex:
        result = L.boundary_outcome(ex)
    except (OSError, ValueError, KeyError, TypeError, RuntimeError, RT.Reject) as ex:
        (Path(args.work) / 'error.json').write_bytes(canonical(dict(type=type(ex).__name__, message=str(ex)[:4096])) + b'\n')
        result = dict(verdict='ERROR', reason='checker-unavailable')
    except Exception as ex:
        result = dict(verdict='ERROR', reason='checker-internal-error',detail='exception:'+type(ex).__name__)
    (Path(args.work) / 'process-receipts.json').write_bytes(canonical(dict(
        process_receipts=getattr(L.run_process, 'step_reports', []))) + b'\n')
    result = {key: value for key, value in result.items()
              if key in ('verdict', 'reason', 'assurance', 'record', 'artifact', 'stage', 'detail')}
    (Path(args.work) / 'verdict.json').write_bytes(canonical(result) + b'\n')
    return {'PASS':0,'FAIL':1,'INVALID':2,'REJECTED':2,'ERROR':3}[result['verdict']]


if __name__ == '__main__':
    sys.exit(main())
