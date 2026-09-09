#!/usr/bin/env python3
"""Bounded, process-isolated Rust tests; Python stdlib only, macOS/Linux.

Build once, discover libtest cases, then run each in a fresh process with its
own HOME/XDG directories. No implicit retries or ignored-failure suppression.
"""
import argparse
import concurrent.futures
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
STOP = threading.Event()
RUN_DEADLINE = float("inf")
RUN_EXPIRED = False


def stop_requested():
    global RUN_EXPIRED
    # Per-case limits alone allowed hundreds of stuck cases to accumulate into
    # hours. One shared wall-clock budget covers build, discovery and all cases.
    if time.monotonic() >= RUN_DEADLINE:
        RUN_EXPIRED = True
        STOP.set()
    return STOP.is_set()


def run_process(argv, *, env, cwd, log, timeout, heartbeat=None, cancel_on_stop=True):
    """Never wait for pipe EOF: daemonized children can inherit stdout forever."""
    start = time.monotonic()
    timed_out = False
    with open(log, "wb") as stream:
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdout=stream,
                                 stderr=subprocess.STDOUT, start_new_session=True)
        next_heartbeat = start + 2
        try:
            while child.poll() is None:
                now = time.monotonic()
                if (cancel_on_stop and stop_requested()) or now - start >= timeout:
                    timed_out = True
                    break
                if heartbeat and now >= next_heartbeat:
                    heartbeat(now - start)
                    next_heartbeat = now + 2
                time.sleep(0.01)
        finally:
            # Only our new process group, never pkill/name matching or a user's server.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            # Even reaping must be bounded; the outer tool deadline is the final
            # safeguard if the OS cannot terminate/reap a process promptly.
            try:
                child.wait(timeout=1)
            except subprocess.TimeoutExpired:
                timed_out = True
    return child.returncode if child.returncode is not None else -9, timed_out, time.monotonic() - start


def isolated_env(base):
    env = os.environ.copy()
    for key in ("TMUX", "TMUX_PANE", "SB_TMUX_TEST_BINARY", "SB_TMUX_DESKTOP_ACCEPTANCE",
                "ENV", "BASH_ENV", "CLAUDECODE"):
        env.pop(key, None)
    for key, folder in (("HOME", "home"), ("XDG_DATA_HOME", "data"),
                        ("XDG_RUNTIME_DIR", "runtime"), ("ZDOTDIR", "home")):
        path = base / folder
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
        env[key] = str(path)
    # Personal shell startup hooks are neither deterministic nor part of most tests.
    env.update(SHELL="/bin/sh", TERM="xterm-256color", RUST_BACKTRACE="short",
               CARGO_MANIFEST_DIR=str(ROOT), SB_TEST_RESOURCE_ROOT=str(base))
    return env


def cleanup_resources(base, env, cli, report, index):
    """Detached tmux/legacy servers escape process groups. Only visit owned sockets."""
    errors = []
    cleanup_deadline = time.monotonic() + 5
    for number, socket in enumerate(base.rglob("*")):
        if socket.is_symlink() or not socket.is_socket():
            continue
        if not socket.resolve().is_relative_to(base.resolve()):
            continue
        cleanup_env = env.copy()
        if socket.name == "socket":
            command = ["tmux", "-N", "-S", str(socket), "kill-server"]
        elif socket.name == "daemon.sock" and cli:
            cleanup_env["XDG_RUNTIME_DIR"] = str(socket.parent.parent)
            command = [cli, "shutdown"]
        else:
            continue
        remaining = cleanup_deadline - time.monotonic()
        if remaining <= 0:
            errors.append("Cleanup exceeded five-second budget; resources retained")
            break
        log = report / f"{index:04d}_cleanup_{number}.log"
        rc, expired, _ = run_process(command, env=cleanup_env, cwd=ROOT, log=log,
                                     timeout=min(2, remaining), cancel_on_stop=False)
        if expired or rc:
            errors.append(str(log))
    return errors


def build_args(suite):
    if suite in ("full", "fast"):
        return ["--tests"]
    if suite == "unit":
        return ["--lib", "--bin", "sb", "--test", "terminology_tests"]
    if suite == "e2e":
        return ["--test", "e2e"]
    if suite == "desktop":
        return ["--test", "tmux_sidebar"]
    return [item for target in ("tmux_sidebar", "tmux_chooser", "tmux_inspector", "tmux_legacy_launch")
            for item in ("--test", target)]


