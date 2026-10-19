"""Fail-closed source reconstruction, independent of the expensive Lean build."""
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
import sys
import subprocess
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import measuring_sticks as M


def digest(data):
    return hashlib.sha256(data).hexdigest()


class PublicationIntegrity(unittest.TestCase):
    def test_archive_chained_links_cannot_escape(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();archive=root/'chain.tar';destination=root/'dest';destination.mkdir()
            with tarfile.open(archive,'w') as stream:
                for name,target in [('r/a','.'),('r/a/b/link','../..')]:
                    row=tarfile.TarInfo(name);row.type=tarfile.SYMTYPE;row.linkname=target;stream.addfile(row)
                row=tarfile.TarInfo('r/a/b/link/ESCAPED.txt');row.size=1;stream.addfile(row,io.BytesIO(b'x'))
            with self.assertRaises(M.IntegrityError):
                M.extract_archive(archive,destination,allow_internal_links=True)
            self.assertFalse((root/'ESCAPED.txt').exists())

    def test_public_frozen_identities_are_hard_pinned(self):
        pins = {
            'evm': ('12b01e249765de5cefdda41966123a479d533fcc8a0ff93d39d46ede39cbb83c',
                    '12b01e249765de5cefdda41966123a479d533fcc8a0ff93d39d46ede39cbb83c',
                    '226529f12850817070bcb80474fbf5f5da9b006727f216eff1c424bed5f276ad'),
            'riscv-v2': ('95eac4f0457e522bf59a855e6981adcf9f60efbf239e176e28b3cd3c15ebba94',
                         'c5b8b3b5c02602eb85805996dd1fb8f64a98e521f1c277668f8cdc9da71068f4',
                         '2713d60c047f93d8c5c71aeb63e78f27fa8c0ae17b604fb9db7604ca49ae302b')}
        for name, (source, definition, types) in pins.items():
            _, expected=M.load_descriptor(Path(M.__file__).parent/'data/sticks'/name)
            self.assertEqual((expected['source_digest'], expected['definition_digest'], M.map_digest(expected['types'])),
                             (source, definition, types))
            self.assertNotIn('port_digest',expected)

    def test_unsupported_bootstrap_profile_fails_before_creating_output(self):
        with tempfile.TemporaryDirectory() as temp:
            out=Path(temp)/'base';metadata=Path(M.__file__).parent/'data/sticks/base'
            with patch('platform.system',return_value='Linux'),patch('platform.machine',return_value='x86_64'):
                with self.assertRaisesRegex(M.IntegrityError,'unsupported-toolchain-profile'):
                    M.bootstrap_base(metadata,out,Path(temp)/'cache',None)
            self.assertFalse(out.exists())

    def test_fresh_public_cache_can_omit_a_pinned_empty_search_root(self):
        self.bootstrap_empty_root_case('empty')

    def test_bootstrap_never_fills_a_missing_nonempty_search_root(self):
        self.bootstrap_empty_root_case('missing-nonempty')

    def test_bootstrap_rejects_an_empty_search_root_link(self):
        self.bootstrap_empty_root_case('linked')

    def test_bootstrap_rejects_an_empty_search_root_linked_parent(self):
        self.bootstrap_empty_root_case('linked-parent')

    def bootstrap_empty_root_case(self, case):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();metadata=root/'metadata';metadata.mkdir();destination=root/'base'
            profile='Fixture-arm64';compiler={'lean-v4.34.1/bin/lake':digest(b'compiler')}
            artifact='.lake/packages/mathlib/.lake/build/lib/lean/Core.olean'
            library={artifact:digest(b'cached theorem')}
            expected=dict(packages={'Cli':'a'*40,'mathlib':'b'*40},
                compiler_archives={profile:dict(sha256='0'*64,url='https://example.invalid/compiler',content_sha256='1'*64)},
                toolchain_profiles={profile:dict(count=1,sha256=M.map_digest(compiler))},toolchain_symlinks={profile:{}},
                library_inventories={'.lake/packages/Cli/.lake/build/lib/lean':dict(count=0,sha256='44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a'),
                    '.lake/packages/mathlib/.lake/build/lib/lean':dict(count=1,sha256=M.map_digest(library))})
            (metadata/'expected.json').write_text(json.dumps(expected))
            for name in ['lake-manifest.json','lakefile.toml','lean-toolchain']:(metadata/name).write_text('fixture')
            def extract(archive, target, **kwargs):
                (target/'bin').mkdir();(target/'bin/lake').write_bytes(b'compiler')
            def cache_run(*args):
                if case!='missing-nonempty':
                    target=destination/artifact;target.parent.mkdir(parents=True);target.write_bytes(b'cached theorem')
                if case in ['linked','linked-parent']:
                    outside=root/'outside';outside.mkdir();target=destination/('.lake/packages/Cli/.lake/build/lib/lean' if case=='linked' else '.lake/packages/Cli/.lake/build')
                    target.parent.mkdir(parents=True);target.symlink_to(outside,target_is_directory=True)
            with patch('platform.system',return_value='Fixture'),patch('platform.machine',return_value='arm64'),patch.object(M,'fetch_archive',return_value=root/'archive'),patch.object(M,'extract_archive',side_effect=extract):
                if case=='empty':
                    self.assertEqual(M.bootstrap_base(metadata,destination,root/'cache',cache_run),destination)
                    self.assertTrue((destination/'.lake/packages/Cli/.lake/build/lib/lean').is_dir())
                    # A default bootstrap resume must perform the same empty-root preparation.
                    (destination/'.lake/packages/Cli/.lake/build/lib/lean').rmdir()
                    self.assertEqual(M.bootstrap_base(metadata,destination,root/'cache',cache_run),destination)
                else:
                    with self.assertRaises((M.IntegrityError,OSError)):
                        M.bootstrap_base(metadata,destination,root/'cache',cache_run)

    def test_bootstrap_isolates_cache_tools_without_weakening_import_pins(self):
        from zk_registry import safe_fs
        for case in ['clean', 'mutated-tool', 'partial-tool', 'unknown-import', 'collision', 'linked-parent']:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temp:
                base=Path(temp).resolve(); root='.lake/packages/mathlib/.lake/build/lib/lean'
                core={root+'/Mathlib.olean':digest(b'core')}
                full={**core,root+'/Cache/Main.olean':digest(b'tool'),root+'/Cache/IO.olean':digest(b'io')}
                pin=lambda rows:dict(count=len(rows),sha256=M.map_digest(rows))
                expected=dict(packages={'mathlib':'a'*40},library_inventories={root:pin(full)},
                    bootstrap_tool_inventory=dict(root=root,subdirectory='Cache',without_tools=pin(core)),
                    toolchain_profiles={'fixture':pin({})},toolchain_symlinks={'fixture':{}})
                (base/'lean-v4.34.1').mkdir()
                for name in full:
                    p=base/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b'core' if name.endswith('Mathlib.olean') else b'tool' if name.endswith('Main.olean') else b'io')
                if case=='mutated-tool':(base/root/'Cache/Main.olean').write_bytes(b'path-dependent tool')
                if case=='partial-tool':(base/root/'Cache/IO.olean').unlink()
                if case=='unknown-import':(base/root/'Extra.olean').write_bytes(b'extra')
                if case=='collision':(base/'.bootstrap-tools/mathlib/Cache').mkdir(parents=True)
                if case=='linked-parent':
                    outside=base/'outside';outside.mkdir();(base/'.bootstrap-tools').symlink_to(outside,target_is_directory=True)
                if case in ['mutated-tool','partial-tool','unknown-import']:
                    with self.assertRaises(M.IntegrityError):M.verify_import_inventory(base,expected,'fixture')
                if case in ['collision','linked-parent']:
                    with self.assertRaises((M.IntegrityError,OSError)):M.prepare_bootstrap_libraries(base,expected)
                    self.assertTrue((base/root/'Cache').is_dir())
                    continue
                M.prepare_bootstrap_libraries(base,expected)
                self.assertFalse((base/root/'Cache').exists())
                self.assertTrue((base/'.bootstrap-tools/mathlib/Cache/Main.olean').is_file())
                if case=='unknown-import':
                    with self.assertRaises(M.IntegrityError):M.verify_import_inventory(base,expected,'fixture')
                else:M.verify_import_inventory(base,expected,'fixture')
                # Trusted supplied caches are never normalized by verification.
                self.assertTrue((base/'.bootstrap-tools/mathlib/Cache/Main.olean').is_file())

    def test_removed_lean_comment_is_hunk_content_not_a_filename(self):
        M.validate_patch('--- a/Pkg/Model.lean\n+++ b/Pkg/Model.lean\n@@ -1 +1 @@\n--- comment\n+-- edited comment\n', {'Pkg'})

    def test_archive_final_link_graph_cannot_leave_destination(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();archive=root/'chain.tar';destination=root/'dest'
            with tarfile.open(archive,'w') as stream:
                for name,target in [('r/a','b/..'),('r/b','.')]:
                    row=tarfile.TarInfo(name);row.type=tarfile.SYMTYPE;row.linkname=target;stream.addfile(row)
            with self.assertRaises(M.IntegrityError):M.extract_archive(archive,destination,allow_internal_links=True)

    def test_inventory_rejects_fifo_without_opening_a_stream(self):
        import os
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);os.mkfifo(root/'Poison.olean')
            with self.assertRaisesRegex(M.IntegrityError,'regular-file'):
                M.observed_inventory(root,root,loadable_only=True)

    def test_compact_inventory_must_cover_every_search_root(self):
        with tempfile.TemporaryDirectory() as temp:
            with self.assertRaisesRegex(M.IntegrityError,'library-inventory-roots'):
                M.verify_import_inventory(temp,dict(packages={'required':'pin'},library_inventories={}), 'fixture')

    def test_public_recipes_have_neutral_labels_and_no_private_receipts(self):
        root=Path(M.__file__).parent/'data/sticks'
        for name in ['evm','riscv-v2']:
            descriptor=json.loads((root/name/'descriptor.json').read_text())
            for recipe in descriptor['build']['module_recipes'].values():
                self.assertFalse(Path(recipe['source_label']).is_absolute())
                self.assertFalse(set(recipe)&{'origin_record','receipt_sha256','origin_record_sha256','olean_sha256'})
            self.assertNotIn('validation',descriptor)

