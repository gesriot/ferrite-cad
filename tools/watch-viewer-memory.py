#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""macOS-only, independent memory guard for ONE owned viewer/test process.

Starts the child stopped, arms measurement, then lets it exec the command. Never
attaches to/kills an existing application. JSONL is evidence, not a leak detector.
Exit 125 means an aborted experiment (limit, pressure, missing measurement, time).
"""

import argparse
import ctypes
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


class Usage(ctypes.Structure):
    # Darwin rusage_info_v0, sys/resource.h. These are overlapping metrics;
    # physical footprint already accounts for charged unified/GPU allocations.
    _fields_ = [("uuid", ctypes.c_ubyte * 16)] + [
        (name, ctypes.c_uint64)
        for name in (
            "user_time", "system_time", "pkg_idle_wkups", "interrupt_wkups",
            "pageins", "wired_size", "resident_size", "phys_footprint",
            "proc_start_abstime", "proc_exit_abstime",
        )
    ]


class Swap(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("total", "avail", "used")]
    _fields_ += [("pagesize", ctypes.c_int32), ("encrypted", ctypes.c_int32)]


class ExperimentStopped(BaseException):
    """Must cross subprocess/measurement code that retries InterruptedError."""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--limit-mib", type=float, default=1536)
    parser.add_argument("--seconds", type=float, default=1800)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if (sys.platform != "darwin" or not command
            or not all(math.isfinite(v) and v > 0 for v in (args.limit_mib, args.seconds))):
        parser.error("requires macOS, a command, and finite positive limits")
    args.log.parent.mkdir(parents=True, exist_ok=True)
    # Refuse overwriting a previous measurement or attaching to somebody's PID.
    log = args.log.open("x", buffering=1)
    started = time.monotonic()

    def record(event, **facts):
        log.write(json.dumps(dict(event=event, elapsed=time.monotonic() - started,
                                 time=time.time(), **facts)) + "\n")

    lib = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
    lib.proc_pid_rusage.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    lib.sysctlbyname.argtypes = [ctypes.c_char_p, ctypes.c_void_p,
                               ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p,
                               ctypes.c_size_t]

    def usage(pid):
        result = Usage()
        if lib.proc_pid_rusage(pid, 0, ctypes.byref(result)):
            raise OSError(ctypes.get_errno(), "proc_pid_rusage", str(pid))
        return dict(pid=pid, start=result.proc_start_abstime,
                    rss=result.resident_size, footprint=result.phys_footprint)

    def sysctl(name, result):
        size = ctypes.c_size_t(ctypes.sizeof(result))
        if lib.sysctlbyname(name.encode(), ctypes.byref(result), ctypes.byref(size), None, 0):
            raise OSError(ctypes.get_errno(), "sysctlbyname", name)
        return result

    def system():
        pressure = sysctl("kern.memorystatus_vm_pressure_level", ctypes.c_int()).value
        swap = sysctl("vm.swapusage", Swap())
        disk = os.statvfs(args.log.parent)
        return dict(pressure=pressure, swap_used=swap.used, swap_total=swap.total,
                    disk_available=disk.f_bavail * disk.f_frsize)

    initial = system()  # Refuse before launching anything if safety is unavailable.
    record("preflight", **initial, limit_bytes=int(args.limit_mib * 1024**2), command=command)
    if initial["pressure"] != 1 or initial["disk_available"] < 4 * 1024**3:
        record("refused", reason="initial pressure or less than 4 GiB free disk")
        return 125
    processes = subprocess.check_output(["/bin/ps", "-axo", "comm="], text=True, timeout=2)
    if any(Path(row.strip()).name.startswith(("ferritecad-viewer", "ferritecad_viewer-"))
           for row in processes.splitlines()):
        record("refused", reason="an existing viewer/test process is running; leave it alone")
        return 125

    # The shim stops AFTER Python exec, so Popen's exec-error pipe has closed.
    # preexec_fn(SIGSTOP) would deadlock Popen before the guard could be armed.
    shim = ("import os,signal,sys; os.kill(os.getpid(),signal.SIGSTOP); "
            "signal.pthread_sigmask(signal.SIG_UNBLOCK,"
            "[signal.SIGINT,signal.SIGTERM,signal.SIGHUP]); "
            "os.execvp(sys.argv[1],sys.argv[1:])")
    with args.log.with_suffix(".stdout").open("xb") as stdout, \
            args.log.with_suffix(".stderr").open("xb") as stderr:
        child = None
        owned = {}
        reason = None
        watched_signals = (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)
        handlers = {}

        def interrupted(signum, _frame):
            raise ExperimentStopped(f"received {signal.Signals(signum).name}")

        try:
            for sig in watched_signals:
                handlers[sig] = signal.signal(sig, interrupted)
            # Do not lose a newly spawned child if termination arrives between
            # fork/exec and Popen returning its PID. The stopped shim restores
            # these signals before starting the requested command.
            previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, watched_signals)
            try:
                child = subprocess.Popen([sys.executable, "-c", shim, *command],
                                         stdout=stdout, stderr=stderr, start_new_session=True)
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
            _, status = os.waitpid(child.pid, os.WUNTRACED)
            if not os.WIFSTOPPED(status):
                raise RuntimeError("child did not wait for guard")
            owned[child.pid] = usage(child.pid)["start"]
            record("armed", pid=child.pid, start=owned[child.pid])
            args.log.with_suffix(".pid").write_text(str(child.pid) + "\n")
            os.kill(child.pid, signal.SIGCONT)
            next_compression = 0.0
            while child.poll() is None:
                # Descendants only, discovered from parent relationships. No
                # global name match is ever used to terminate anything.
                rows = subprocess.check_output(["/bin/ps", "-axo", "pid=,ppid="],
                                               text=True, timeout=2)
                parents = dict(map(int, row.split()) for row in rows.splitlines())
                descendants = {child.pid}
                while True:
                    expanded = descendants | {p for p, parent in parents.items() if parent in descendants}
                    if expanded == descendants:
                        break
                    descendants = expanded
                facts = []
                for pid in sorted(descendants | owned.keys()):
                    try:
                        fact = usage(pid)
                    except OSError:
                        if pid == child.pid and child.poll() is None:
                            raise
                        continue  # An exited descendant is no longer a charge.
                    if pid in owned and owned[pid] != fact["start"]:
                        continue  # Never follow a reused PID.
                    owned[pid] = fact["start"]
                    facts.append(fact)
                state = system()
                total = sum(fact["footprint"] for fact in facts)
                record("sample", processes=facts, footprint_sum=total, **state)
                if total > args.limit_mib * 1024**2:
                    reason = "physical footprint limit"
                elif state["pressure"] != 1:
                    reason = "system memory pressure"
                elif state["disk_available"] < 4 * 1024**3:
                    reason = "less than 4 GiB free disk"
                elif time.monotonic() - started > args.seconds:
                    reason = "time limit"
                elif args.log.with_suffix(".stop").exists():
                    reason = "requested experiment stop"
                if reason:
                    break
                if time.monotonic() >= next_compression:
                    # CMPRS is an actual compressed-memory measurement, not
                    # footprint minus RSS. Rounded top units are kept verbatim.
                    result = subprocess.run(["/usr/bin/top", "-l", "1", "-pid", str(child.pid),
                                             "-stats", "pid,mem,cmprs", "-n", "1"],
                                            capture_output=True, text=True, timeout=2)
                    record("compression", pid=child.pid, status=result.returncode,
                           top=result.stdout, error=result.stderr)
                    next_compression = time.monotonic() + 10
                time.sleep(0.5)
        except BaseException as error:
            reason = f"guard failure: {error}"
        finally:
            # Repeated termination signals must not interrupt cleanup. SIGKILL
            # cannot be handled; this sampler is not an OS resource limit.
            for sig in handlers:
                signal.signal(sig, signal.SIG_IGN)
            if not reason:
                for pid, birth in owned.items():
                    if pid == child.pid:
                        continue
                    try:
                        if usage(pid)["start"] == birth:
                            reason = "owned descendant outlived the test process"
                    except OSError:
                        pass
            if reason:
                # A full disk or failed log is itself a reason to stop. Never
                # put fallible reporting on the only path to child cleanup.
                try:
                    record("aborted", reason=reason)
                except OSError:
                    pass
                # Our child remains unreaped until poll/wait; descendants must
                # still have the observed birth identity before each signal.
                for sig in (signal.SIGTERM, signal.SIGKILL):
                    # Also covers a failed first measurement, before we learned
                    # its birth identity. Popen still owns this unreaped child.
                    if child is not None and child.poll() is None:
                        child.send_signal(sig)
                    for pid, birth in owned.items():
                        try:
                            if usage(pid)["start"] == birth:
                                os.kill(pid, sig)
                        except (OSError, ProcessLookupError):
                            pass
                    if sig == signal.SIGTERM:
                        time.sleep(1)
            code = child.wait() if child is not None else 125
            try:
                record("exit", pid=child.pid if child is not None else None,
                       returncode=code, aborted=reason is not None)
            except OSError:
                reason = reason or "could not record experiment exit"
            finally:
                for sig, handler in handlers.items():
                    signal.signal(sig, handler)
    return 125 if reason else (code if code >= 0 else 128 - code)


if __name__ == "__main__":
    sys.exit(main())