def select_case(suite, target, name, pattern):
    if pattern and pattern not in f"{target}::{name}":
        return False
    if suite == "desktop":
        return name == "editor_logs_native_mouse_copy_and_restart"
    if suite == "fast" and target == "e2e":
        return name in ("terminology::test_tmux_terminology_workflow", "test_command_throughput_in_tui") or name.startswith("harness::")
    return True


def main(argv=None):
    global RUN_DEADLINE, RUN_EXPIRED
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=["fast", "unit", "tmux", "e2e", "full", "desktop"], default="fast")
    parser.add_argument("--filter", default="", help="substring of target::test; no matches is an error")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=30, help="hard seconds per test (desktop: use 60)")
    parser.add_argument("--build-timeout", type=float, default=60)
    parser.add_argument("--run-timeout", type=float, default=60,
                        help="seconds for build + discovery + all tests; then up to 5s cleanup per active worker")
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument("--verbose", action="store_true", help="print every passing/ignored case")
    args = parser.parse_args(argv)
    if os.name != "posix":
        parser.error("This runner requires macOS/Linux (Windows: use WSL)")
    if not all(0 < value < float("inf") for value in
               (args.jobs, args.timeout, args.build_timeout, args.run_timeout, args.repeat)):
        parser.error("jobs, timeouts and repeat must be positive")
    if args.suite == "desktop" and (sys.platform != "darwin" or args.jobs != 1):
        parser.error("desktop requires macOS and --jobs 1; GUI work is never part of fast/full")

    STOP.clear()
    RUN_EXPIRED = False
    RUN_DEADLINE = time.monotonic() + args.run_timeout
    # Interrupts must stop workers, not make ThreadPoolExecutor wait for every
    # queued test. The partial report explicitly records cancellation as failure.
    for signo in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signo, lambda _signo, _frame: STOP.set())

    report = ROOT / "target" / "test_reports" / f"{time.strftime('%Y%m%d_%H%M%S')}_{os.getpid()}"
    report.mkdir(parents=True)
    started = time.monotonic()
    print(f"BUILD {args.suite}; whole-run limit {args.run_timeout:g}s; logs: {report}", flush=True)
    build_log = report / "build.log"
    rc, expired, _ = run_process(
        ["cargo", "test", "--locked", "--no-run", "--message-format=json", *build_args(args.suite)],
        env=os.environ.copy(), cwd=ROOT, log=build_log, timeout=args.build_timeout,
        heartbeat=lambda elapsed: print(f"BUILD still running ({elapsed:.0f}s); {build_log}", flush=True))
    if rc or expired:
        print(f"BUILD {'TIMEOUT' if expired else 'FAILED'}: {build_log}", flush=True)
        print(build_log.read_text(errors="replace")[-8000:])
        if RUN_EXPIRED:
            print("RUN TIMEOUT: whole-run budget exhausted during build", flush=True)
        return 124 if RUN_EXPIRED else 1
    artifacts = []
    cli = None
    for line in build_log.read_text().splitlines():
        try:
            obj = json.loads(line)
        except ValueError:
            continue
        if obj.get("reason") == "compiler-artifact" and obj.get("executable"):
            if obj.get("profile", {}).get("test"):
                artifacts.append((obj["target"]["name"], obj["executable"]))
            elif obj["target"]["name"] == "sb":
                cli = obj["executable"]
    cases = []
    for target, executable in sorted(set(artifacts)):
        listing = report / f"{target}_list.log"
        rc, expired, _ = run_process([executable, "--list", "--format", "terse"],
                                     env=os.environ.copy(), cwd=ROOT, log=listing, timeout=5)
        if rc or expired:
            print(f"DISCOVERY {'RUN TIMEOUT' if RUN_EXPIRED else 'FAILED'}: {listing}")
            return 124 if RUN_EXPIRED else 1
        for line in listing.read_text().splitlines():
            if line.endswith(": test") and select_case(args.suite, target, line[:-6], args.filter):
                cases.append((target, executable, line[:-6]))
    if not cases:
        print("ERROR: no tests matched (not a passing run)", flush=True)
        return 1
    # Start historically slow cases first so a long PTY case does not begin only
    # after hundreds of tiny unit tests. Timings affect order, never test selection.
    timings = {}
    for previous in sorted(report.parent.glob("*/results.json"), reverse=True)[:10]:
        try:
            for result in json.loads(previous.read_text())["results"]:
                if result["status"] == "PASS":
                    timings.setdefault(result["test"], result["seconds"])
        except (OSError, ValueError, KeyError):
            continue
    cases *= args.repeat
    cases.sort(key=lambda c: timings.get(f"{c[0]}::{c[2]}", 0.01 if c[0] in ("sb", "sidebar_tui") else 1), reverse=True)
    print(f"RUN {len(cases)} cases, {args.jobs} workers, {args.timeout:g}s hard limit each", flush=True)

    def execute(index, case):
        target, executable, name = case
        if stop_requested():
            return dict(test=f"{target}::{name}", status="CANCELLED", seconds=0)
        log = report / f"{index:04d}_{target}.log"
        # Short /tmp paths also avoid the macOS Unix-socket path-length limit.
        directory = Path(tempfile.mkdtemp(prefix="sb-check-", dir="/tmp"))
        env = isolated_env(directory)
        if args.suite == "desktop":
            env["SB_TMUX_DESKTOP_ACCEPTANCE"] = "1"
        cleanup_errors = []
        try:
            rc, expired, duration = run_process(
                [executable, "--exact", name, "--nocapture", "--test-threads=1"],
                env=env, cwd=ROOT, log=log, timeout=args.timeout)
        finally:
            cleanup_errors = cleanup_resources(directory, env, cli, report, index)
            if not cleanup_errors:
                shutil.rmtree(directory)
        output = log.read_text(errors="replace")
        # Check libtest's summary too: an incorrect exact filter must not pass silently.
        status = "TIMEOUT" if expired else "FAIL" if rc else (
            "PASS" if "1 passed; 0 failed" in output else
            "IGNORED" if "0 passed; 0 failed; 1 ignored" in output else "NO_TEST")
        result = dict(test=f"{target}::{name}", status=status, seconds=round(duration, 3), log=str(log))
        if cleanup_errors:
            result.update(status="CLEANUP_FAILED", cleanup_logs=cleanup_errors, retained_resources=str(directory))
        return result

    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        pending = {pool.submit(execute, i, case): (case, time.monotonic()) for i, case in enumerate(cases)}
        last_progress = time.monotonic()
        while pending:
            done, _ = concurrent.futures.wait(pending, timeout=1, return_when=concurrent.futures.FIRST_COMPLETED)
            for future in done:
                case, _ = pending.pop(future)
                try:
                    result = future.result()
                except Exception as error:
                    result = dict(test=f"{case[0]}::{case[2]}", status="ERROR", seconds=0, error=str(error))
                results.append(result)
                if args.verbose or result["seconds"] >= 0.25 or result["status"] not in ("PASS", "IGNORED") or len(results) % 25 == 0 or len(results) == len(cases):
                    print(f"[{len(results)}/{len(cases)}] {result['status']:7s} {result['seconds']:6.2f}s {result['test']}", flush=True)
                if result["status"] not in ("PASS", "IGNORED"):
                    print(f"  {result.get('log', result.get('error'))}", flush=True)
                last_progress = time.monotonic()
            if time.monotonic() - last_progress >= 2:
                running = [f"{case[0]}::{case[2]}" for future, (case, _) in pending.items() if future.running()]
                print("RUNNING " + ", ".join(running), flush=True)
                last_progress = time.monotonic()
    elapsed = time.monotonic() - started
    failed = [r for r in results if r["status"] not in ("PASS", "IGNORED")]
    (report / "results.json").write_text(json.dumps(dict(seconds=round(elapsed, 3), run_timeout=RUN_EXPIRED, results=results), indent=2))
    print(f"SUMMARY: {sum(r['status'] == 'PASS' for r in results)} passed, "
          f"{sum(r['status'] == 'IGNORED' for r in results)} ignored, {len(failed)} failed; {elapsed:.2f}s total", flush=True)
    print("SLOWEST: " + ", ".join(f"{r['test']} {r['seconds']:.2f}s" for r in sorted(results, key=lambda r: r['seconds'], reverse=True)[:5]))
    print(f"REPORT: {report / 'results.json'}")
    if RUN_EXPIRED:
        print("RUN TIMEOUT: whole-run budget exhausted; unfinished cases are not passes", flush=True)
        return 124
    return 130 if STOP.is_set() else int(bool(failed))


if __name__ == "__main__":
    sys.exit(main())
