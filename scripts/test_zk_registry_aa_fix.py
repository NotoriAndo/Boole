"""Review-fix regressions through acceptance-gate execution and caller interfaces."""
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from dataclasses import replace

sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import lean_runner as L
from zk_registry import ratchet as RT, ratchet_gnark as RG, ratchet_air as RA
from zk_registry import ratchet_noir as RN
from zk_registry import ratchet_native as N, ratchet_halo2 as RH, ratchet_zokrates as RZ
from zk_registry.sandbox import Sandbox
import os
import platform
from zk_registry import correctness_runtime as CR
import importlib.util
import struct
import subprocess
import json


def comparator_status_fixture(events=None, rc=0):
    """Trusted fake Comparator output, separate from potentially hostile kernel stdout."""
    if events is None:
        events = [dict(stage=stage, result='accept') for stage in ['nanoda', 'lean', 'quotient']]
    records = [dict(version=1, **event) for event in events]
    records.append(dict(version=1, stage='final', rc=rc))
    return ''.join(json.dumps(record)+'\n' for record in records)


def comparator_status_program(events=None, rc=0):
    return ('import os\nfrom pathlib import Path\n'
            'Path(os.environ["COMPARATOR_STATUS_FILE"]).write_text('
            + repr(comparator_status_fixture(events, rc)) + ')\n')


