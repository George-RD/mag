"""Keep an exiting process-group race separate from genuine cleanup failures."""
import errno
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from benches.memory_intelligence import capture


class ProcessCleanupTests(unittest.TestCase):
    def test_reaps_exited_leader_and_retries_after_group_permission_race(self):
        process = mock.Mock(pid=43210)
        events = []
        process.poll.side_effect = lambda: events.append("reap") or 0

        def kill_group(pid, signum):
            events.append("signal")
            if len(events) == 1:
                raise PermissionError(errno.EPERM, "exiting group")
            raise ProcessLookupError(errno.ESRCH, "reaped group")

        with mock.patch.object(capture.os, "killpg", side_effect=kill_group) as killpg:
            capture._stop_group(process)
        self.assertEqual(events, ["signal", "reap", "signal"])
        self.assertEqual(killpg.call_args_list, [mock.call(43210, signal.SIGKILL)] * 2)
        process.wait.assert_called_once_with()

    def test_retry_still_signals_descendants_after_reaping_leader(self):
        process = mock.Mock(pid=43210)
        process.poll.return_value = 0
        with mock.patch.object(capture.os, "killpg", side_effect=[PermissionError(errno.EPERM, "race"), None]) as killpg:
            capture._stop_group(process)
        self.assertEqual(killpg.call_count, 2)
        process.wait.assert_called_once_with()

    def test_live_leader_permission_failure_does_not_block_in_wait(self):
        process = mock.Mock(pid=43210)
        process.poll.return_value = None
        error = PermissionError(errno.EPERM, "live process")
        with mock.patch.object(capture.os, "killpg", side_effect=error) as killpg:
            with self.assertRaises(PermissionError) as raised:
                capture._stop_group(process)
        self.assertIs(raised.exception, error)
        killpg.assert_called_once_with(43210, signal.SIGKILL)
        process.wait.assert_not_called()

    def test_repeated_permission_failure_is_not_treated_as_success(self):
        process = mock.Mock(pid=43210)
        process.poll.return_value = 0
        error = PermissionError(errno.EPERM, "remaining group denied")
        with mock.patch.object(capture.os, "killpg", side_effect=error) as killpg:
            with self.assertRaises(PermissionError) as raised:
                capture._stop_group(process)
        self.assertIs(raised.exception, error)
        self.assertEqual(killpg.call_count, 2)
        process.wait.assert_not_called()

    def test_normal_and_absent_groups_keep_existing_wait_semantics(self):
        for effect in (None, ProcessLookupError(errno.ESRCH, "gone")):
            with self.subTest(effect=effect):
                process = mock.Mock(pid=43210)
                with mock.patch.object(capture.os, "killpg", side_effect=effect) as killpg:
                    capture._stop_group(process)
                killpg.assert_called_once_with(43210, signal.SIGKILL)
                process.wait.assert_called_once_with()
                process.poll.assert_not_called()

    def test_invoke_closes_pipes_even_when_group_cleanup_is_denied(self):
        started = []
        popen = subprocess.Popen
        stop = capture._stop_group

        def launch(*args, **kwargs):
            process = popen(*args, **kwargs)
            started.append(process)
            return process

        try:
            with tempfile.TemporaryDirectory() as cwd:
                with mock.patch.object(capture.subprocess, "Popen", side_effect=launch), mock.patch.object(
                    capture, "_stop_group", side_effect=PermissionError(errno.EPERM, "denied"),
                ):
                    with self.assertRaises(PermissionError):
                        capture._invoke([sys.executable, "-c", "import time; time.sleep(10)"], b"", cwd, 0.1, 1024)
                self.assertEqual(len(started), 1)
                self.assertTrue(all(stream.closed for stream in (started[0].stdin, started[0].stdout, started[0].stderr)))
        finally:
            # The injected failure must not leave our own real fixture running.
            for process in started:
                stop(process)
                for stream in (process.stdin, process.stdout, process.stderr):
                    stream.close()


if __name__ == "__main__":
    unittest.main()
