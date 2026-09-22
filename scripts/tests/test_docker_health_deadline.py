#!/usr/bin/env python3
"""Regression: a continuously arriving HTTP body must not evade the probe deadline."""
import http.server
import importlib.util
from pathlib import Path
import signal
import threading
import time
import unittest
from unittest.mock import patch
import urllib.request

SPEC = importlib.util.spec_from_file_location(
    "docker_health", Path(__file__).with_name("test_docker_health.py"))
HEALTH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HEALTH)


class ProbeDeadlineTests(unittest.TestCase):
    def setUp(self):
        self.stop = threading.Event()
        self.bytes_sent = 0
        fixture = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.connection.settimeout(1)
                slow = self.path == "/slow"
                body = b"x" * 100 if slow else b"deve-sub-web"
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                try:
                    for byte in body:
                        if slow and fixture.stop.wait(0.02):
                            break
                        self.wfile.write(bytes([byte]))
                        self.wfile.flush()
                        fixture.bytes_sent += 1
                except (OSError, ConnectionError):
                    pass

            def log_message(self, format, *args):
                pass

        self.server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        self.server.timeout = 0.05
        self.worker = threading.Thread(target=self.serve, daemon=True)
        self.worker.start()
        self.addCleanup(self.close_server)
        self.base = f"http://127.0.0.1:{self.server.server_port}"
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), HEALTH.NoRedirect())
        self.alarm_handler = lambda *_: self.fail("probe left an active alarm")
        previous = signal.signal(signal.SIGALRM, self.alarm_handler)
        self.addCleanup(signal.signal, signal.SIGALRM, previous)
        self.addCleanup(signal.setitimer, signal.ITIMER_REAL, 0)

    def serve(self):
        while not self.stop.is_set():
            self.server.handle_request()

    def close_server(self):
        self.stop.set()
        try:
            self.worker.join(timeout=2)
            self.assertFalse(self.worker.is_alive(), "fixture HTTP thread did not stop")
        finally:
            self.server.server_close()

    def assert_alarm_restored(self):
        self.assertIs(signal.getsignal(signal.SIGALRM), self.alarm_handler)
        self.assertEqual(signal.getitimer(signal.ITIMER_REAL), (0.0, 0.0))

    def test_continuous_body_obeys_total_deadline(self):
        started = time.monotonic()
        with patch.object(HEALTH, "HTTP_DEADLINE_SECONDS", 0.15):
            with self.assertRaisesRegex(HEALTH.SmokeFailure, "exceeded the 0.15-second deadline"):
                HEALTH.probe(self.opener, self.base + "/slow")
        elapsed = time.monotonic() - started
        self.assertGreaterEqual(elapsed, 0.14)
        self.assertLess(elapsed, 1, "probe ignored the wall deadline")
        self.assertGreaterEqual(self.bytes_sent, 2, "fixture must actively stream during the probe")
        self.assert_alarm_restored()

    def test_small_response_succeeds_and_restores_alarm(self):
        with patch.object(HEALTH, "HTTP_DEADLINE_SECONDS", 0.15):
            status, body = HEALTH.probe(self.opener, self.base + "/fast")
        self.assertEqual(status, 200)
        self.assertEqual(body, b"deve-sub-web")
        self.assert_alarm_restored()


if __name__ == "__main__":
    unittest.main()
