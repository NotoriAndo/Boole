"""DET3d review: trust boundaries, capped checker plumbing and public provenance."""
import copy
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import fuel_caps as F, closure as D, safe_fs as FS
from zk_registry.sandbox import Sandbox
import test_zk_registry_det3d as base_tests


class AuthorityReview(unittest.TestCase):
    setUp = base_tests.BoundCaps.setUp
    def test_structural_bound_is_the_only_reason_for_rejection(self):
        reference = copy.deepcopy(self.reference)
        reference['units']['lean'] = 2**61
        key = F.digest(reference)
        authority = dict(self.authority, references={key: reference})
        bound = copy.deepcopy(self.bound)
        bound['cap']['reference'] = key
        bound['cap']['units']['lean'] = 2**63
        bound['sha256'] = F.digest(bound['cap'])
        # All hashes, exact 4x equality and the uint64 range are valid.
        with patch.dict(F.STRUCTURAL, lean=2**63):
            self.assertEqual(F.resolve(bound, authority)['units']['lean'], 2**63)
        with self.assertRaisesRegex(F.FuelCapError, 'invalid-fourfold-cap'):
            F.resolve(bound, authority)

    def test_authority_parses_the_bytes_it_hashed(self):
        path = Path(F.__file__).parent/'data/fuel_cap_authority_v1.json'
        original = Path.read_bytes
        raw = path.read_bytes()
        expected = json.loads(raw)
        calls = []
        def read(selected):
            if selected != path:
                return original(selected)
            calls.append(selected)
            return raw if len(calls)==1 else json.dumps(dict(factors=expected['factors'], changed=True)).encode()
        with patch.object(F.Path, 'read_bytes', read), patch.object(F.Path, 'read_text', return_value=json.dumps(dict(factors=expected['factors'], changed=True))):
            self.assertEqual(F.load_authority(), expected)
        self.assertEqual(len(calls), 1, 'the authority must be read exactly once')

    def test_package_refuses_unresolvable_cap(self):
        from test_zk_registry_package import make_package
        from zk_registry import package as P
        with tempfile.TemporaryDirectory() as temp:
            directory, problem = make_package(temp)
            bound = copy.deepcopy(self.bound)
            bound['cap']['problem'] = problem['package_id']
            bound['sha256'] = F.digest(bound['cap'])
            problem['fuel_cap'] = bound
            self.assertTrue(P.validate_problem(problem, directory))


