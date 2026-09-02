from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from scripts import native_shadow_installed_mac_e2e_v1 as e2e


ROOT = Path(__file__).resolve().parents[1]
GRANT = ROOT / "native/containment/native-shadow-closed-local-replay-grant-arm64-v1.json"
FIXTURES = ROOT / "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history"


class InstalledMacCaseMatrixTests(unittest.TestCase):
    def test_private_directory_repairs_tmp_group_inheritance(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "private"
            e2e._prepare_private_directory(path)
            metadata = path.stat()
            self.assertEqual(metadata.st_uid, os.geteuid())
            self.assertEqual(metadata.st_gid, os.getegid())
            self.assertEqual(metadata.st_mode & 0o777, 0o700)

    def test_real_four_case_matrix_requires_accept_reject_reject_precheck(self) -> None:
        observed: list[dict[str, object]] = []
        replies = iter(
            [
                (200, {"outcome": "accepted", "reasonCode": "accepted"}),
                (
                    200,
                    {
                        "outcome": "deterministic_reject",
                        "reasonCode": "checker_rejected",
                    },
                ),
                (
                    200,
                    {
                        "outcome": "deterministic_reject",
                        "reasonCode": "checker_rejected",
                    },
                ),
                (400, {"outcome": "precheck_reject", "reasonCode": "empty_response"}),
            ]
        )

        def post(payload: dict[str, object]) -> tuple[int, bytes]:
            observed.append(payload)
            status, body = next(replies)
            return status, json.dumps(body).encode("utf-8")

        result = e2e.run_case_matrix(GRANT, FIXTURES, post)

        self.assertEqual(
            [row["caseId"] for row in result],
            ["accepted", "tampered", "constant", "empty"],
        )
        self.assertEqual([row["passed"] for row in result], [True] * 4)
        self.assertEqual([payload["epoch"] for payload in observed], [0, 1, 2, 3])
        self.assertEqual(
            {payload["familyVersion"] for payload in observed},
            {"TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1"},
        )
        self.assertEqual(
            {payload["templateId"] for payload in observed},
            {"800eee9c303c6a0e771e3a3db914eb15ea4ca68d10b19385d60fedd2c23e04b5"},
        )

    def test_transport_layout_exposes_only_signed_host_and_guest_prefixes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            output = root / "http-root"
            output.mkdir()
            for metadata in (
                "release-manifest.json",
                "release-signature.json",
                "guest-update-manifest",
                "guest-update-signature",
                "TRUST-ROOTS.json",
            ):
                (output / metadata).write_bytes(metadata.encode("ascii"))

            product = {}
            for role in ("host-cli", "host-node", "host-wallet-agent", "host-controller"):
                path = source / role
                path.write_bytes(("product:" + role).encode("ascii"))
                product[role] = str(path)
            guest = {}
            for role in (
                "guest-kernel",
                "guest-root-disk",
                "rootfs-content-manifest",
                "registry",
                "execution-policy",
                "toolchain-identity",
                "checker-release-manifest",
                "registry-overlay",
                "closed-local-replay-grant",
                "local-execution-authority",
                "closed-local-replay-execution-authority",
            ):
                path = source / role
                path.write_bytes(("guest:" + role).encode("ascii"))
                guest[role] = str(path)
            plan = {
                "outputDir": str(output),
                "sourceRevision": "12" * 20,
                "productArtifacts": product,
                "guestArtifacts": guest,
            }
            plan_path = root / "plan.json"
            plan_path.write_text(json.dumps(plan), encoding="utf-8")

            result = e2e.materialize_transport_layout(plan_path, output)

            self.assertEqual(result, {"hostArtifacts": 4, "guestArtifacts": 11})
            self.assertEqual(
                {path.name for path in output.iterdir()},
                {
                    "release-manifest.json",
                    "release-signature.json",
                    "guest-update-manifest",
                    "guest-update-signature",
                    "TRUST-ROOTS.json",
                    "host-cli",
                    "host-node",
                    "host-wallet-agent",
                    "host-controller",
                    "guest",
                },
            )
            self.assertEqual(
                {path.name for path in (output / "guest").iterdir()}, set(guest)
            )
            self.assertEqual((output / "host-node").read_bytes(), b"product:host-node")
            self.assertEqual(
                (output / "guest" / "guest-kernel").read_bytes(), b"guest:guest-kernel"
            )

    def test_real_cli_process_installs_from_loopback_with_both_explicit_roots(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            bundle = root / "bundle"
            bundle.mkdir()
            (bundle / "release-manifest.json").write_bytes(b"signed-product-manifest")
            roots = {
                "productKeyId": "product-kat",
                "productPublicKeyHex": "11" * 32,
                "guestKeyId": "guest-kat",
                "guestPublicKeyHex": "22" * 32,
            }
            (bundle / "TRUST-ROOTS.json").write_text(json.dumps(roots), encoding="utf-8")
            cli = root / "fake-boole-cli"
            cli.write_text(
                """#!/usr/bin/env python3
import json, pathlib, sys, urllib.request
args = sys.argv[1:]
def value(name): return args[args.index(name) + 1]
assert args[:2] == ['product', 'install-direct-boot']
assert urllib.request.urlopen(value('--base-url') + '/release-manifest.json').read() == b'signed-product-manifest'
install = pathlib.Path(value('--install-root'))
install.mkdir()
(install / 'observed.json').write_text(json.dumps({'args': args}))
print(json.dumps({'ok': True, 'version': 'v1', 'command': 'product.install-direct-boot', 'data': {'releaseSequence': 1, 'guestReleaseSequence': 1}}))
""",
                encoding="utf-8",
            )
            cli.chmod(0o700)
            install_root = root / "install"
            staging = root / "staging"

            result = e2e.install_direct_boot_bundle(
                cli, bundle, install_root, staging, roots, timeout_seconds=5
            )

            self.assertEqual(result["command"], "product.install-direct-boot")
            observed = json.loads((install_root / "observed.json").read_text(encoding="utf-8"))[
                "args"
            ]
            self.assertEqual(observed[0:2], ["product", "install-direct-boot"])
            self.assertEqual(observed[observed.index("--product-trust-root-key-id") + 1], "product-kat")
            self.assertEqual(observed[observed.index("--guest-trust-root-key-id") + 1], "guest-kat")
            base_url = observed[observed.index("--base-url") + 1]
            self.assertTrue(base_url.startswith("http://127.0.0.1:"), base_url)
            self.assertFalse(staging.exists())

    def test_installed_node_serves_matrix_then_exits_cleanly_on_term(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            install = root / "install"
            version = install / "versions" / "000000000001-test"
            version.mkdir(parents=True)
            (install / "installed-release.json").write_text(
                json.dumps(
                    {
                        "schema": "boole.curl-product-install-state.v1",
                        "releaseSequence": 1,
                        "releaseVersion": "test",
                        "manifestSha256": "11" * 32,
                        "versionDirectory": version.name,
                    }
                ),
                encoding="utf-8",
            )
            node = version / "host-node"
            node.write_text(
                """#!/usr/bin/env python3
import http.server, json, signal, threading
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        length = int(self.headers['Content-Length'])
        epoch = json.loads(self.rfile.read(length))['epoch']
        rows = {
          0: (200, 'accepted', 'accepted'),
          1: (200, 'deterministic_reject', 'checker_rejected'),
          2: (200, 'deterministic_reject', 'checker_rejected'),
          3: (400, 'precheck_reject', 'empty_response'),
        }
        status, outcome, reason = rows[epoch]
        body = json.dumps({'outcome': outcome, 'reasonCode': reason}).encode()
        self.send_response(status); self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)
server = http.server.ThreadingHTTPServer(('127.0.0.1', 8082), Handler)
def stop(*_): threading.Thread(target=server.shutdown, daemon=True).start()
signal.signal(signal.SIGTERM, stop); signal.signal(signal.SIGINT, stop)
server.serve_forever(); server.server_close()
""",
                encoding="utf-8",
            )
            node.chmod(0o700)
            runtime = root / "runtime"
            journal = root / "state" / "replay.ndjson"
            work = root / "work"
            roots = {
                "productKeyId": "product-kat",
                "productPublicKeyHex": "11" * 32,
                "guestKeyId": "guest-kat",
                "guestPublicKeyHex": "22" * 32,
            }

            result = e2e.run_installed_node_matrix(
                install,
                runtime,
                journal,
                work,
                roots,
                GRANT,
                FIXTURES,
                startup_timeout_seconds=5,
            )

            self.assertEqual([row["caseId"] for row in result], list(e2e.CASE_FILES))
            self.assertEqual(list(runtime.iterdir()), [])
            self.assertTrue((work / "node.stdout").is_file())
            self.assertTrue((work / "node.stderr").is_file())

    def test_harness_composes_metadata_install_route_and_bounded_cleanup(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            sources = root / "sources"
            sources.mkdir()
            node = sources / "host-node"
            node.write_text(
                """#!/usr/bin/env python3
import http.server, json, signal, threading
class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        size = int(self.headers['Content-Length']); epoch = json.loads(self.rfile.read(size))['epoch']
        rows = {0:(200,'accepted','accepted'),1:(200,'deterministic_reject','checker_rejected'),2:(200,'deterministic_reject','checker_rejected'),3:(400,'precheck_reject','empty_response')}
        status, outcome, reason = rows[epoch]; body = json.dumps({'outcome':outcome,'reasonCode':reason}).encode()
        self.send_response(status); self.send_header('Content-Type','application/json'); self.send_header('Content-Length',str(len(body))); self.end_headers(); self.wfile.write(body)
server=http.server.ThreadingHTTPServer(('127.0.0.1',8082),Handler)
def stop(*_): threading.Thread(target=server.shutdown,daemon=True).start()
signal.signal(signal.SIGTERM,stop); signal.signal(signal.SIGINT,stop); server.serve_forever(); server.server_close()
""",
                encoding="utf-8",
            )
            node.chmod(0o700)
            cli = sources / "host-cli"
            cli.write_text(
                """#!/usr/bin/env python3
import json, pathlib, shutil, sys, urllib.request
a=sys.argv[1:]
def v(name): return a[a.index(name)+1]
install=pathlib.Path(v('--install-root')); version=install/'versions'/'000000000001-test'; version.mkdir(parents=True)
with urllib.request.urlopen(v('--base-url')+'/host-node') as r, (version/'host-node').open('wb') as w: shutil.copyfileobj(r,w)
(version/'host-node').chmod(0o700)
(install/'installed-release.json').write_text(json.dumps({'schema':'boole.curl-product-install-state.v1','releaseSequence':1,'releaseVersion':'test','manifestSha256':'11'*32,'versionDirectory':version.name}))
print(json.dumps({'ok':True,'command':'product.install-direct-boot','data':{'releaseSequence':1,'guestReleaseSequence':1}}))
""",
                encoding="utf-8",
            )
            cli.chmod(0o700)
            product = {"host-cli": str(cli), "host-node": str(node)}
            for role in ("host-wallet-agent", "host-controller"):
                path = sources / role
                path.write_bytes(role.encode())
                product[role] = str(path)
            guest = {}
            for role in e2e.GUEST_ARTIFACT_ROLES:
                path = sources / role
                path.write_bytes(role.encode())
                guest[role] = str(path)
            work = root / "work"
            plan = root / "plan.json"
            plan.write_text(
                json.dumps(
                    {
                        "outputDir": str(work / "http-root"),
                        "sourceRevision": "12" * 20,
                        "productArtifacts": product,
                        "guestArtifacts": guest,
                    }
                ),
                encoding="utf-8",
            )
            kat = root / "fake-kat"
            kat.write_text(
                """#!/usr/bin/env python3
import json, pathlib, sys
p=json.loads(pathlib.Path(sys.argv[1]).read_text()); out=pathlib.Path(p['outputDir']); out.mkdir()
roots={'productKeyId':'product-kat','productPublicKeyHex':'11'*32,'guestKeyId':'guest-kat','guestPublicKeyHex':'22'*32}
for name in ('release-manifest.json','release-signature.json','guest-update-manifest','guest-update-signature'): (out/name).write_text(name)
(out/'TRUST-ROOTS.json').write_text(json.dumps(roots)); print(json.dumps(roots))
""",
                encoding="utf-8",
            )
            kat.chmod(0o700)
            result_path = root / "result.json"

            completed = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/native_shadow_installed_mac_e2e_v1.py"),
                    "--kat-plan",
                    str(plan),
                    "--kat-binary",
                    str(kat),
                    "--cli",
                    str(cli),
                    "--work",
                    str(work),
                    "--result",
                    str(result_path),
                    "--grant",
                    str(GRANT),
                    "--fixtures",
                    str(FIXTURES),
                    "--startup-timeout-seconds",
                    "5",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
                timeout=30,
            )

            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertEqual(
                completed.stdout.strip(),
                "installed Mac E2E: INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS",
            )
            result = json.loads(result_path.read_text(encoding="utf-8"))
            self.assertEqual(result["status"], "INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS")
            self.assertEqual([row["caseId"] for row in result["cases"]], list(e2e.CASE_FILES))
            self.assertFalse((work / "http-root").exists())
            self.assertFalse((work / "install-root").exists())
            self.assertTrue((work / "node-logs" / "node.stderr").is_file())


if __name__ == "__main__":
    unittest.main()