class NativeBoundaryOutcomes(unittest.TestCase):
    """Every native framework preserves the trusted execution-boundary outcome."""

    def test_frameworks_keep_boundary_error_and_invalid_reasons(self):
        """A compiler's stdout/exit code cannot overwrite the boundary verdict."""
        for family, module in [('circom', RT), ('gnark', RG), ('air', RA), ('zokrates', RZ), ('halo2', RH), ('noir', RN)]:
            for verdict, reason in [('ERROR', 'sandbox-unavailable'), ('ERROR', 'resource-unavailable'),
                                    ('INVALID', 'owned-descendants-remained'), ('INVALID', 'resource-cap')]:
                with self.subTest(family=family, verdict=verdict, reason=reason), tempfile.TemporaryDirectory() as temp:
                    result = L.RunResult(125, 'payload says compile failed', 0, False,
                                         boundary=dict(verdict=verdict, reason=reason))

                    def native(*args, **kwargs):
                        return L.run_submission(['/unused/compiler'], {}, temp, 1)

                    with patch.object(L, 'run_process', return_value=result) as executor:
                        executor.submission_boundary = Sandbox.fixture()
                        if module in (RZ, RH):
                            with patch.object(N, 'run_candidate', side_effect=native):
                                actual, report = N.run_check(module, temp, temp, temp, temp, temp, None)
                        else:
                            with patch.object(module, 'run_candidate', side_effect=native):
                                actual, report = module.run_check(temp, temp, temp, temp, {}, None)
                    self.assertEqual((actual, report['reason']), (verdict, reason))

    def test_host_log_and_replay_symlinks_do_not_touch_canary(self):
        """Host consumers refuse both terminal-file and parent-directory symlinks."""
        from zk_registry import check as C, gates as G
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            canary = root / 'host-canary'
            canary.write_text('must remain unchanged')
            work = root / 'work'
            (work / 'proof').mkdir(parents=True)
            (work / 'proof/compile.log').symlink_to(canary)
            env = L.LeanEnv('/unused', [], 'v4.34.1', {}, '', str(root / 'scratch'))
            problem = dict(checker=dict(theorem_fqn='X.goal', lean_opts=[], files=[]))
            with patch.object(L, 'run_lean', return_value=L.RunResult(1, 'overwrite', 0, False)):
                with self.assertRaises(OSError):
                    C._compile_and_postcheck(env, problem, temp, 'theorem goal : True := by trivial', str(work), 1)
            self.assertEqual(canary.read_text(), 'must remain unchanged')
            (work / 'replay.out').symlink_to(canary)
            with self.assertRaises(OSError):
                G.parse_replay(str(work / 'replay.out'))
            (work / 'linked').symlink_to(root, target_is_directory=True)
            with self.assertRaises(OSError):
                RT.put(str(work / 'linked/host-canary'), 'overwrite')
            self.assertEqual(canary.read_text(), 'must remain unchanged')

    def test_comparator_output_cannot_forge_launcher_failure(self):
        """Only a structured boundary signal, not exported/user text, causes sandbox ERROR."""
        from zk_registry import closure as D
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ('challenge', 'solution'):
                (root / name).write_text('sealed fixture')
            tools = D.Tools('/unused', '/unused', '/unused', '/usr', None)
            result = L.RunResult(0, 'Lean default kernel accepts the solution\nnanoda kernel accepts the solution\n'
                                 'BOOLE_SANDBOX_UNAVAILABLE:spoof\nError while interacting with nanoda kernel\n',
                                 0, False)
            def comparator(command, env, *args, **kwargs):
                Path(env['COMPARATOR_STATUS_FILE']).write_text(comparator_status_fixture())
                return result
            with patch.object(CR, 'runner', return_value=comparator):
                actual = D.compare_exports(str(root / 'challenge'), str(root / 'solution'), 'X.goal', tools,
                                           Sandbox.fixture(), str(root / 'compare'), 1, [])
            self.assertEqual(actual['verdict'], 'PASS', actual)

    def test_axiom_binding_keeps_boundary_reason(self):
        """An unavailable binding probe keeps its precise trusted boundary classification."""
        from zk_registry import axiom_policy as A, gates as G, closure as D
        from zk_registry.sandbox import SandboxUnavailable
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'build').mkdir()
            (root / 'build/X.olean').write_bytes(b'pristine fixture')
            row = dict(module='X', name='X.a', module_olean_sha256=D.digest(root / 'build/X.olean'))
            env = L.LeanEnv('/unused', [], 'v4.34.1', {}, '', str(root / 'scratch'))
            for verdict, reason, error in [('ERROR', 'sandbox-unavailable', A.BindingUnavailable),
                                           ('INVALID', 'owned-descendants-remained', L.BoundaryInvalid)]:
                run = L.RunResult(125, '', 0, False, boundary=dict(verdict=verdict, reason=reason))
                with self.subTest(reason=reason), patch.object(G, 'run_replay', return_value=(run, {})):
                    with self.assertRaisesRegex(error, reason):
                        A.resolve(env, [row], str(root / 'build'), Sandbox.fixture(), 1)

    def test_async_cleanup_is_bounded_and_cannot_override_verdict(self):
        """Cleanup observations are auxiliary even when asynchronous removal cannot finish."""
        source = Path(__file__).parent / 'zk_registry/sandbox_backend/launcher.py'
        spec = importlib.util.spec_from_file_location('trusted_launcher', source)
        launcher = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(launcher)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'cgroup.events').write_text('populated 1\n')
            (root / 'cgroup.kill').write_text('')
            started = __import__('time').monotonic()
            result = launcher.cleanup_step(root, [], timeout=.03)
            self.assertFalse(result['complete'])
            self.assertLess(__import__('time').monotonic() - started, .2)
            self.assertIn('population-timeout', result['errors'])

    def test_ci_tightens_apparmor_before_setup(self):
        """Execute the authored setup block against a sysctl/install boundary fixture."""
        workflow = (Path(__file__).parents[1] / '.github/workflows/acceptance-gate-linux.yml').read_text()
        block = workflow.split('name: Build and require the pinned hardened boundary', 1)[1]
        block = block.split('        run: |\n', 1)[1].split('\n      - name:', 1)[0]
        block = '\n'.join(line[10:] for line in block.splitlines())
        with tempfile.TemporaryDirectory() as temp:
            prefix = ('sudo() { printf "sudo-call:%s\\n" "$*"; }; '
                      'sysctl() { printf "kernel.apparmor_restrict_unprivileged_userns = 1\\n"; }; '
                      'bash() { printf "setup-called\\n"; }; ')
            result = subprocess.run(['/bin/bash', '-c', prefix + block], text=True, capture_output=True,
                                    env=dict(PATH='/usr/bin:/bin', RUNNER_TEMP=temp,
                                             GITHUB_ENV=str(Path(temp) / 'env')), cwd=temp)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('sudo-call:-n sysctl -w kernel.apparmor_restrict_unprivileged_userns=1', result.stdout)
            self.assertLess(result.stdout.index('apparmor_restrict'), result.stdout.index('setup-called'))

    def test_generated_artifact_parsers_refuse_host_symlinks(self):
        """Native model loaders cannot read a host file through a compiler output link."""
        from zk_registry import r1cs as R, halo2_ir as H
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'canary.json').write_text('{}')
            (root / 'output').symlink_to(root / 'canary.json')
            for parse in (R.read_sym, H.load):
                with self.subTest(parser=parse.__name__), self.assertRaises(OSError):
                    parse(str(root / 'output'))

    def test_noir_preserves_boundary_failure_too(self):
        """The additional native nargo/acir route cannot silently drop a receipt."""
        from zk_registry import noir_toolchain as NT
        from zk_registry.sandbox import SandboxUnavailable
        with tempfile.TemporaryDirectory() as temp:
            tools = NT.Toolchain(temp, temp, temp)
            result = L.RunResult(125, '', 0, False, boundary=dict(verdict='ERROR', reason='sandbox-unavailable'))
            with patch.object(L, 'run_process', return_value=result):
                with self.assertRaisesRegex(SandboxUnavailable, 'sandbox-unavailable'):
                    tools.run_nargo('fixture', ['compile'], temp, 1)

    def test_candidate_elaboration_keeps_boundary_reason(self):
        """The shared intermediate statement compiler preserves a trusted containment verdict."""
        with tempfile.TemporaryDirectory() as temp:
            env = L.LeanEnv('/unused', [], 'v4.34.1', {}, '', str(Path(temp) / 'scratch'),
                            dict(acceptance_sandbox=Sandbox.fixture()))
            result = L.RunResult(125, '', 0, False,
                                 boundary=dict(verdict='INVALID', reason='owned-descendants-remained'))
            with patch.object(L, 'compile_module', return_value=(result, [])):
                with self.assertRaisesRegex(L.BoundaryInvalid, 'owned-descendants-remained'):
                    RT.elaborate(env, temp, ['Synthetic.lean'], 'X.goal', [], str(Path(temp) / 'build'), 1)

    def test_tracked_pid_caps_cover_every_measured_stage(self):
        """Production commands use the pinned stage cap with at least fourfold measured headroom."""
        import json
        data = json.loads((Path(__file__).parent / 'zk_registry/data/acceptance_tools_v2.json').read_text())
        policy = data['sandbox']['parallelism']
        self.assertEqual(policy['cpus'], 4)
        for stage, peak in policy['measured_peak_tasks'].items():
            cap = policy['pids_max'][stage]
            self.assertGreaterEqual(cap, 4 * peak, stage)
            self.assertLessEqual(cap, 1024, stage)


