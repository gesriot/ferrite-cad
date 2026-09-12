#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Real macOS child-process regressions for the memory guard; no viewer or GPU."""

import ctypes
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPT = Path(__file__).with_name("watch-viewer-memory.py")
spec = importlib.util.spec_from_file_location("guard", SCRIPT)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)

# Only the measurement journal fails; stdout/stderr and real process metrics
# still work. This models a full disk without filling the user's filesystem.
FAIL_LOG = '''import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("guard", sys.argv[1])
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
real_open = pathlib.Path.open
class BrokenLog:
    def __init__(self, inner): self.inner = inner; self.samples = 0
    def write(self, text):
        if '"event": "sample"' in text: self.samples += 1
        if self.samples >= 3: raise OSError("injected measurement log failure")
        return self.inner.write(text)
def opened(path, *args, **kwargs):
    stream = real_open(path, *args, **kwargs)
    return BrokenLog(stream) if path.suffix == ".jsonl" and args == ("x",) else stream
pathlib.Path.open = opened
sys.argv = sys.argv[1:]
sys.exit(m.main())
'''


@unittest.skipUnless(sys.platform == "darwin", "the guard uses Darwin process metrics")
class GuardTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="ferrite-memory-guard-")
        self.addCleanup(self.directory.cleanup)
        self.log = Path(self.directory.name) / "events.jsonl"
        self.lib = ctypes.CDLL("/usr/lib/libSystem.B.dylib")
        self.lib.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
        self.owned = {}
        self.process = None
        self.control = subprocess.Popen(["/bin/sleep", "40"])
        self.addCleanup(self.cleanup)

    def alive(self, pid, birth):
        usage = guard.Usage()
        return (self.lib.proc_pid_rusage(pid, 0, ctypes.byref(usage)) == 0
                and usage.proc_start_abstime == birth and not usage.proc_exit_abstime)

    def events(self):
        rows = []
        if self.log.exists():
            for line in self.log.read_text().splitlines():
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # The writer may be between write and LF.
        for row in rows:
            if row["event"] == "armed":
                self.owned[row["pid"]] = row["start"]
            for fact in row.get("processes", []):
                self.owned[fact["pid"]] = fact["start"]
        return rows

    def cleanup(self):
        if self.process is not None and self.process.poll() is None:
            self.process.kill()
            self.process.wait()
        self.events()
        for pid, birth in self.owned.items():
            if self.alive(pid, birth):
                os.kill(pid, signal.SIGKILL)
        self.control.terminate()
        self.control.wait()

    def start(self, command, *, broken_log=False, limit="1536"):
        args = [sys.executable, str(SCRIPT), "--log", str(self.log),
                "--seconds", "20", "--limit-mib", limit, "--", *command]
        if broken_log:
            args = [sys.executable, "-c", FAIL_LOG, *args[1:]]
        self.process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def await_sample(self):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if any(r["event"] == "sample" for r in self.events()):
                return
            if self.process.poll() is not None:
                self.fail(f"guard did not arm: {self.process.communicate()}, {self.events()}")
            time.sleep(0.025)
        self.fail("guard did not measure its owned child")

    def finished(self, expected):
        out, err = self.process.communicate(timeout=10)
        self.assertEqual(self.process.returncode, expected, (out, err, self.events()))
        self.events()
        self.assertFalse([pid for pid, birth in self.owned.items() if self.alive(pid, birth)])
        self.assertIsNone(self.control.poll(), "independent process was stopped")

    def test_normal_child(self):
        self.start(["/bin/sleep", "1"])
        self.finished(0)
        self.assertTrue(any(r["event"] == "sample" for r in self.events()))

    def test_termination_signals_clean_up(self):
        for sig in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT):
            with self.subTest(signal=sig):
                self.log = self.log.with_name(f"signal-{sig}.jsonl")
                self.start(["/bin/sleep", "30"])
                self.await_sample()
                self.process.send_signal(sig)
                self.finished(125)

    def test_failed_log_still_stops_child(self):
        self.start(["/bin/sleep", "30"], broken_log=True)
        self.finished(125)
        self.assertTrue(self.owned, "a preflight refusal is not a cleanup test")

    def test_footprint_limit(self):
        self.start([sys.executable, "-c", "import time; data=bytearray(64*1024**2); time.sleep(30)"],
                   limit="32")
        self.finished(125)
        self.assertTrue(any(r.get("reason") == "physical footprint limit" for r in self.events()))

    def test_nonfinite_limits_never_launch(self):
        for value in ("nan", "inf"):
            with self.subTest(value=value):
                self.log = self.log.with_name(f"limit-{value}.jsonl")
                self.start(["/bin/sleep", "30"], limit=value)
                self.finished(2)
                self.assertFalse(self.log.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
