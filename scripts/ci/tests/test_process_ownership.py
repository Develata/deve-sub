"""Only synthetic owned children: timeout/TERM must reach cleanup finally blocks."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from normal import execute

CHILD = '''import subprocess,sys,signal,time
from pathlib import Path
p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)'])
def stop(*_):
 p.wait(timeout=3)
 raise SystemExit(0)
signal.signal(signal.SIGTERM,stop)
Path(sys.argv[1]).write_text(str(p.pid))
time.sleep(60)
'''


class Ownership(unittest.TestCase):
    def test_noncooperative_descendant_cannot_outlive_its_direct_parent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);pids=root/'child.pid'
            grandchild='import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'
            parent='import subprocess,sys,time;from pathlib import Path;p=subprocess.Popen([sys.executable,"-c",sys.argv[2]]);Path(sys.argv[1]).write_text(str(p.pid));time.sleep(60)'
            with self.assertRaises(subprocess.TimeoutExpired):
                execute([sys.executable,'-c',parent,str(pids),grandchild],root,os.environ.copy(),root/'command.log',timeout=.5)
            pid=int(pids.read_text());state=Path(f'/proc/{pid}/stat')
            deadline=time.monotonic()+2
            while state.exists() and state.read_text().split()[2]!='Z' and time.monotonic()<deadline:time.sleep(.01)
            self.assertTrue(not state.exists() or state.read_text().split()[2]=='Z')

    def test_timeout_reaps_owned_group(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);pids=root/'child.pid'
            with self.assertRaises(subprocess.TimeoutExpired):
                execute([sys.executable,'-c',CHILD,str(pids)],root,os.environ.copy(),root/'command.log',timeout=.5)
            pid=int(pids.read_text())
            with self.assertRaises(ProcessLookupError):os.kill(pid,0)

    def test_termination_runs_the_coordinator_finally(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);pids=root/'child.pid'
            script='from common import install_interrupt_handlers;from normal import execute;import os,sys;from pathlib import Path;install_interrupt_handlers();execute([sys.executable,"-c",sys.argv[1],sys.argv[2]],Path(sys.argv[3]),os.environ.copy(),Path(sys.argv[3])/"command.log")'
            env={**os.environ,'PYTHONPATH':str(Path(__file__).resolve().parents[1])}
            process=subprocess.Popen([sys.executable,'-c',script,CHILD,str(pids),str(root)],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            try:
                deadline=time.monotonic()+5
                while not pids.exists() and time.monotonic()<deadline:time.sleep(.01)
                self.assertTrue(pids.exists())
                process.send_signal(signal.SIGTERM)
                process.wait(timeout=8)
                with self.assertRaises(ProcessLookupError):os.kill(int(pids.read_text()),0)
            finally:
                if process.poll() is None:process.kill();process.wait()


if __name__=='__main__':unittest.main()