class ComparatorReview(unittest.TestCase):
    def fixture(self, root, *, debits=True, expect_budget=False, quotient="accept", code=0, declaration="Control.target"):
        state, left, right = [root/name for name in ('state.olean', 'left.ndjson', 'right.ndjson')]
        for path in (state, left, right): path.write_bytes(b'generated immutable input')
        binary = root/'comparator'
        events = [dict(version=1, stage=stage, result=quotient if stage=='quotient' else 'accept') for stage in ('lean', 'nanoda', 'quotient')]
        if debits:
            events += [dict(version=1, stage=stage, kernel=kernel, debit=1, checked=1, calls=1, depth=1,
                            **({'composed_depth': 1} if kernel=='nanoda' else {}))
                       for stage,kernel in [('fuel','lean'),('nanoda-fuel','nanoda')]]
        events += [dict(version=1, stage='final', rc=code)]
        binary.write_text('#!/usr/bin/python3\nimport json,os,sys\nfrom pathlib import Path\n'
            'cfg=json.loads(Path(sys.argv[1]).read_text())\n'
            + ('assert cfg["lean_kernel"]["aggregate_raw"]==324\nassert cfg["nanoda_kernel"]["aggregate_work"]==548\n'
               'assert cfg["candidate_declarations"]=='+repr([declaration])+'\n' if expect_budget else '')
            + 'Path('+repr(str(root/'started'))+').touch()\n'
            + 'Path(os.environ["COMPARATOR_STATUS_FILE"]).write_text('+repr(''.join(json.dumps(e)+'\n' for e in events))+')\nraise SystemExit('+str(code)+')\n')
        binary.chmod(0o755)
        budget = F.Budget(324,548,'a'*64,str(state),D.digest(state),'b'*64)
        tools = D.Tools(str(binary),'/usr/bin/true','/usr/bin/true','/usr',None)
        return state,left,right,budget,tools

    def compare(self, root, boundary=None, **kwargs):
        state,left,right,budget,tools = self.fixture(root, **kwargs)
        return D.compare_exports(str(left),str(right),'Control.target',tools,boundary or Sandbox.fixture(),
                                 str(root/'work'),5,[],fuel_budget=budget)

    def test_deleted_old_writer_does_not_poison_an_unrelated_cache(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);writer=root/'old-writer';writer.mkdir();identity=FS.root_identity(writer);writer.rmdir()
            boundary=replace(Sandbox.fixture(),exposed_writers={str(writer):identity})
            row=self.compare(root,boundary)
            self.assertEqual(row['verdict'],'PASS',row)

    def test_renamed_writer_is_refused_by_its_inode(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);writer=root/'old-writer';writer.mkdir();identity=FS.root_identity(writer)
            renamed=root/'renamed';writer.rename(renamed)
            state,left,right,budget,tools=self.fixture(root)
            target=renamed/'nested/state.olean';target.parent.mkdir();state.rename(target)
            budget=replace(budget,state=str(target))
            boundary=replace(Sandbox.fixture(),exposed_writers={str(writer):identity})
            row=D.compare_exports(str(left),str(right),'Control.target',tools,boundary,str(root/'work'),5,[],fuel_budget=budget)
            self.assertEqual(row['verdict'],'ERROR')
            self.assertFalse((root/'started').exists())

    def test_capped_legacy_tool_table_still_requires_both_debits(self):
        with tempfile.TemporaryDirectory() as temp:
            self.assertEqual(self.compare(Path(temp),debits=False)['verdict'],'ERROR')

    def test_capped_quotient_rejection_requires_debits(self):
        with tempfile.TemporaryDirectory() as temp:
            row=self.compare(Path(temp),debits=False,quotient='reject',code=2)
            self.assertEqual(row['verdict'],'ERROR')

    def test_legacy_generated_decoder_unavailability_is_error(self):
        from zk_registry import correctness_runtime as CR
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);state,left,right,budget,tools=self.fixture(root)
            events=[dict(version=1,stage='error',reason='kernel-unavailable'),dict(version=1,stage='final',rc=70)]
            binary=Path(tools.comparator)
            binary.write_text('#!/usr/bin/python3\nimport os,json\nfrom pathlib import Path\n'
                'Path(os.environ["COMPARATOR_STATUS_FILE"]).write_text('+repr(''.join(json.dumps(e)+'\n' for e in events))+')\nraise SystemExit(70)\n')
            row=D.compare_exports(str(left),str(right),'Control.target',tools,Sandbox.fixture(),str(root/'work'),5,[])
            self.assertEqual(row['verdict'],'ERROR')

    def test_capped_check_binds_the_real_statement_and_native_configuration(self):
        from test_zk_registry_package import make_package
        from test_zk_registry_check import fake_env, submission, VerdictMappingTests
        from zk_registry import check as C, axiom_policy as A
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            pkg,problem=make_package(str(root/'package'))
            declaration=problem['checker']['theorem_fqn']
            state,left,right,budget,tools=self.fixture(root,expect_budget=True,declaration=declaration)
            fixture=base_tests.BoundCaps();fixture.setUp()
            reference=copy.deepcopy(fixture.reference);reference['problem']=problem['package_id']
            authority=copy.deepcopy(fixture.authority);key=F.digest(reference);authority['references']={key:reference}
            cap=copy.deepcopy(fixture.cap);cap.update(problem=problem['package_id'],reference=key)
            problem['fuel_cap']=dict(cap=cap,sha256=F.digest(cap))
            Path(pkg,'problem.json').write_text(json.dumps(problem))
            proof=root/'Proof.lean';proof.write_text(submission(Path(pkg,'Statement.lean').read_text(),'  trivial\n'))
            env=fake_env(temp)
            compile_record=VerdictMappingTests().ok_compile(post=VerdictMappingTests().ok_post(
                consts=[dict(name=declaration,kind='theorem',unsafe=False,partial=False)]))
            # Substitute only the native compilation/export boundaries. Real check(),
            # fuel binding, statement identity and Comparator consumer remain in use.
            reference['environment']='a'*64
            state_root=root/'certified';cert=state_root/('a'*64)/'state.olean';cert.parent.mkdir(parents=True);cert.write_bytes(state.read_bytes())
            authority['environments']={'a'*64:dict(eligible=True,certified_kernels=['lean','nanoda'],profiles={
                F.platform.system()+'-'+F.platform.machine():dict(state_sha256=D.digest(cert),certificate_sha256='e'*64)})}
            env=replace(env,extra=dict(env.extra,certified_environment_root=str(state_root)))
            with patch.object(F,'load_authority',return_value=authority),patch.object(C,'compile_in_sandbox',return_value=compile_record), \
                 patch.object(D,'prepare_reference',return_value=dict(verdict='PASS',path=str(left),statement_closure_sha256='b'*64)), \
                 patch.object(D,'export_module',return_value=dict(verdict='PASS',path=str(right))),patch.object(A,'resolve',return_value=[]):
                row=C.check(pkg,str(proof),env,tools=tools,sandbox=Sandbox.fixture(),policy_fixture=env.pins())
            self.assertEqual(row['verdict'],'PASS',row)
            self.assertEqual(row['two_kernels']['fuel_cap']['units'],dict(lean=324,nanoda=548))


class SourceAdoptionReview(unittest.TestCase):
    def test_equal_content_with_different_path_case_cannot_be_adopted(self):
        from zk_registry import counter_identity
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'manifest.json').write_bytes(b'public generated bytes')
            # On case-sensitive hosts the alias has the same bytes as well.
            if not (root/'MANIFEST.json').exists(): (root/'MANIFEST.json').write_bytes(b'public generated bytes')
            with self.assertRaisesRegex(ValueError,'case-exact'):
                counter_identity.case_exact_path(root,'MANIFEST.json',['manifest.json'])
            self.assertEqual(counter_identity.case_exact_path(root,'manifest.json',['manifest.json']),root/'manifest.json')

    def test_consumer_open_is_checked_against_tracked_spelling(self):
        from zk_registry.source_adoption import audit_reads, verify_snapshot
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);(root/'manifest.json').write_bytes(b'generated public bytes')
            expected={'manifest.json':hashlib.sha256(b'generated public bytes').hexdigest()}
            with audit_reads(root,expected) as observed:
                self.assertEqual((root/'manifest.json').read_bytes(),b'generated public bytes')
                with self.assertRaisesRegex(ValueError,'case-exact'):
                    (root/'MANIFEST.json').read_bytes()
            self.assertEqual(verify_snapshot(root,expected,observed),dict(files=1,opened_paths=['manifest.json']))


if __name__=='__main__':unittest.main()
