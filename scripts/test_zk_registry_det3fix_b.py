"""Service-style replay resolution and reference availability regressions."""
from pathlib import Path
from unittest.mock import patch
import hashlib
import json
import os
import platform
import subprocess
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parent))
from zk_registry import lean_runner as L, gates as G, closure as D, axiom_policy as A
from zk_registry import ratchet as R, correctness_build as B
from zk_registry.sandbox import Sandbox


class ReferenceReplayReview(unittest.TestCase):
    def test_explicit_reference_tools_override_stale_environment_tools(self):
        from zk_registry import check as C, artifact_pins
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'Statement.lean').write_text('theorem t : True := by trivial')
            drivers=[]
            for name in ('stale','explicit'):
                path=root/name;path.write_text('#!/bin/sh\nexit 0\n');path.chmod(0o755);drivers.append(str(path))
            stale=D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None,lean_replay=drivers[0])
            explicit=D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None,lean_replay=drivers[1])
            env=L.LeanEnv('/usr',[],'v4.34.1',{},'',str(root),dict(acceptance_tools=stale))
            table=json.loads((Path(D.__file__).parent/'data/acceptance_tools_v2.json').read_text());pin=table['binary_profiles']['Darwin-arm64']['lean_replay']['sha256']
            def compile_probe(selected,*args,**kwargs):
                with patch.object(platform,'system',return_value='Darwin'),patch.object(platform,'machine',return_value='arm64'),patch.object(artifact_pins,'digest',return_value=pin),patch.object(L,'run_process',return_value=L.RunResult(0,'',0,False)) as launched:
                    G.run_replay(selected,'M.olean','T.t',str(root/'record'),[],[],5)
                self.assertEqual(launched.call_args.args[0][0],drivers[1])
                return dict(error='generated-probe-complete')
            with patch.object(C,'compile_in_sandbox',side_effect=compile_probe):
                D.prepare_reference(env,dict(checker=dict(statement_file='Statement.lean')),str(root),str(root/'work'),str(root/'sealed'),explicit,Sandbox.fixture(),5,[])

    def test_old_catalog_tool_binding_fails_closed_before_execution(self):
        from zk_registry import acceptance as binding
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);metadata=root/'problem.json';metadata.write_text('{"package_id":"generated"}')
            export=root/'acceptance/Challenge.ndjson';export.parent.mkdir();export.write_bytes(b'sealed reference')
            manifest=dict(schema_version=2,package_id='generated',metadata_sha256=D.digest(metadata),
                environment_descriptor_sha256=D.digest(binding.DATA/'acceptance_environment_v2.json'),
                # Deliberately stale descriptor: this negative binding is not a current pin.
                tools_descriptor_sha256='1ca9703bf526f52c56d7e185a91f903913beafc5684901b913fc049360f5675f',
                forbidden_version=5,challenge_export=dict(path='acceptance/Challenge.ndjson',sha256=D.digest(export)),
                reference_type_sha256='ab'*32,measuring_stick_axioms=[],migration={})
            (root/'Acceptance.json').write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError,'^reference-binding-invalid$'):
                binding.read(str(root),dict(package_id='generated'),required=True)

    def test_tools_arriving_only_through_service_env_launch_the_native_driver(self):
        from zk_registry import artifact_pins
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);driver=root/'replay';driver.write_text('#!/bin/sh\nexit 0\n');driver.chmod(0o755)
            tools=D.Tools('/usr/bin/true','/usr/bin/true','/usr/bin/true','/usr',None,lean_replay=str(driver))
            env=L.LeanEnv('/usr',[],'v4.34.1',{},'',str(root),dict(acceptance_tools=tools))
            table=json.loads((Path(D.__file__).parent/'data/acceptance_tools_v2.json').read_text())
            pin=table['binary_profiles']['Darwin-arm64']['lean_replay']['sha256']
            with patch.object(platform,'system',return_value='Darwin'),patch.object(platform,'machine',return_value='arm64'),patch.object(artifact_pins,'digest',return_value=pin),patch.object(L,'run_process',return_value=L.RunResult(0,'',0,False)) as launched:
                result,_=G.run_replay(env,'M.olean','T.t',str(root/'record'),[],[],5)
            self.assertEqual(result.rc,0)
            self.assertEqual(launched.call_args.args[0][0],str(driver))

    def test_g_elab_and_ratchet_never_fail_an_unavailable_replay(self):
        warning=dict(severity='warning',data="declaration uses sorry")
        post=dict(replay=None,target=None,axioms=[])
        with tempfile.TemporaryDirectory() as root:
            with patch.object(L,'compile_module',side_effect=[(L.RunResult(0,'',0,False),[]),(L.RunResult(0,'',0,False),[warning])]),patch.object(G,'run_replay',return_value=(L.RunResult(70,'',0,False,tool_exit=70),post)):
                result=G.g_elab(None,root,root,'N',root)
            self.assertEqual((result.status,result.detail.get('reason')),('ERROR','environment-tool-unavailable:replay'))
            with patch.object(L,'compile_module',return_value=(L.RunResult(0,'',0,False),[warning])),patch.object(G,'run_replay',return_value=(L.RunResult(70,'',0,False,tool_exit=70),post)):
                result=R._elaborate(None,root,[],'T.t',[],root,5)
            self.assertEqual((result['status'],result.get('reason')),('ERROR','environment-tool-unavailable:replay'))

    def test_correctness_reference_raises_availability_instead_of_generic_failure(self):
        warning=dict(severity='warning',data='declaration uses sorry')
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            with patch.object(B.CG,'theorem_template',return_value='theorem t : True := by sorry'),patch.object(L,'compile_module',return_value=(L.RunResult(0,'',0,False),[warning])),patch.object(G,'run_replay',return_value=(L.RunResult(70,'',0,False,tool_exit=70),{})):
                with self.assertRaises(L.ResourceUnavailable):B.reference(dict(id='Toy'),'soundness',root,root,None,5)

    def test_reference_binding_non_reject_exits_are_unavailable(self):
        for code in (1,3,4,70,71):
            with self.subTest(code=code),tempfile.TemporaryDirectory() as temp:
                root=Path(temp);binary=root/'Ref.olean';binary.write_bytes(b'pristine')
                row=dict(module='Ref',name='N.ax',module_olean_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
                env=L.LeanEnv('/usr',[],'v4.34.1',{},'',str(root))
                with patch.object(G,'run_replay',return_value=(L.RunResult(code,'',0,False,tool_exit=code),{})):
                    with self.assertRaises(A.BindingUnavailable) as caught:A.resolve(env,[row],str(root),Sandbox.fixture(),5)
                from zk_registry import error_policy
                self.assertEqual(error_policy.classify(str(caught.exception),'reference:replay'),'D-node')

    def test_archive_ignores_autocrlf_and_host_config(self):
        from zk_registry.build_fuel_tools import archive
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);repo=root/'repo';repo.mkdir();env=dict(os.environ,GIT_CONFIG_GLOBAL='/dev/null',GIT_CONFIG_NOSYSTEM='1')
            subprocess.run(['git','init','-q',str(repo)],env=env,check=True)
            (repo/'sample.txt').write_bytes(b'one\ntwo\n');(repo/'.gitattributes').write_text('*.txt text\n')
            subprocess.run(['git','-C',str(repo),'add','.'],env=env,check=True)
            subprocess.run(['git','-C',str(repo),'-c','user.name=NotoriAndo','-c','user.email=281125350+NotoriAndo@users.noreply.github.com','commit','-qm','generated archive fixture'],env=env,check=True)
            subprocess.run(['git','-C',str(repo),'config','core.autocrlf','true'],env=env,check=True)
            archive(repo,'HEAD',root/'out')
            self.assertEqual((root/'out/sample.txt').read_bytes(),b'one\ntwo\n')


class NativeReplayConfiguration(unittest.TestCase):
    @unittest.skipUnless(os.environ.get('BOOLE_DET3_NATIVE_RUNTIME'),'native runtime supplied by the offline build controls')
    def test_malformed_trusted_limits_are_unavailable(self):
        root=Path(os.environ['BOOLE_DET3_NATIVE_RUNTIME'])
        prefixes=list(root.glob('lean-4.34.1-*'));self.assertEqual(len(prefixes),1)
        env=dict(os.environ,LEAN_SYSROOT=str(prefixes[0].resolve()),LEAN_STACK_SIZE_KB='262144')
        for limits in ('{broken','{"max_heartbeats":"bad"}'):
            result=subprocess.run([str(root/'comparator/.lake/build/bin/zk_replay'),'missing.olean','T.t','unused',limits],env=env,capture_output=True,timeout=10)
            self.assertEqual(result.returncode,70,result.stderr.decode())


if __name__=='__main__':unittest.main()
