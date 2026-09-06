"""Exercise report calculations against a local HTTP fixture, not a scale benchmark."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest


class ProbeTests(unittest.TestCase):
    def run_probe(self, status):
        received = []
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                received.append((self.path, self.headers.get('Authorization')))
                self.rfile.read(int(self.headers['Content-Length']))
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                if status == 302:
                    self.send_header('Location', '/unexpected-target')
                self.end_headers()
                self.wfile.write(json.dumps({'results': [{'document_id': 'doc-a'}]}).encode())
            def log_message(self, *args):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / 'queries.json').write_text(json.dumps([{'q': 'Cobalt', 'expected_document_ids': ['doc-a', 'doc-b']}]))
                proc = subprocess.run([
                    'python3', str(Path(__file__).with_name('benchmark.py')),
                    '--url', f'http://127.0.0.1:{server.server_port}', '--kb', 'fixture-kb',
                    '--queries', str(root / 'queries.json'), '--repeats', '2', '--concurrency', '2',
                    '--output', str(root / 'report.json')
                ], env={**os.environ, 'ARCADIA_TOKEN': 'fixture-only'}, capture_output=True, text=True)
                report = json.loads((root / 'report.json').read_text())
                self.assertNotIn('fixture-only', proc.stdout + proc.stderr)
                return proc.returncode, report, received
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_counts_success_and_document_recall(self):
        code, report, received = self.run_probe(200)
        self.assertEqual(code, 0)
        self.assertEqual(report['successes'], 2)
        self.assertEqual(report['mean_document_recall_at_20'], 0.5)
        self.assertEqual(len(received), 2)

    def test_refuses_redirect_and_reports_failure(self):
        code, report, received = self.run_probe(302)
        self.assertEqual(code, 1)
        self.assertEqual(report['failures'], 2)
        self.assertIsNone(report['p50_ms'])
        self.assertTrue(all(path.endswith('/search') for path, _ in received))


if __name__ == '__main__':
    unittest.main()
