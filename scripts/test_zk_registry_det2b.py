import json, re, sys, unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from zk_registry import check as C
class PolicyV5(unittest.TestCase):
    def test_shared_cases(self):
        corpus=json.loads((Path(__file__).resolve().parents[1]/'fixtures/submission-policy/cases-v5.json').read_text())
        for case in corpus['cases']:
            with self.subTest(case=case['name']):
                actual=C.forbidden_scan([('part',case['source'])])
                expected=[(h['label'],h['line']) for h in case['hits']]
                self.assertEqual(sorted((h.split(' in ')[0],int(re.search(r'line (\d+)',h)[1])) for h in actual),sorted(expected))
    def test_historical_vocabulary(self):
        self.assertNotIn('output-command', C.FORBIDDEN_SETS[4])
        self.assertTrue({'output-command','output-option','forbidden-name-character'}<=set(C.FORBIDDEN_SETS[5]))

class GateDescriptor(unittest.TestCase):
    def test_new_store_records_current_policy(self):
        import tempfile, hashlib
        from zk_registry import service as S
        with tempfile.TemporaryDirectory() as root:
            registry=S.Registry.create(root,dict(entries=[],packages={},excluded={}))
            self.assertEqual(registry.status()['submission_policy'],dict(version=5,sha256=hashlib.sha256((Path(C.__file__).parent/'data/submission_policy_v5.json').read_bytes()).hexdigest()))

if __name__=='__main__': unittest.main()