class RebuildIntegrity(unittest.TestCase):
    def fixture(self, root):
        data = root / 'data'
        cache = root / 'cache'
        data.mkdir()
        cache.mkdir()
        archive = cache / 'upstream.tar.gz'
        with tarfile.open(archive, 'w:gz') as stream:
            info = tarfile.TarInfo('upstream/Model.lean')
            body = b'def answer : Nat := 1\n'
            info.size = len(body)
            stream.addfile(info, io.BytesIO(body))
        patch = (b'--- a/Model/Model.lean\n+++ b/Model/Model.lean\n'
                 b'@@ -1 +1 @@\n-def answer : Nat := 1\n+def answer : Nat := 2\n')
        (data / 'repair.patch').write_bytes(patch)
        sources = {'Model/Model.lean': digest(b'def answer : Nat := 2\n')}
        expected = dict(source_files=sources, source_digest=M.map_digest(sources),
                        definition_files=sources, definition_digest=M.map_digest(sources))
        (data / 'expected.json').write_text(json.dumps(expected))
        descriptor = dict(schema_version=1, name='fixture', lean='v4.34.1',
            upstreams=[dict(name='Model', repository='https://example.invalid/model', commit='1'*40,
                            archive_url='https://example.invalid/upstream.tar.gz',
                            archive_sha256=digest(archive.read_bytes()), content_sha256=M.archive_content_digest(archive), licence='MIT', notice='NOTICE.md')],
            patches=[dict(path='repair.patch', sha256=digest(patch))], expected='expected.json',
            expected_sha256=digest((data / 'expected.json').read_bytes()))
        (data / 'NOTICE.md').write_text('Synthetic fixture notice\n')
        descriptor['tracked_files']={'NOTICE.md':digest((data/'NOTICE.md').read_bytes())}
        (data / 'descriptor.json').write_text(json.dumps(descriptor))
        archive.rename(cache / (descriptor['upstreams'][0]['archive_sha256'] + '.tar.gz'))
        return data, cache, descriptor

    def test_same_tree_in_different_archive_is_accepted(self):
        with tempfile.TemporaryDirectory() as temp:
            data,cache,descriptor=self.fixture(Path(temp))
            row=descriptor['upstreams'][0];archive=cache/(row['archive_sha256']+'.tar.gz')
            with tarfile.open(archive,'w:gz') as stream:
                info=tarfile.TarInfo('other-root/Model.lean');body=b'def answer : Nat := 1\n'
                info.size=len(body);info.mtime=123;stream.addfile(info,io.BytesIO(body))
            self.assertNotEqual(M.sha256(archive),row['archive_sha256'])
            self.assertEqual(M.fetch_archive(row,cache,True),archive)
            M.materialize_sources(data,Path(temp)/'output',cache,offline=True)

    def test_untracked_notice_and_plugin_arguments_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            data,_,descriptor=self.fixture(Path(temp))
            descriptor['tracked_files']={}
            (data/'descriptor.json').write_text(json.dumps(descriptor))
            with self.assertRaisesRegex(M.IntegrityError,'notice-untracked'):M.load_descriptor(data)
            descriptor['tracked_files']={'NOTICE.md':M.sha256(data/'NOTICE.md')}
            descriptor['build']={'module_recipes':{'Model':{'arguments':['--plugin','untrusted']}}}
            (data/'descriptor.json').write_text(json.dumps(descriptor))
            with self.assertRaisesRegex(M.IntegrityError,'recipe-argument'):M.load_descriptor(data)

    def test_explicit_file_rows_preserve_frozen_canonical_digest(self):
        with tempfile.TemporaryDirectory() as temp:
            data, _, descriptor = self.fixture(Path(temp))
            expected = json.loads((data / 'expected.json').read_text())
            for key in ['source_files', 'definition_files']:
                expected[key] = [dict(path=name, sha256=value) for name, value in expected[key].items()]
            (data / 'expected.json').write_text(json.dumps(expected))
            descriptor['expected_sha256'] = digest((data / 'expected.json').read_bytes())
            (data / 'descriptor.json').write_text(json.dumps(descriptor))
            _, decoded = M.load_descriptor(data)
            self.assertEqual(M.map_digest(decoded['source_files']), expected['source_digest'])
            self.assertEqual(decoded['definition_files'], decoded['source_files'])

    def test_explicit_file_rows_reject_duplicate_paths(self):
        with self.assertRaisesRegex(M.IntegrityError, 'duplicate-file-path'):
            M.file_map([dict(path='A.lean', sha256='1'*64), dict(path='A.lean', sha256='2'*64)])

    def test_verified_archive_and_ordered_patch_rebuild_exact_sources(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            data, cache, _ = self.fixture(root)
            M.materialize_sources(data, root / 'out', cache, offline=True)
            self.assertEqual((root / 'out/Model/Model.lean').read_text(), 'def answer : Nat := 2\n')
            M.verify_sources(root / 'out', M.load_descriptor(data)[1])

    def test_corrupt_cache_fails_before_source_publication(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            data, cache, descriptor = self.fixture(root)
            (cache / (descriptor['upstreams'][0]['archive_sha256'] + '.tar.gz')).write_bytes(b'corrupt')
            with self.assertRaisesRegex(M.IntegrityError, 'archive-sha256'):
                M.materialize_sources(data, root / 'out', cache, offline=True)
            self.assertFalse((root / 'out/Model/Model.lean').exists())

    def test_patch_and_expected_tampering_fail_before_extraction(self):
        for name in ['repair.patch', 'expected.json']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                data, cache, _ = self.fixture(root)
                with (data / name).open('ab') as stream:
                    stream.write(b' ')
                with self.assertRaises(M.IntegrityError):
                    M.materialize_sources(data, root / 'out', cache, offline=True)
                self.assertFalse((root / 'out').exists())

    def test_source_drift_and_definition_digest_drift_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            data, cache, _ = self.fixture(root)
            M.materialize_sources(data, root / 'out', cache, offline=True)
            expected = M.load_descriptor(data)[1]
            expected['definition_digest'] = '0' * 64
            with self.assertRaisesRegex(M.IntegrityError, 'definition-digest'):
                M.verify_sources(root / 'out', expected)
            (root / 'out/Model/Model.lean').write_text('def answer : Nat := 3\n')
            with self.assertRaisesRegex(M.IntegrityError, 'source-sha256'):
                M.verify_sources(root / 'out', expected)

    def test_archive_rejects_links_and_parent_traversal(self):
        for name, kind in [('x/../outside', tarfile.REGTYPE), ('x/link', tarfile.SYMTYPE)]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                archive = root / 'bad.tar.gz'
                with tarfile.open(archive, 'w:gz') as stream:
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = str(root.resolve().parent / 'outside')
                    stream.addfile(member)
                with self.assertRaises(M.IntegrityError):
                    M.extract_archive(archive, root / 'out')

    def test_internally_inconsistent_digest_is_rejected_by_schema_check(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            data, _, descriptor = self.fixture(root)
            expected = json.loads((data / 'expected.json').read_text())
            expected['definition_digest'] = '0' * 64
            (data / 'expected.json').write_text(json.dumps(expected))
            descriptor['expected_sha256'] = digest((data / 'expected.json').read_bytes())
            (data / 'descriptor.json').write_text(json.dumps(descriptor))
            with self.assertRaisesRegex(M.IntegrityError, 'definition-digest'):
                M.load_descriptor(data)


class BaseCacheInventory(unittest.TestCase):
    def fixture(self, root):
        import platform
        base, metadata = root / 'base', root / 'metadata'
        metadata.mkdir()
        roots = [base / '.lake/packages' / name / '.lake/build/lib/lean' for name in ['early', 'good']]
        for directory in [*roots, base / 'lean-v4.34.1/lib/lean']:
            directory.mkdir(parents=True)
        pinned = roots[1] / 'Pinned.olean'
        pinned.write_bytes(b'pinned artifact')
        for name in ['lakefile.toml', 'lake-manifest.json', 'lean-toolchain']:
            (metadata / name).write_text('pinned project metadata')
            (base / name).write_bytes((metadata / name).read_bytes())
        expected = dict(packages=dict(early='1'*40, good='2'*40),
            library_files={str(pinned.relative_to(base)): digest(pinned.read_bytes())},
            toolchain_profiles={platform.system()+'-'+platform.machine(): {}}, toolchain_symlinks={})
        (metadata / 'expected.json').write_text(json.dumps(expected))
        return base, metadata, roots

    def test_extra_loadable_module_cannot_shadow_pinned_library(self):
        for suffix in ['.olean', '.olean.server', '.olean.private', '.ir', '.ir.sig']:
            for location in ['package', 'sysroot']:
                with self.subTest(suffix=suffix, location=location), tempfile.TemporaryDirectory() as temp:
                    base, metadata, roots = self.fixture(Path(temp))
                    directory = roots[0] if location == 'package' else base / 'lean-v4.34.1/lib/lean'
                    (directory / ('Pinned' + suffix)).write_bytes(b'unpinned shadow artifact')
                    with self.assertRaisesRegex(M.IntegrityError, 'unexpected-base-artifact'):
                        M.verify_base(base, metadata)

    def test_nested_directory_link_cannot_hide_loadable_modules(self):
        with tempfile.TemporaryDirectory() as temp:
            base, metadata, roots = self.fixture(Path(temp))
            (roots[0] / 'Alias').symlink_to(roots[1], target_is_directory=True)
            with self.assertRaisesRegex(M.IntegrityError, 'unexpected-base-link'):
                M.verify_base(base, metadata)

    @unittest.skipIf(__import__("os").geteuid() == 0, "requires non-root permission enforcement")
    def test_searchable_unlistable_directory_cannot_hide_module(self):
        with tempfile.TemporaryDirectory() as temp:
            base, metadata, roots = self.fixture(Path(temp))
            hidden = roots[0] / 'Hidden'
            hidden.mkdir()
            (hidden / 'Unknown.olean').write_bytes(b'unpinned artifact')
            hidden.chmod(0o111)
            try:
                with self.assertRaisesRegex(M.IntegrityError, 'import-inventory-read'):
                    M.verify_base(base, metadata)
            finally:
                hidden.chmod(0o700)

    def test_resume_build_root_rejects_shadow_modules_and_sidecars(self):
        for name in ['Mathlib/Shadow.olean', 'Pinned.olean.private', 'Pinned.ir']:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temp:
                destination = Path(temp)
                target = destination / 'build' / name
                target.parent.mkdir(parents=True)
                target.write_bytes(b'unpinned artifact')
                expected = dict(port_manifest=dict(files={'build/Pinned.olean': '1'*64}))
                with self.assertRaisesRegex(M.IntegrityError, 'unexpected-build-artifact'):
                    M.verify_build_inventory(destination, expected)

    def test_pinned_companions_are_allowed_and_content_checked(self):
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp)
            build = destination / 'build'
            build.mkdir()
            primary = build / 'Pinned.olean'
            primary.write_bytes(b'frozen primary')
            companion = build / 'Pinned.olean.private'
            companion.write_bytes(b'frozen private declarations')
            expected = dict(port_manifest=dict(files={'build/Pinned.olean': digest(primary.read_bytes())}),
                            companion_artifacts={'build/Pinned.olean.private': digest(companion.read_bytes())})
            M.verify_build_inventory(destination, expected)
            M.verify_frozen_artifacts(destination, expected)
            companion.write_bytes(b'changed private declarations')
            with self.assertRaises(M.IntegrityError):
                M.verify_frozen_artifacts(destination, expected)

    def test_nonloadable_lake_metadata_is_permitted(self):
        with tempfile.TemporaryDirectory() as temp:
            base, metadata, roots = self.fixture(Path(temp))
            (roots[0] / 'Pinned.ilean').write_bytes(b'editor metadata')
            M.verify_base(base, metadata)


class VerifiedCompilerInput(unittest.TestCase):
    def test_only_hash_verified_held_source_reaches_compiler_stdin(self):
        helper = Path(__file__).parent / 'zk_registry/exec_stick_source.py'
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / 'Model.lean'
            source.write_bytes(b'def answer := 2\n')
            command = [sys.executable, str(helper), str(source), digest(source.read_bytes()), sys.executable,
                       '-c', 'import sys;sys.stdout.buffer.write(sys.stdin.buffer.read())']
            result = subprocess.run(command, capture_output=True)
            self.assertEqual((result.returncode, result.stdout), (0, source.read_bytes()))
            source.write_text('changed input')
            result = subprocess.run(command, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b'')

    def test_source_rewrite_after_exec_cannot_change_verified_input(self):
        helper = Path(__file__).parent / 'zk_registry/exec_stick_source.py'
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp) / 'Model.lean'
            original = b'def answer := 2\n'
            source.write_bytes(original)
            program = ('import sys;from pathlib import Path;'
                       'Path(sys.argv[1]).write_text("replacement");'
                       'sys.stdout.buffer.write(sys.stdin.buffer.read())')
            result = subprocess.run([sys.executable, str(helper), str(source), digest(original),
                                     sys.executable, '-c', program, str(source)], capture_output=True)
            self.assertEqual((result.returncode, result.stdout), (0, original))


if __name__ == '__main__':
    unittest.main()
