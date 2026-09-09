"""Regression tests for watchdogs, inherited handles, isolation and suite routing."""
import os
import contextlib
import io
import json
import signal
import time
from pathlib import Path
import subprocess
import socket
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

import test_runner as runner


class RunnerTests(unittest.TestCase):
    def setUp(self):
        runner.STOP.clear()
        runner.RUN_DEADLINE = float("inf")
        runner.RUN_EXPIRED = False
        for signo in (signal.SIGINT, signal.SIGTERM):
            self.addCleanup(signal.signal, signo, signal.getsignal(signo))
        self.temp = tempfile.TemporaryDirectory(prefix="sb-runner-selftest-", dir="/tmp")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.log = self.directory / "process.log"

    def run_python(self, source, timeout=2):
        return runner.run_process([sys.executable, "-c", source], env=os.environ.copy(),
                                  cwd=self.directory, log=self.log, timeout=timeout)

    def test_success_and_both_output_streams(self):
        rc, expired, _ = self.run_python("import sys; print('stdout'); print('stderr', file=sys.stderr)")
        self.assertEqual(rc, 0)
        self.assertFalse(expired)
        self.assertIn("stdout", self.log.read_text())
        self.assertIn("stderr", self.log.read_text())

    def test_nonzero_is_not_hidden_by_logging(self):
        rc, expired, _ = self.run_python("raise SystemExit(7)")
        self.assertEqual(rc, 7)
        self.assertFalse(expired)

    def test_timeout_is_bounded(self):
        rc, expired, elapsed = self.run_python("import time; time.sleep(30)", timeout=0.1)
        self.assertTrue(expired)
        self.assertNotEqual(rc, 0)
        self.assertLess(elapsed, 1)

    def test_inherited_stdout_cannot_hold_runner_open(self):
        source = "import subprocess, sys; subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)']); print('parent finished')"
        rc, expired, elapsed = self.run_python(source)
        self.assertEqual(rc, 0)
        self.assertFalse(expired)
        self.assertLess(elapsed, 1)
        self.assertIn("parent finished", self.log.read_text())

    def test_cancel_does_not_wait_for_timeout(self):
        timer = threading.Timer(0.1, runner.STOP.set)
        timer.start()
        self.addCleanup(timer.cancel)
        _, expired, elapsed = self.run_python("import time; time.sleep(30)")
        self.assertTrue(expired)
        self.assertLess(elapsed, 1)

    def test_timeout_preserves_unrelated_process(self):
        sentinel = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"], start_new_session=True)
        try:
            self.run_python("import time; time.sleep(30)", timeout=0.1)
            self.assertIsNone(sentinel.poll())
        finally:
            sentinel.kill()
            sentinel.wait()

    def test_isolation_does_not_modify_parent(self):
        old_home = os.environ.get("HOME")
        env = runner.isolated_env(self.directory)
        self.assertEqual(os.environ.get("HOME"), old_home)
        self.assertNotEqual(env["HOME"], old_home)
        self.assertEqual(env["SHELL"], "/bin/sh")
        self.assertNotIn("SB_TMUX_DESKTOP_ACCEPTANCE", env)
        self.assertNotIn("TMUX", env)
        self.assertTrue(Path(env["XDG_RUNTIME_DIR"]).is_dir())

    def test_cleanup_only_visits_owned_sockets_even_when_cancelled(self):
        resource = self.directory / "owned"
        resource.mkdir()
        native = socket.socket(socket.AF_UNIX)
        self.addCleanup(native.close)
        native.bind(str(resource / "socket"))
        outside_dir = tempfile.TemporaryDirectory(prefix="sb-sentinel-", dir="/tmp")
        self.addCleanup(outside_dir.cleanup)
        outside = socket.socket(socket.AF_UNIX)
        self.addCleanup(outside.close)
        outside.bind(str(Path(outside_dir.name) / "socket"))
        (resource / "foreign").symlink_to(outside_dir.name, target_is_directory=True)
        calls = []
        def record(argv, **kwargs):
            calls.append((argv, kwargs))
            return 0, False, 0.01
        runner.STOP.set()
        with patch.object(runner, "run_process", side_effect=record):
            errors = runner.cleanup_resources(resource, {}, None, self.directory, 0)
        self.assertFalse(errors)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][0], ["tmux", "-N", "-S", str(resource / "socket"), "kill-server"])
        self.assertFalse(calls[0][1]["cancel_on_stop"])

    def test_failed_cleanup_is_not_reported_as_success(self):
        native = socket.socket(socket.AF_UNIX)
        self.addCleanup(native.close)
        native.bind(str(self.directory / "socket"))
        with patch.object(runner, "run_process", return_value=(-9, True, 2)):
            errors = runner.cleanup_resources(self.directory, {}, None, self.directory, 0)
        self.assertEqual(len(errors), 1)

    def test_whole_run_deadline_cancels_running_and_queued_cases(self):
        executable = self.directory / "fake_test"
        executable.write_text(f"#!{sys.executable}\nimport sys, time\n"
                              "if '--list' in sys.argv:\n"
                              "    print('\\n'.join(f'case_{i}: test' for i in range(10)))\n"
                              "else:\n    time.sleep(10)\n")
        executable.chmod(0o700)
        real_run = runner.run_process
        def build_or_run(argv, **kwargs):
            if argv[0] == "cargo":
                Path(kwargs["log"]).write_text(json.dumps(dict(reason="compiler-artifact",
                    executable=str(executable), target=dict(name="fake"), profile=dict(test=True))) + "\n")
                return 0, False, 0
            if "--list" in argv:
                Path(kwargs["log"]).write_text("\n".join(f"case_{i}: test" for i in range(10)))
                return 0, False, 0
            return real_run(argv, **kwargs)
        start = time.monotonic()
        with patch.object(runner, "ROOT", self.directory), patch.object(runner, "run_process", side_effect=build_or_run), contextlib.redirect_stdout(io.StringIO()):
            rc = runner.main(["--suite", "unit", "--jobs", "1", "--timeout", "10", "--run-timeout", "0.3"])
        self.assertEqual(rc, 124)
        self.assertLess(time.monotonic() - start, 2)
        report = json.loads(next(self.directory.glob("target/test_reports/*/results.json")).read_text())
        self.assertTrue(report["run_timeout"])
        self.assertEqual(len(report["results"]), 10)
        self.assertEqual(sum(r["status"] == "TIMEOUT" for r in report["results"]), 1)
        self.assertEqual(sum(r["status"] == "CANCELLED" for r in report["results"]), 9)

    def test_whole_run_deadline_also_limits_build(self):
        real_run = runner.run_process
        def slow_build(argv, **kwargs):
            return real_run([sys.executable, "-c", "import time; time.sleep(10)"], **kwargs)
        start = time.monotonic()
        with patch.object(runner, "ROOT", self.directory), patch.object(runner, "run_process", side_effect=slow_build), contextlib.redirect_stdout(io.StringIO()):
            rc = runner.main(["--build-timeout", "10", "--run-timeout", "0.1"])
        self.assertEqual(rc, 124)
        self.assertLess(time.monotonic() - start, 1)

    def test_fast_filter_is_explicit_not_full_suite(self):
        self.assertTrue(runner.select_case("fast", "e2e", "terminology::test_tmux_terminology_workflow", ""))
        self.assertTrue(runner.select_case("fast", "e2e", "harness::idle_read", ""))
        self.assertFalse(runner.select_case("fast", "e2e", "old_workflow", ""))
        self.assertTrue(runner.select_case("full", "e2e", "old_workflow", ""))
        self.assertFalse(runner.select_case("full", "e2e", "old_workflow", "does_not_exist"))
        self.assertEqual(runner.build_args("full"), ["--tests"])


if __name__ == "__main__":
    unittest.main()
