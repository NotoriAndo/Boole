"""Runtime writes stay below the caller's owned, real directory tree."""
from pathlib import Path
import hashlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))


class RuntimeStagingTests(unittest.TestCase):
    def test_directory_link_never_changes_borrowed_bytes_or_mtime(self):
        from zk_registry.runtime_staging import stage_binary
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);owned=root/'owned';owned.mkdir();borrowed=root/'borrowed';borrowed.mkdir()
            source=root/'source';source.write_bytes(b'new executable');source.chmod(0o755)
            canary=borrowed/'tool';canary.write_bytes(b'borrowed original')
            before=(canary.read_bytes(),canary.stat().st_mtime_ns)
            (owned/'linked').symlink_to(borrowed,target_is_directory=True)
            with self.assertRaises((OSError,ValueError)):
                stage_binary(owned, 'linked/tool', source, hashlib.sha256(source.read_bytes()).hexdigest())
            self.assertEqual((canary.read_bytes(),canary.stat().st_mtime_ns),before)

    def test_existing_file_is_not_replaced_and_new_copy_is_verified(self):
        from zk_registry.runtime_staging import stage_binary
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);owned=root/'owned';owned.mkdir();source=root/'source';source.write_bytes(b'bytes');source.chmod(0o755)
            digest=hashlib.sha256(source.read_bytes()).hexdigest()
            target=stage_binary(owned,'component/bin/tool',source,digest)
            self.assertEqual(target.read_bytes(),b'bytes')
            before=target.stat().st_mtime_ns
            with self.assertRaises(FileExistsError):stage_binary(owned,'component/bin/tool',source,digest)
            self.assertEqual(target.stat().st_mtime_ns,before)
            with self.assertRaises(ValueError):stage_binary(owned,'../outside',source,digest)


if __name__=='__main__':unittest.main()
