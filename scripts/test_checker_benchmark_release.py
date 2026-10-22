"""The real benchmark prepares a known release without a shared helper build."""
import ast
import hashlib
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

class BenchmarkRelease(unittest.TestCase):
    def test_prepared_checker_matches_released_bytes_without_running_lean(self):
        root=pathlib.Path(__file__).resolve().parents[1]
        shell=(root/'scripts/proof-to-block-benchmark.sh').read_text()
        source=shell.split("<<'PY'\n",1)[1].rsplit('\nPY',1)[0]
        tree=ast.parse(source)
        functions=[node for node in tree.body if isinstance(node,ast.FunctionDef) and node.name in ('write_lean_checker_workspace','checker_artifact_hash')]
        self.assertEqual(len(functions),2)
        namespace=dict(root=root,pathlib=pathlib,hashlib=hashlib,shutil=shutil,subprocess=subprocess,sys=sys)
        exec(compile(ast.Module(body=functions,type_ignores=[]),str(root/'scripts/proof-to-block-benchmark.sh'),'exec'),namespace)
        with tempfile.TemporaryDirectory() as temp,patch.object(subprocess,'run',side_effect=AssertionError('fixture preparation must not execute a toolchain')):
            workspace=pathlib.Path(temp)/'checker'
            proof=namespace['write_lean_checker_workspace'](workspace)
            self.assertTrue(proof.is_file())
            pin=re.search(r'^([0-9a-f]{64})$',(root/'lean/checker/README.md').read_text(),re.M)[1]
            self.assertEqual(namespace['checker_artifact_hash'](workspace),pin)
            self.assertEqual((workspace/'Boole/Family/V0Helpers.lean').read_bytes(),(root/'lean/checker/Boole/Family/V0Helpers.lean').read_bytes())
            self.assertFalse((workspace/'.lake/build').exists())

if __name__=='__main__':unittest.main()
