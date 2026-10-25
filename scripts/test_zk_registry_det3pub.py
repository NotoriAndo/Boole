"""Publication privacy, adopted source identity and terminal library availability."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
import subprocess
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import publication_provenance as P, error_policy as E, closure as D
from zk_registry.service import Registry
from zk_registry.sandbox import Sandbox
import test_zk_registry_det3fix_d as review

ROOT = Path(__file__).resolve().parents[1]


class Publication(unittest.TestCase):
    def test_generator_records_symbolic_roots_and_exact_utf8_lengths(self):
        from zk_registry.build_nanoda_fuel import symbolic_identity
        build='/'+'Users/δοκιμή/build'
        registry='/'+'home/operator/registry'
        identity=dict(source_build_path=build+'/nanoda',build_path=build,nested=dict(registry_path=registry),flags=['--remap-path-prefix',build+'/nanoda=/boole/nanoda','--remap-path-prefix',registry+'=/boole/cargo-registry'])
        public=symbolic_identity(identity,build,registry)
        self.assertEqual(P.path_leaks(json.dumps(public).encode()),[])
        self.assertEqual(public['source_build_path'],'<build-root>/nanoda')
        self.assertEqual(public['build_path'],'<build-root>')
        self.assertEqual(public['nested']['registry_path'],'<cargo-registry>')
        self.assertEqual(public['root_utf8_bytes'],{'<build-root>':len(build.encode()),'<cargo-registry>':len(registry.encode())})
        self.assertEqual(identity['source_build_path'],build+'/nanoda')

    def test_path_guard_distinguishes_personal_homes_from_placeholders_and_urls(self):
        for root in ('Users', 'home'):
            with self.subTest(root=root):
                self.assertEqual(P.path_leaks(('/'+root+'/bob/x').encode()), [1])
            for name in ('example', 'other', 'runner', 'alice', 'operator'):
                with self.subTest(root=root, placeholder=name):
                    self.assertEqual(P.path_leaks(('/'+root+'/'+name+'/x').encode()), [])
        for raw in (b'/' + b'Users/', b'https://github.com/' + b'Users/tmp',
                    ('/'+'Users/|/'+'home/runner|/'+'tmp/').encode()):
            with self.subTest(raw=raw):
                self.assertEqual(P.path_leaks(raw), [])

    def test_every_tracked_publication_path_is_clean(self):
        self.assertEqual(P.scan_tracked(ROOT), {})

    def test_path_guard_distinguishes_user_scratch_from_shared_and_documented_paths(self):
        bad = [('/var', 'folders', 'ab', 'abcdefghijklmnopqrstuvwxyz0123', 'T', 'x'),
               ('.'+'claude', 'projects', 'x'),
               ('/private', 'tmp', 'claude-501', 'x'),
               ('/tmp', 'claude-501', 'x')]
        for parts in bad:
            with self.subTest(rule=parts[1]):
                self.assertEqual(P.path_leaks('/'.join(parts).encode()), [1])
        good = [('.'+'claude', 'commands'), ('.'+'claude', ''),
                ('/private', 'var', 'folders', 'boole-canary-disk.img'),
                ('/private', 'tmp', 'shared-fixture'), ('local'+'-docs', 'evidence')]
        for parts in good:
            with self.subTest(parts=parts):
                self.assertEqual(P.path_leaks('/'.join(parts).encode()), [])

    def test_path_guard_scans_all_tracked_text_except_operator_tasks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            for name in ('docs/example.md', 'native/record.json', 'src/main.rs',
                         'tasks/note.md', 'fixture.bin'):
                path = root/name
                path.parent.mkdir(parents=True, exist_ok=True)
                raw = ('/'+'home/bob/x').encode()
                path.write_bytes((b'\0' if name.endswith('.bin') else b'') + raw)
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            self.assertEqual(P.scan_tracked(root), {
                'docs/example.md': [1], 'native/record.json': [1], 'src/main.rs': [1]})

    def test_all_adopted_provenance_matches_committed_sources_and_pins(self):
        table=json.loads((ROOT/'scripts/zk_registry/data/acceptance_tools_v2.json').read_text())
        self.assertEqual(P.provenance_errors(table, ROOT/'scripts/zk_registry'), [])

    def test_one_stale_binary_digest_is_refused(self):
        table=json.loads((ROOT/'scripts/zk_registry/data/acceptance_tools_v2.json').read_text())
        table['binary_profiles']['Darwin-arm64']['nanoda']['provenance']['binary_sha256']='0'*64
        self.assertIn('Darwin-arm64:nanoda:binary',P.provenance_errors(table, ROOT/'scripts/zk_registry'))

    def test_each_source_and_patch_digest_has_its_own_guard(self):
        table=json.loads((ROOT/'scripts/zk_registry/data/acceptance_tools_v2.json').read_text())
        self.assertEqual(P.provenance_errors(table,ROOT/'scripts/zk_registry'),[])
        for profile in ('Darwin-arm64','Linux-aarch64'):
            for tool,field,key in [('comparator','fuel_sources','CertifiedBase.lean'),('comparator','fuel_sources','Main.lean'),('nanoda','modules','NanodaMain.rs'),('nanoda','sources','src/parser.rs'),('comparator','patch_sha256','comparator-d03acab-own.patch')]:
                stale=copy.deepcopy(table)
                stale['binary_profiles'][profile][tool]['provenance'][field][key]='0'*64
                with self.subTest(profile=profile,field=field,key=key):
                    self.assertTrue(P.provenance_errors(stale,ROOT/'scripts/zk_registry'))


class LibraryAvailability(unittest.TestCase):
    def test_real_registry_consumes_scope_error_without_recheck_or_reclassification(self):
        import test_zk_registry_det2c as fixtures
        from zk_registry.service import RegistryError
        case=fixtures.RegistryPolicy();case.setUp()
        try:
            reason='library-outside-certified-base'
            case.verdict=dict(verdict='ERROR',reason=reason,attempt=dict(case.context(),stage='comparator',reason=reason,**{'class':E.classify(reason)}))
            seq=case.commit()
            self.assertEqual(case.reveal(seq)['outcome'],'REJECTED')
            self.assertEqual(case.calls,1)
            logged=case.registry._events()[-1]['payload']['verdict']
            self.assertEqual((logged['verdict'],logged['reason'],logged['attempt']['class']),('ERROR',reason,'S'))
            with self.assertRaises(RegistryError):
                case.registry.recheck(seq,3,attempt=case.context())
            self.assertEqual(case.calls,1)
            self.assertEqual(case.registry.show(case.ident)['status'],'OPEN')
        finally:case.doCleanups()

    def test_real_version1_store_keeps_its_policy_and_nonterminal_replay(self):
        import test_zk_registry_det2c as fixtures
        from zk_registry import service as S
        case=fixtures.RegistryPolicy();case.setUp()
        try:
            descriptor=dict(version=1,rules_sha256=E.LEGACY_POLICY_SHA256)
            case.registry=S.Registry.create(str(Path(case.tmp.name)/'v1'),case.catalog,verifier=case.verify,error_policy=descriptor)
            case.registry.clock=lambda:case.now
            seq=case.commit()
            self.assertEqual(case.reveal(seq)['outcome'],'CHECKING')
            self.assertEqual(case.registry.status()['error_policy'],descriptor)
            def forbidden(*args):raise AssertionError('nonterminal history must not be reclassified')
            replica=case.registry.replay(str(Path(case.tmp.name)/'v1-replay'),verifier=forbidden)
            self.assertEqual(replica.log.read_bytes(),case.registry.log.read_bytes())
        finally:case.doCleanups()

    def test_library_error_has_typed_terminal_group_without_retries(self):
        reason='library-outside-certified-base'
        self.assertEqual(E.classify(reason,'comparator'),'S')
        verdict=dict(verdict='ERROR',reason=reason,attempt=dict(reason=reason,stage='comparator',**{'class':'S'}))
        self.assertTrue(Registry._terminal(verdict,E.RULES))
        with patch.object(Registry,'_check',return_value=copy.deepcopy(verdict)) as check,patch('time.sleep') as sleep:
            result=Registry._replay_check(Registry.__new__(Registry),{},None,None,None,verdict,E.RULES)
        self.assertEqual(result,verdict);self.assertEqual(check.call_count,1);sleep.assert_not_called()

    def test_frozen_v1_policy_remains_readable_without_migration(self):
        import hashlib
        old=E.LEGACY_POLICY_PATH.read_bytes()
        self.assertEqual(E.load(dict(version=1,rules_sha256=hashlib.sha256(old).hexdigest())),json.loads(old))
        self.assertEqual(json.loads(old)['version'],1)
        self.assertNotIn('library-outside-certified-base',json.loads(old)['terminal_reasons'])

    def test_native_library_marker_survives_consumer_without_becoming_a_node_fault(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);_,left,right,_,tools=review.ComparatorReview().fixture(root)
            events=[dict(version=1,stage='error',reason='library-outside-certified-base'),dict(version=1,stage='final',rc=70)]
            Path(tools.comparator).write_text('#!/usr/bin/python3\nimport os,json\nfrom pathlib import Path\nPath(os.environ["COMPARATOR_STATUS_FILE"]).write_text('+repr(''.join(json.dumps(e)+'\n' for e in events))+')\nraise SystemExit(70)\n')
            row=D.compare_exports(str(left),str(right),'Control.target',tools,Sandbox.fixture(),str(root/'work'),5,[])
            self.assertEqual((row['verdict'],row['reason']),('ERROR','library-outside-certified-base'))


if __name__=='__main__':unittest.main()
