"""Portable shell TERM control; it never executes the guest recipe or Linux API."""
import json,os,re,signal,subprocess,sys,tempfile,unittest
from pathlib import Path
SOURCE=Path(__file__).parent/'devnet/w0/guest.sh'
if '--source' in sys.argv:
    i=sys.argv.index('--source');SOURCE=Path(sys.argv[i+1]);del sys.argv[i:i+2]

class ShellCleanupTests(unittest.TestCase):
    def test_term_interrupts_wait_and_retires_the_manager_job(self):
        text=SOURCE.read_text();match=re.search(r'^w0_cleanup\(\) \{.*?^\}',text,re.M|re.S)
        self.assertIsNotNone(match)
        # Use the recipe's actual cleanup function. Only its Linux pidfd helper
        # is replaced by a host fake that kills this test's one owned child.
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp).resolve();(root/'group/manager').mkdir(parents=True);(root/'bin').mkdir()
            fake=root/'bin/python3';fake.write_text('#!/bin/bash\nread -r pid < "$3/cgroup.procs"\n/bin/kill -KILL "$pid"\n');fake.chmod(0o700)
            job=root/'job.sh';job.write_text('#!/bin/bash\necho $$ > "'+str(root/'group/manager/cgroup.procs')+'"\necho READY\nexec /bin/sleep 30\n');job.chmod(0o700)
            background=bool(re.search(r'&\s*\nw0_job=\$!;wait',text))
            script='set -e\nw0_cgroup="'+str(root/'group')+'"\nw0_run="'+str(root)+'"\nw0_packages=0\n'+match.group(0)+'\ntrap w0_cleanup EXIT\ntrap "exit 124" TERM\n"'+str(job)+'"'+(' &\nw0_job=$!;wait "$w0_job"\n' if background else '\n')
            process=subprocess.Popen(['/bin/bash','-c',script],env={'PATH':str(root/'bin')+':/usr/bin:/bin','LC_ALL':'C'},stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
            try:
                self.assertEqual(process.stdout.readline(),b'READY\n')
                process.send_signal(signal.SIGTERM)
                self.assertEqual(process.wait(timeout=2),124)
                self.assertEqual(json.loads((root/'guest-release.json').read_text())['guest_status'],124)
                pid=int((root/'group/manager/cgroup.procs').read_text())
                with self.assertRaises(ProcessLookupError):os.kill(pid,0)
            finally:
                if process.poll() is None:os.killpg(process.pid,signal.SIGKILL);process.wait()
                process.stdout.close();process.stderr.close()

if __name__=='__main__':unittest.main()
