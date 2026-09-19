"""Offline regression tests: python3 scripts/testing/test_query_peers.py.

Requires bash and jq. Runs the real script with only curl replaced.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "query_peers.sh"


class QueryPeersTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(shutil.which("bash"), "bash is required")
        self.assertIsNotNone(shutil.which("jq"), "jq is required")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        curl = self.root / "curl"
        curl.write_text(
            f"#!{sys.executable}\n"
            "import json, os, sys\n"
            "from urllib.parse import urlparse\n"
            "host = urlparse(sys.argv[-1]).hostname\n"
            "with open(os.environ['QUERY_TEST_CALLS'], 'a') as f:\n"
            "    f.write(host + '\\n')\n"
            "if host == 'fail.invalid':\n"
            "    sys.exit(7)\n"
            "count = int(os.environ['QUERY_TEST_PEERS'])\n"
            "print(json.dumps({'jsonrpc': '2.0', 'id': 1, 'result': {\n"
            "    'peer_id': host, 'peer_count': count,\n"
            "    'external_addresses': [], 'listen_addresses': [],\n"
            "    'connected_peers': []}}))\n"
        )
        curl.chmod(0o755)

    def run_query(self, hosts, *, json_mode=False, peers=1):
        calls = self.root / "calls"
        calls.write_text("")
        env = dict(os.environ)
        env.update(
            PATH=str(self.root) + os.pathsep + env.get("PATH", ""),
            QUERY_TEST_CALLS=str(calls),
            QUERY_TEST_PEERS=str(peers),
        )
        result = subprocess.run(
            ["bash", str(SCRIPT), *(["-j"] if json_mode else []), *hosts],
            env=env, text=True, capture_output=True, timeout=15,
        )
        return result, calls.read_text().splitlines()

    def test_single_json_success(self):
        result, calls = self.run_query(["one.invalid"], json_mode=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["peer_id"], "one.invalid")
        self.assertEqual(calls, ["one.invalid"])

    def test_single_text_success(self):
        result, _ = self.run_query(["one.invalid"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("[SUCCESS]", result.stdout)
        self.assertIn("one.invalid - Peer ID:", result.stdout)

    def test_zero_peers_success(self):
        for json_mode in (False, True):
            with self.subTest(json_mode=json_mode):
                result, _ = self.run_query(["one.invalid"], json_mode=json_mode, peers=0)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_multiple_hosts_continue(self):
        for json_mode in (False, True):
            with self.subTest(json_mode=json_mode):
                result, calls = self.run_query(
                    ["one.invalid", "two.invalid"], json_mode=json_mode,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("two.invalid", calls)
                self.assertIn("two.invalid", result.stdout)
                if not json_mode:
                    self.assertIn("Successfully queried all 2 nodes", result.stdout)

    def test_partial_success(self):
        for hosts in (["fail.invalid", "one.invalid"], ["one.invalid", "fail.invalid"]):
            with self.subTest(hosts=hosts):
                result, calls = self.run_query(hosts)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertTrue(all(host in calls for host in hosts))
                self.assertIn("Successfully queried 1 out of 2 nodes", result.stdout)

    def test_all_failed(self):
        for json_mode in (False, True):
            with self.subTest(json_mode=json_mode):
                result, calls = self.run_query(["fail.invalid"], json_mode=json_mode)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(calls, ["fail.invalid"])
                self.assertIn("Failed to connect", result.stdout)


if __name__ == "__main__":
    unittest.main(verbosity=2)
