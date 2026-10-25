"""Publication review contracts at registry, metadata and native boundaries."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
import subprocess
import hashlib
import os
import glob
import re
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import error_policy as E, service as S
from zk_registry import build_nanoda_fuel as B
from zk_registry import publication_provenance as P
from zk_registry import service_check as SC
import test_zk_registry_det2c as fixtures

ROOT = Path(__file__).resolve().parents[1]


class BoundPolicy(unittest.TestCase):
    def test_adapter_error_uses_the_bound_policy(self):
        for version in (1, 2):
            descriptor = dict(version=version, rules_sha256=(E.LEGACY_POLICY_SHA256 if version == 1 else E.RULES_SHA256))
            verifier = SC.ProductionVerifier(dict(canary=True))
            verifier.rules = E.load(descriptor)
            with self.subTest(version=version):
                self.assertEqual(verifier.error('library-outside-certified-base')['attempt']['class'],
                                 'T' if version == 1 else 'S')

    def test_replay_reclassifies_present_attempt_classes_with_the_store_policy(self):
        for version in (1, 2):
            with self.subTest(version=version):
                case = fixtures.RegistryPolicy(); case.setUp()
                try:
                    descriptor = dict(version=version, rules_sha256=(E.LEGACY_POLICY_SHA256 if version == 1 else E.RULES_SHA256))
                    case.registry = S.Registry.create(str(Path(case.tmp.name)/'bound-replay'), case.catalog,
                        verifier=case.verify, error_policy=descriptor)
                    case.verdict = fixtures.fixtures.IntakeTests.verify(None, None, None)
                    seq = case.commit(); case.reveal(seq)
                    original = case.registry.log.read_bytes(); calls = []
                    verifier = SC.ProductionVerifier(dict(host_id='A', worker_id='W1', canary=True))
                    def replay_worker(worker, *args):
                        calls.append(worker)
                        if len(calls) <= 2:
                            return worker.error('library-outside-certified-base' if len(calls) == 1 else 'resource-unavailable')
                        return fixtures.fixtures.IntakeTests.verify(*args)
                    with patch.object(SC.ProductionVerifier, '__call__', replay_worker), patch.object(S.time, 'sleep'):
                        with self.assertRaisesRegex(S.RegistryError, 'replay-unavailable' if version == 1 else 'replayed verdict differs'):
                            case.registry.replay(str(Path(case.tmp.name)/'replay'), verifier=verifier)
                    self.assertEqual(len(calls), 2 if version == 1 else 1)
                    self.assertEqual(case.registry.log.read_bytes(), original)
                finally:
                    case.doCleanups()

    def test_documented_mismatch_boundary_matches_reveal_and_recheck(self):
        case = fixtures.RegistryPolicy(); case.setUp()
        try:
            seq = case.commit(); case.verdict = case.error('checker-unavailable', 'other', 'other')
            case.reveal(seq)
            self.assertEqual(case.state(seq)['attempts'][0]['class'], 'T')
            case.now += 60; case.verdict = case.error('checker-unavailable', 'other', 'other', True)
            case.registry.recheck(seq, 3, attempt=case.context('A', 'W2', True))
            self.assertEqual(case.state(seq)['attempts'][1]['class'], 'D-node')
        finally:
            case.doCleanups()
        doc = (ROOT/'docs/zk-problem-registry.md').read_text()
        self.assertIn('rechecks (attempt 2 and later)', doc)
        self.assertIn('first reveal', doc)

    def test_public_build_environment_statement_is_neutral(self):
        doc = (ROOT/'docs/zk-problem-registry.md').read_text()
        self.assertNotIn('Local admission runners', doc)
        self.assertRegex(doc, r"(?s)operator's local tooling sets `RUSTUP_AUTO_INSTALL=0` and `CARGO_NET_OFFLINE=true`\s+for every build; published builders enforce these settings themselves\.")

    def test_first_terminal_S_does_not_emit_checking_alert(self):
        case = fixtures.RegistryPolicy(); case.setUp()
        try:
            case.verdict = dict(verdict='ERROR', reason='library-outside-certified-base')
            sequence = case.commit()
            self.assertEqual(case.reveal(sequence)['outcome'], 'REJECTED')
            path = case.registry.root/'alerts.jsonl'
            rows = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
            self.assertNotIn('checking', [row['kind'] for row in rows])
        finally:
            case.doCleanups()

    def test_v1_new_library_reason_quarantines_and_voids_while_v2_is_terminal(self):
        for version in (1, 2):
            with self.subTest(version=version):
                case = fixtures.RegistryPolicy(); case.setUp()
                try:
                    descriptor = dict(version=version, rules_sha256=(E.LEGACY_POLICY_SHA256 if version == 1 else E.RULES_SHA256))
                    case.registry = S.Registry.create(str(Path(case.tmp.name)/'bound'), case.catalog,
                        verifier=case.verify, error_policy=descriptor)
                    case.registry.clock = lambda: case.now
                    def failure(host, worker):
                        return dict(verdict='ERROR', reason='library-outside-certified-base',
                            attempt=dict(case.context(host, worker), stage='comparator',
                                reason='library-outside-certified-base', **{'class': 'S'}))
                    seq = case.commit(); case.verdict = failure('A', 'W1')
                    self.assertEqual(case.reveal(seq)['outcome'], 'CHECKING' if version == 1 else 'REJECTED')
                    self.assertEqual(case.state(seq)['attempts'][0]['class'], 'T' if version == 1 else 'S')
                    if version == 1:
                        case.now += 60; case.verdict = failure('B', 'W2')
                        case.registry.recheck(seq, 3, attempt=case.context('B', 'W2'))
                        self.assertEqual(case.state(seq)['status'], 'QUARANTINED')
                        gate = 'ab'*32; case.registry.gates[gate] = case.verify
                        case.now += 3600; case.verdict = failure('C', 'W3')
                        case.registry.recheck(seq, 4, gate=gate, attempt=case.context('C', 'W3'))
                        case.registry.void(seq, 5)
                        self.assertEqual(case.state(seq)['status'], 'VOID')
                        replica = case.registry.replay(str(Path(case.tmp.name)/'replica'),
                            verifier=case.verify, gates={gate: case.verify})
                        self.assertEqual(replica.log.read_bytes(), case.registry.log.read_bytes())
                finally:
                    case.doCleanups()


class Sidecar(unittest.TestCase):
    def test_workflow_control_detects_the_real_ignore_rule_being_removed(self):
        self._workflow_control_mutant(remove_ignore=True)

    def test_workflow_control_detects_sidecar_upload(self):
        self._workflow_control_mutant(upload_sidecar=True)

    def _workflow_control_mutant(self, *, remove_ignore=False, upload_sidecar=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); workflow = root/'.github/workflows/det3-counter-identity.yml'
            workflow.parent.mkdir(parents=True)
            text = (ROOT/'.github/workflows/det3-counter-identity.yml').read_text()
            if upload_sidecar:
                text = text.replace('          path: |\n', '          path: |\n            ${{ github.workspace }}/target\n')
            workflow.write_text(text)
            ignore = (ROOT/'.gitignore').read_text()
            if remove_ignore:
                ignore = ignore.replace('/target/\n', '')
            (root/'.gitignore').write_text(ignore)
            (root/'scripts').symlink_to(ROOT/'scripts', target_is_directory=True)
            with patch.dict(globals(), ROOT=root):
                result = unittest.TestResult()
                Sidecar('test_counter_workflow_supplies_an_explicit_git_ignored_sidecar').run(result)
            self.assertEqual(result.errors, [])
            self.assertEqual(len(result.failures), 1, 'the actual workflow control accepted an unsafe input')

    def test_counter_workflow_supplies_an_explicit_git_ignored_sidecar(self):
        workflow = (ROOT/'.github/workflows/det3-counter-identity.yml').read_text()
        step = workflow.split('      - name: Build actual counter drivers', 1)[1]
        step = step.split('        run: |\n', 1)[1].split('\n      - name:', 1)[0]
        script = '\n'.join(line[10:] if line.startswith('          ') else line for line in step.splitlines())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root/'.gitignore').write_bytes((ROOT/'.gitignore').read_bytes())
            module = root/'scripts/zk_registry'; module.mkdir(parents=True)
            binaries = root/'bin'; binaries.mkdir()
            python = binaries/'python3'
            python.write_text('#!/usr/bin/python3\nimport argparse,os,sys\n'
                + 'sys.path.insert(0,' + repr(str(ROOT/'scripts')) + ')\n'
                + 'if "zk_registry.build_nanoda_fuel" in sys.argv:\n'
                + ' from zk_registry import build_nanoda_fuel as builder\n'
                + ' from pathlib import Path\n'
                + ' parser=argparse.ArgumentParser();parser.add_argument("--local-sidecar",required=True)\n'
                + ' args,_=parser.parse_known_args()\n'
                + ' builder.ROOT=Path(os.environ["GITHUB_WORKSPACE"])/"scripts/zk_registry"\n'
                + ' builder.save_local_identity({"source_build_path":"<build-root>"},args.local_sidecar)\n')
            rustup = binaries/'rustup'; rustup.write_text('#!/bin/sh\nprintf "%s\\n" /pinned/bin/cargo\n')
            python.chmod(0o755); rustup.chmod(0o755)
            env = dict(os.environ, PATH=str(binaries)+':/usr/bin:/bin',
                       GITHUB_WORKSPACE=str(root), RUNNER_TEMP=str(root/'temporary'))
            result = subprocess.run(['/bin/bash', '-c', script], cwd=root, env=env,
                                    capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            saved = list((root/'target').rglob('*.json'))
            self.assertEqual(len(saved), 1)
            upload_paths = []
            for step in workflow.split('      - ')[1:]:
                if 'uses: actions/upload-artifact@' not in step:
                    continue
                lines = step.splitlines()
                index = next(i for i, line in enumerate(lines) if line.startswith('          path:'))
                value = lines[index].split(':', 1)[1].strip()
                if value in ('|', '|-', '|+'):
                    for line in lines[index+1:]:
                        if line.strip() and not line.startswith('            '):
                            break
                        if line.strip():
                            upload_paths.append(line.strip())
                else:
                    upload_paths.append(value.strip('\'"'))
            self.assertTrue(upload_paths, 'the workflow upload paths must be inspected')
            variables = {'runner.temp': env['RUNNER_TEMP'], 'github.workspace': str(root)}
            for pattern in upload_paths:
                expanded = re.sub(r'\$\{\{\s*(runner\.temp|github\.workspace)\s*\}\}',
                                  lambda match: variables[match[1]], pattern)
                self.assertNotIn('$', expanded, 'unhandled upload expression')
                if not os.path.isabs(expanded):
                    expanded = str(root/expanded)
                for uploaded in glob.glob(expanded, recursive=True):
                    uploaded = Path(uploaded).resolve()
                    self.assertFalse(saved[0].resolve() == uploaded or uploaded in saved[0].resolve().parents,
                                     'private sidecar lies inside an artifact upload path: ' + pattern)

    def test_unignored_directory_named_like_private_notes_is_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            module = root/'scripts/zk_registry'; module.mkdir(parents=True)
            (root/'.gitignore').write_text('/local-docs/\n')
            destination = root/'scripts/local-docs/x.json'
            with patch.object(B, 'ROOT', module):
                with self.assertRaisesRegex(ValueError, 'ignored'):
                    B.save_local_identity({'source_build_path': '<build-root>'}, destination)
            self.assertFalse(destination.exists())

    def test_explicit_ignored_sidecar_does_not_require_a_folder_name(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            module = root/'scripts/zk_registry'; module.mkdir(parents=True)
            (root/'.gitignore').write_text('/private-evidence/\n')
            destination = root/'private-evidence/x.json'
            with patch.object(B, 'ROOT', module):
                B.save_local_identity({'source_build_path': '<build-root>'}, destination)
            self.assertEqual(json.loads(destination.read_text()), {'source_build_path': '<build-root>'})
            self.assertEqual(destination.stat().st_mode & 0o777, 0o600)


class FrozenHistory(unittest.TestCase):
    def test_exact_two_current_historical_blobs_only(self):
        expected = {
            'native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json': '74b9507932b4eda97c89753f642bac579593b034b3e9eff24bb5b056c09079a6',
            'native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json': '2ff7a3a30513092495a2d8b67555b4e974ef75af47de08acfe8c049063549126'}
        self.assertEqual(getattr(P, 'FROZEN_HISTORICAL', {}), expected)
        for name, digest in expected.items():
            raw = (ROOT/name).read_bytes()
            with self.subTest(name=name):
                self.assertEqual(hashlib.sha256(raw).hexdigest(), digest, 'no stale frozen entry')
                self.assertEqual(P.path_leaks(raw, relative_path=name), [])
                self.assertTrue(P.path_leaks(raw + b'\n', relative_path=name))
                self.assertTrue(P.path_leaks(raw, relative_path='native/copy.json'))


class ProvenanceCompleteness(unittest.TestCase):
    def test_missing_profile_key_guard_is_exercised(self):
        original = P.provenance_errors
        def skip_missing_profile(table, root):
            if {'Darwin-arm64', 'Linux-aarch64'} - table['binary_profiles'].keys():
                return []
            return original(table, root)
        with patch.object(P, 'provenance_errors', skip_missing_profile):
            result = unittest.TestResult()
            ProvenanceCompleteness('test_each_provenance_gap_is_rejected_independently').run(result)
        self.assertEqual(result.errors, [])
        self.assertEqual(len(result.failures), 2, 'missing-profile-key guard was never reached')

    def test_each_provenance_gap_is_rejected_independently(self):
        table = json.loads((ROOT/'scripts/zk_registry/data/acceptance_tools_v2.json').read_text())
        self.assertEqual(P.provenance_errors(table, ROOT/'scripts/zk_registry'), [])
        mutations = [
            ('none-provenance', lambda p: p['comparator'].update(provenance=None)),
            ('missing-fuel-source', lambda p: p['comparator']['provenance']['fuel_sources'].pop('CertifiedBase.lean')),
            ('extra-fuel-source', lambda p: p['comparator']['provenance']['fuel_sources'].update(extra='0'*64)),
            ('patch-order', lambda p: p['comparator']['provenance']['patch_order'].reverse()),
            ('extra-patch', lambda p: p['comparator']['provenance']['patch_sha256'].update(extra='0'*64)),
            ('missing-nanoda-source', lambda p: p['nanoda']['provenance']['sources'].pop('src/parser.rs')),
            ('extra-module', lambda p: p['nanoda']['provenance']['modules'].update(extra='0'*64)),
            ('cargo-toml', lambda p: p['nanoda']['provenance'].update(cargo_toml_sha256='0'*64)),
            ('cargo-lock', lambda p: p['nanoda']['provenance'].update(cargo_lock_sha256='0'*64)),
            ('patched-tests', lambda p: p['nanoda']['provenance']['sources'].pop('src/tests/natlit.rs', None))]
        for profile in ('Darwin-arm64', 'Linux-aarch64'):
            bad = copy.deepcopy(table); bad['binary_profiles'].pop(profile)
            with self.subTest(profile=profile, gap='missing-profile-key'):
                self.assertTrue(P.provenance_errors(bad, ROOT/'scripts/zk_registry'))
            for name, mutate in mutations:
                bad = copy.deepcopy(table); mutate(bad['binary_profiles'][profile])
                with self.subTest(profile=profile, gap=name):
                    self.assertTrue(P.provenance_errors(bad, ROOT/'scripts/zk_registry'))


if __name__ == '__main__':
    unittest.main()