class LiveReviewFixes(unittest.TestCase):
    """Production controls require a real runtime and never use an implicit fallback."""

    def setUp(self):
        runtime = os.environ.get('BOOLE_TEST_SANDBOX')
        if platform.system() != 'Linux' or not runtime:
            self.skipTest('opt-in live Linux runtime')
        self.boundary = Sandbox.production(runtime)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)

    def execute(self, text):
        """Run a small submission and retain its production receipt."""
        root = Path(self.temp.name)
        (root / 'payload.py').write_text(text)
        result = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                           readable_paths=['/usr', str(root)], writable_paths=[str(root)])(
            ['/usr/bin/python3', str(root / 'payload.py')], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        return result

    def test_four_cpu_affinity_and_runtime_parallelism(self):
        """Node core count does not change honest submission thread budgets."""
        result = self.execute('import os,json\nprint(json.dumps(dict(cpus=len(os.sched_getaffinity(0)), '
                              'go=os.environ.get("GOMAXPROCS"), cargo=os.environ.get("CARGO_BUILD_JOBS"), '
                              'rayon=os.environ.get("RAYON_NUM_THREADS"))))\n')
        import json
        self.assertEqual(result.boundary['verdict'], 'PASS', result.boundary)
        self.assertEqual(json.loads(result.out), dict(cpus=4, go='4', cargo='4', rayon='4'))

    def test_payload_exit_125_is_data_not_setup_failure(self):
        """An honest payload's reserved-looking exit code does not manufacture an ERROR."""
        result = self.execute('raise SystemExit(125)\n')
        self.assertEqual((result.rc, result.boundary['verdict']), (125, 'PASS'), result.boundary)

    def test_cgroup_move_failure_is_structured(self):
        """An injected pre-exec cgroup failure must not become a payload exit 125."""
        import hashlib
        import json
        import shutil
        root = Path(self.temp.name)
        fixture = root / 'fixture-gate'
        fixture.mkdir()
        source = (Path(__file__).parent / 'zk_registry/sandbox_backend/isolate.c').read_text()
        needle = 'write_control(fds[0], pid);'
        self.assertEqual(source.count(needle), 1)
        (root / 'injected.c').write_text(source.replace(needle,
            'errno = EIO; unavailable("cgroup-move-injected");'))
        build = subprocess.run(['/usr/bin/gcc', '-O2', '-static', str(root / 'injected.c'),
                                '-o', str(fixture / 'gate-isolate')], capture_output=True, text=True)
        self.assertEqual(build.returncode, 0, build.stderr)
        installed = Path(self.boundary.executable).parent
        for name in ('launcher.py', 'comparator-landrun', 'seccomp-' + platform.machine() + '.bpf'):
            shutil.copy2(installed / name, fixture / name)
        data = json.loads((installed / 'bundle.json').read_text())
        data['sandbox']['architectures'][platform.machine()]['isolate_sha256'] = hashlib.sha256(
            (fixture / 'gate-isolate').read_bytes()).hexdigest()
        (fixture / 'bundle.json').write_text(json.dumps(data))
        boundary = replace(self.boundary, executable=str(fixture / 'launcher.py'),
                           authority=hashlib.sha256((fixture / 'bundle.json').read_bytes()).hexdigest())
        (root / 'work').mkdir()
        result = CR.runner(str(root / 'trusted-logs'), 20, sandbox=boundary,
                           readable_paths=['/usr'], writable_paths=[str(root / 'work')])(
            ['/usr/bin/true'], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('ERROR', 'sandbox-unavailable'), result.boundary)
        self.assertTrue(result.boundary['observation']['setup_failed'])

    def test_process_receipts_are_outside_submission_writable_roots(self):
        """Submission code cannot remove or forge the host's copied process receipts."""
        import json
        root = Path(self.temp.name)
        run = CR.runner(str(root / 'processes'), 20, sandbox=self.boundary,
                        readable_paths=['/usr', str(root)], writable_paths=[str(root)])
        result = run(['/usr/bin/python3', '-c', 'import os; print("payload")'],
                     {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        self.assertEqual(result.boundary['verdict'], 'PASS')
        logs = Path(getattr(run, 'log_directory', root / 'processes'))
        self.assertFalse(logs.is_relative_to(root), logs)
        before = json.dumps(result.process_receipt, sort_keys=True)
        self.assertFalse(logs.exists(), 'completed runner leaves no empty owner directory')
        attempted = run(['/usr/bin/python3', '-c', 'from pathlib import Path; '
                        'target=Path(' + repr(str(logs / 'forged.json')) + '); '
                        'target.parent.mkdir(parents=True,exist_ok=True);target.write_text("forged")'],
                        {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        self.assertNotEqual(attempted.rc, 0)
        self.assertFalse((logs / 'forged.json').exists())
        self.assertEqual(json.dumps(result.process_receipt, sort_keys=True), before)
        self.assertFalse(logs.exists())
        self.assertEqual(result.process_receipt['boundary']['verdict'], 'PASS')

    def test_orphan_flood_has_bounded_evidence(self):
        """Many fast orphan exits cannot exhaust the trusted runtime receipt directory."""
        result = self.execute('import os,time\nfor i in range(400):\n p=os.fork()\n if p==0:\n'
                              '  if os.fork()==0: os._exit(0)\n  os._exit(0)\n os.waitpid(p,0)\n'
                              ' time.sleep(.002)\ntime.sleep(.1)\n')
        record = result.boundary
        self.assertEqual((record['verdict'], record['reason']), ('INVALID', 'owned-descendants-remained'))
        self.assertGreater(record['observation']['orphans_reaped'], 256)
        self.assertLessEqual(record['observation'].get('orphan_events_recorded', 100000), 256)
        self.assertLessEqual(record['pid_1_events_bytes'], 40000)
        event_file = Path(record['pid_1_events_path'])
        self.assertFalse(event_file.exists())
        self.assertFalse(Path(str(event_file).replace('.events.ndjson', '.json')).exists())

    def test_break_only_comparator_stage_launcher(self):
        """An intact outer boundary does not hide a missing inner Comparator launcher."""
        from zk_registry import closure as D
        root = Path(self.temp.name)
        for name in ('Challenge.ndjson', 'Solution.ndjson'):
            (root / name).write_text('unexecuted sealed control fixture')
        self.assertEqual(self.execute('print("outer-boundary-works")\n').boundary['verdict'], 'PASS')
        tools = D.Tools('/unexecuted', '/unexecuted', '/unexecuted', '/usr', self.boundary.executable)
        actual = D.compare_exports(str(root / 'Challenge.ndjson'), str(root / 'Solution.ndjson'), 'X.goal', tools,
                                   replace(self.boundary, inner_executable=str(root / 'missing-inner')),
                                   str(root / 'comparison'), 5, [])
        self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'sandbox-unavailable'))

    def comparator_control(self, code, inner=None):
        """Run a trusted transport fixture through the real outer production boundary."""
        from zk_registry import closure as D
        root = Path(self.temp.name)
        for name in ('Challenge.ndjson', 'Solution.ndjson'):
            (root / name).write_text('sealed transport fixture, not a proof')
        program = root / 'comparator-fixture'
        program.write_text('#!/usr/bin/python3\n' + comparator_status_program() + code)
        program.chmod(0o755)
        work = root / 'comparison'
        work.mkdir()
        if inner == 'broken':
            fault = work / 'inner-runtime-broken'
            fault.write_text('#!/missing-inner-interpreter\n')
            fault.chmod(0o755)
            boundary = replace(self.boundary, inner_executable=str(fault))
        else:
            boundary = self.boundary
        tools = D.Tools(str(program), '/usr/bin/true', '/usr/bin/true', '/usr', boundary.executable)
        return D.compare_exports(str(root / 'Challenge.ndjson'), str(root / 'Solution.ndjson'), 'X.goal',
                                 tools, boundary, str(work), 10, [])

    def test_comparator_runtime_inner_launch_failure_is_error(self):
        """An existing inner launcher that fails at exec is ERROR, not kernel rejection."""
        actual = self.comparator_control('import os,subprocess\ntry:\n'
            ' subprocess.run([os.environ["COMPARATOR_LANDRUN"],"--","/usr/bin/true"],check=True)\n'
            'except (OSError,subprocess.CalledProcessError):\n raise SystemExit(70)\n', inner='broken')
        self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'sandbox-unavailable'), actual)

    def test_nanoda_launch_transport_signal_is_error(self):
        """A structured kernel launch/transport exit code cannot become INVALID."""
        actual = self.comparator_control('raise SystemExit(70)\n')
        self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'sandbox-unavailable'), actual)

    def test_nanoda_runtime_missing_interpreter_is_error(self):
        """The actual inner wrapper distinguishes a nanoda exec failure from rejection."""
        actual = self.comparator_control('import os,subprocess\nfrom pathlib import Path\n'
            'p=Path("nanoda-broken"); p.write_text("#!/missing-nanoda-interpreter\\n"); p.chmod(0o755)\n'
            'r=subprocess.run([os.environ["COMPARATOR_LANDRUN"],"--ro","/usr","--",str(p.absolute())])\n'
            'raise SystemExit(70 if r.returncode else 0)\n')
        self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'sandbox-unavailable'), actual)
        self.assertEqual(actual['inner_signals'][0]['setup_error'], 'landlock-exec')

    def test_missing_inner_success_marker_fails_closed(self):
        """Kernel-looking output and an empty status file cannot manufacture acceptance."""
        actual = self.comparator_control('print("Lean default kernel accepts the solution")\n'
                                         'print("nanoda kernel accepts the solution")\n')
        self.assertEqual((actual['verdict'], actual['reason']), ('ERROR', 'sandbox-unavailable'), actual)

    def test_nanoda_child_cannot_write_status_directory(self):
        """The kernel child cannot forge its parent's exec attestation through the status path."""
        child = ('import os\nfrom pathlib import Path\ntry:\n'
                 ' Path(os.environ["COMPARATOR_BOUNDARY_STATUS"]).write_text("forged")\n'
                 'except PermissionError:\n print("STATUS-WRITE-DENIED")\n'
                 'else:\n raise SystemExit(9)\n')
        code = ('import os,subprocess\n'
                'r=subprocess.run([os.environ["COMPARATOR_LANDRUN"],"--ro","/usr","--",'
                '"/usr/bin/python3","-c",' + repr(child) + '])\n'
                'if r.returncode: raise SystemExit(2)\n'
                'print("Lean default kernel accepts the solution")\n'
                'print("nanoda kernel accepts the solution")\n')
        actual = self.comparator_control(code)
        self.assertEqual(actual['verdict'], 'PASS', actual)
        self.assertEqual(actual['inner_signals'], [dict(inner_exec='ok', exit=0)])

    def test_nanoda_child_cannot_forge_comparator_semantic_status(self):
        """The new semantic channel is writable by the Comparator, never its kernel child."""
        child = ('import sys\nfrom pathlib import Path\ntry:\n'
                 ' Path(sys.argv[1]).write_text("forged")\n'
                 'except PermissionError:\n print("STATUS-WRITE-DENIED")\n'
                 'else:\n raise SystemExit(9)\n')
        code = ('import os,subprocess\n'
                'r=subprocess.run([os.environ["COMPARATOR_LANDRUN"],"--ro","/usr","--",'
                '"/usr/bin/python3","-c",' + repr(child) + ',os.environ["COMPARATOR_STATUS_FILE"]])\n'
                'if r.returncode: raise SystemExit(2)\n')
        actual = self.comparator_control(code)
        self.assertEqual(actual['verdict'], 'PASS', actual)
        self.assertEqual(actual['inner_signals'], [dict(inner_exec='ok', exit=0)])

    def test_exec_setup_failure_is_structured(self):
        """A submission artifact's missing interpreter is INVALID, not a node outage."""
        root = Path(self.temp.name)
        program = root / 'missing-interpreter'
        program.write_text('#!/no-such-interpreter\n')
        program.chmod(0o755)
        result = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                           readable_paths=['/usr', str(root)], writable_paths=[str(root)])(
            [str(program)], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('INVALID', 'submission-exec-failed'), result.boundary)
        self.assertIn('submitted-shebang-interpreter', result.boundary['detail'])

    def test_trusted_tool_exec_failure_stays_error(self):
        """Exec failure of a read-only trusted tool is still an availability ERROR."""
        root = Path(self.temp.name)
        program = root / 'trusted-missing-interpreter'
        program.write_text('#!/no-such-interpreter\n')
        program.chmod(0o755)
        (root / 'work').mkdir()
        result = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                           readable_paths=['/usr', str(program)], writable_paths=[str(root / 'work')])(
            [str(program)], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('ERROR', 'sandbox-unavailable'), result.boundary)

    def test_submission_artifact_without_exec_permission_is_invalid(self):
        """A bad submission-produced artifact cannot force a retryable node outage."""
        root = Path(self.temp.name)
        program = root / 'non-executable-artifact'
        program.write_text('#!/usr/bin/true\n')
        program.chmod(0o600)
        result = CR.runner(str(root / 'logs'), 20, sandbox=self.boundary,
                           readable_paths=['/usr', str(root)], writable_paths=[str(root)])(
            [str(program)], {'PATH': '/usr/bin:/bin'}, str(root), 20, 256)
        self.assertEqual((result.boundary['verdict'], result.boundary['reason']),
                         ('INVALID', 'submission-exec-failed'), result.boundary)

    def test_three_cpu_node_fails_preflight(self):
        """A node unable to supply the declared four CPUs cannot qualify production."""
        from zk_registry.sandbox import SandboxUnavailable
        available = os.sched_getaffinity(0)
        self.assertGreaterEqual(len(available), 4)
        try:
            os.sched_setaffinity(0, sorted(available)[:3])
            with self.assertRaisesRegex(SandboxUnavailable, 'sandbox-unavailable'):
                Sandbox.production(self.boundary.executable)
        finally:
            os.sched_setaffinity(0, available)

    def test_submission_output_symlinks_leave_host_canary_unchanged(self):
        """Real sandbox code replaces stage files; the host checker fails closed on consumption."""
        from zk_registry import check as C, gates as G
        root = Path(self.temp.name)
        work = root / 'work'
        (work / 'proof').mkdir(parents=True)
        canary = root / 'host-canary'
        canary.write_text('unchanged')
        source = ('import os\nfor name in ("compile.log", "replay.out"):\n'
                  ' os.symlink(' + repr(str(canary)) + ', ' + repr(str(work / 'proof')) + '+"/"+name)\n')
        guard = CR.runner(str(root / 'payload-logs'), 20, sandbox=self.boundary,
                          readable_paths=['/usr', str(root)], writable_paths=[str(work)])
        with patch.object(L, 'run_process', guard):
            result = L.run_submission(['/usr/bin/python3', '-c', source], {'PATH': '/usr/bin:/bin'}, str(work), 20, 256)
        self.assertEqual(result.boundary['verdict'], 'PASS', result.boundary)
        pins = L.production_descriptor()
        env = L.LeanEnv('/unexecuted', [], pins['lean'], pins['packages'], pins['lake_manifest_sha256'],
                        str(root / 'scratch'))
        problem = dict(checker=dict(theorem_fqn='X.goal', lean_opts=[], files=[]))
        # This control injects a stage result, not a provisioned Lean runtime.
        # Admit that fixture past C2 so the real sandbox output is consumed.
        with patch.object(L, 'production_environment_reason', return_value=None), \
                patch.object(L, 'run_lean', return_value=result), \
                self.assertRaisesRegex(L.BoundaryInvalid, '^submission-output-path$'):
            C.compile_in_sandbox(env, problem, str(root), 'theorem goal : True := by trivial',
                                 str(work), 5, self.boundary)
        with self.assertRaises(OSError):
            G.parse_replay(str(work / 'proof/replay.out'))
        self.assertEqual(canary.read_text(), 'unchanged')

    def test_reader_cannot_follow_a_previous_stage_symlink(self):
        """A work-directory symlink must not expand the next stage's readable mount graph."""
        root = Path(self.temp.name)
        (root / 'reader').symlink_to('/etc/passwd')
        from zk_registry.sandbox import SandboxUnavailable
        with self.assertRaises(SandboxUnavailable):
            self.boundary.command(['/usr/bin/true'], {}, [str(root / 'reader')], [str(root)])
        (root / 'next-stage').mkdir()
        with self.assertRaises(SandboxUnavailable):
            self.boundary.command(['/usr/bin/true'], {}, ['/etc', str(root / 'reader')], [str(root / 'next-stage')])

    def test_emitted_seccomp_denies_userns_clone_and_mount_setattr(self):
        """Evaluate the actual installed filter, separate from namespace-layer denials."""
        program = (Path(self.boundary.executable).parent / ('seccomp-' + platform.machine() + '.bpf')).read_bytes()
        arch, clone = (0xc00000b7, 220) if platform.machine() == 'aarch64' else (0xc000003e, 56)

        def evaluate(number, argument=0):
            data = struct.pack('<IIQ6Q', number, arch, 0, argument, 0, 0, 0, 0, 0)
            instructions = list(struct.iter_unpack('<HBBI', program))
            accumulator, index = 0, 0
            for _ in range(len(instructions) + 1):
                code, yes, no, value = instructions[index]
                index += 1
                if code == 0x20:
                    accumulator = struct.unpack_from('<I', data, value)[0]
                elif code == 0x15:
                    index += yes if accumulator == value else no
                elif code == 0x35:
                    index += yes if accumulator >= value else no
                elif code == 0x45:
                    index += yes if accumulator & value else no
                elif code == 0x05:
                    index += value
                elif code == 0x54:
                    accumulator &= value
                elif code == 0x06:
                    return value
                else:
                    raise ValueError('unsupported BPF instruction')
            raise ValueError('unbounded filter')

        self.assertEqual(evaluate(442), 0x50001)
        self.assertEqual(evaluate(clone, 0x10000000), 0x50001)
        self.assertEqual(evaluate(clone, 17), 0x7fff0000)
        self.assertNotEqual(evaluate(435), 0x7fff0000)  # clone3: deny all pointer-supplied flags.


if __name__ == '__main__':
    unittest.main()
