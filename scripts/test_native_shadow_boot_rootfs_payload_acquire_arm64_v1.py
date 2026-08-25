#!/usr/bin/env python3
"""RED-first contracts for two-stage ARM64 package payload acquisition."""

from __future__ import annotations

import hashlib
import pathlib
import ssl
import tempfile
import unittest
from unittest import mock

from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payloads


ROOT = pathlib.Path(__file__).resolve().parents[1]
PLAN = ROOT / "native/containment/native-shadow-boot-rootfs-payload-acquisition-plan-arm64-v1.json"
ACQUIRER = ROOT / "scripts/native_shadow_boot_rootfs_payload_acquire_arm64_v1.py"

def _spec(identifier: str, raw: bytes) -> dict[str, object]:
    return {
        "artifactId": identifier,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "sizeBytes": len(raw),
        "url": f"https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/pool/{identifier}.deb",
    }


class NativeShadowBootRootfsPayloadAcquireArm64Tests(unittest.TestCase):
    def test_tracked_plan_pins_exact_authorities_and_51_then_134_contract(self) -> None:
        plan, raw = payloads._load_execution_plan(PLAN)
        self.assertEqual(hashlib.sha256(raw).hexdigest(), payloads.EXPECTED_PLAN_SHA256)
        self.assertEqual(
            plan["authorityInputs"]["payloadAcquirer"],
            {
                "sha256": payloads.payload_acquirer_authority_sha256(
                    ACQUIRER.read_bytes()
                ),
                "sizeBytes": len(ACQUIRER.read_bytes()),
            },
        )
        loaded = payloads._load_pinned_authorities(plan, ROOT)
        metadata, baseline, delta = payloads._validate_authority_and_specs(
            plan,
            loaded,
            pathlib.Path("/opt/homebrew/bin/gpgv").resolve(),
            pathlib.Path("/opt/homebrew/bin/zstd").resolve(),
        )
        self.assertEqual(metadata["sizeBytes"], 1_376_632)
        self.assertEqual(len(baseline), 56)
        self.assertEqual(len(delta), 135)
        self.assertEqual(plan["expected"]["baselineFetches"], 51)
        self.assertEqual(plan["expected"]["deltaFetches"], 134)
        self.assertEqual(
            plan["excludedRustArtifactIds"],
            ["cargo-rustdist", "rust-std-rustdist", "rustc-rustdist"],
        )
        self.assertFalse(plan["activationAllowed"])
        self.assertTrue(all(value is False for value in plan["boundaries"].values()))

    def test_delta_is_never_requested_until_every_baseline_blob_is_verified(self) -> None:
        baseline_raw = b"baseline-package"
        delta_raw = b"delta-package"
        baseline = [_spec("baseline", baseline_raw)]
        delta = [_spec("delta", delta_raw)]
        requested: list[str] = []

        def stream(spec: dict[str, object]):
            requested.append(str(spec["artifactId"]))
            if spec["artifactId"] == "baseline":
                return iter([baseline_raw + b"-corrupt"])
            return iter([delta_raw])

        with tempfile.TemporaryDirectory() as raw_directory:
            with self.assertRaises(payloads.PayloadAcquisitionError):
                payloads.acquire_two_stage_payloads(
                    pathlib.Path(raw_directory) / "cas",
                    baseline,
                    delta,
                    stream_factory=stream,
                )

        self.assertEqual(requested, ["baseline"])

    def test_happy_path_is_baseline_then_delta_and_exact_hits_use_no_network(self) -> None:
        baseline_raw = b"baseline-package"
        delta_raw = b"delta-package"
        baseline = [_spec("baseline", baseline_raw)]
        delta = [_spec("delta", delta_raw)]
        payload_by_id = {"baseline": baseline_raw, "delta": delta_raw}
        requested: list[str] = []

        def stream(spec: dict[str, object]):
            identifier = str(spec["artifactId"])
            requested.append(identifier)
            return iter([payload_by_id[identifier]])

        with tempfile.TemporaryDirectory() as raw_directory:
            cas = pathlib.Path(raw_directory) / "cas"
            first = payloads.acquire_two_stage_payloads(
                cas, baseline, delta, stream_factory=stream
            )
            self.assertEqual(requested, ["baseline", "delta"])
            self.assertEqual(first["baselineFetched"], 1)
            self.assertEqual(first["deltaFetched"], 1)

            requested.clear()
            second = payloads.acquire_two_stage_payloads(
                cas, baseline, delta, stream_factory=stream
            )
            self.assertEqual(requested, [])
            self.assertEqual(second["baselineReused"], 1)
            self.assertEqual(second["deltaReused"], 1)

    def test_cross_stage_digest_alias_is_rejected_before_any_request(self) -> None:
        raw = b"same-package-bytes"
        baseline = [_spec("baseline", raw)]
        delta = [_spec("different-id", raw)]
        requested: list[str] = []

        with tempfile.TemporaryDirectory() as raw_directory:
            with self.assertRaisesRegex(payloads.PayloadAcquisitionError, "overlap"):
                payloads.acquire_two_stage_payloads(
                    pathlib.Path(raw_directory) / "cas",
                    baseline,
                    delta,
                    stream_factory=lambda spec: requested.append(
                        str(spec["artifactId"])
                    )
                    or iter([raw]),
                )
        self.assertEqual(requested, [])

    def test_non_snapshot_url_is_rejected_before_any_request(self) -> None:
        raw = b"payload"
        bad = _spec("bad", raw)
        bad["url"] = "https://example.com/ubuntu/20240425T160000Z/pool/bad.deb"
        requested: list[str] = []

        with tempfile.TemporaryDirectory() as raw_directory:
            with self.assertRaisesRegex(payloads.PayloadAcquisitionError, "URL"):
                payloads.acquire_two_stage_payloads(
                    pathlib.Path(raw_directory) / "cas",
                    [bad],
                    [],
                    stream_factory=lambda spec: requested.append("bad") or iter([raw]),
                )
        self.assertEqual(requested, [])

    def test_insecure_existing_blob_is_not_silently_reused_or_replaced(self) -> None:
        raw = b"payload"
        spec = _spec("baseline", raw)
        requested: list[str] = []
        with tempfile.TemporaryDirectory() as raw_directory:
            cas = pathlib.Path(raw_directory) / "cas"
            destination = cas / "sha256" / str(spec["sha256"])
            destination.parent.mkdir(parents=True)
            destination.write_bytes(raw)
            destination.chmod(0o644)
            with self.assertRaisesRegex(payloads.PayloadAcquisitionError, "mode"):
                payloads.acquire_two_stage_payloads(
                    cas,
                    [spec],
                    [],
                    stream_factory=lambda item: requested.append("baseline") or iter([raw]),
                )
        self.assertEqual(requested, [])

    def test_bad_stream_never_publishes_a_digest_named_blob(self) -> None:
        raw = b"payload"
        spec = _spec("baseline", raw)
        with tempfile.TemporaryDirectory() as raw_directory:
            cas = pathlib.Path(raw_directory) / "cas"
            with self.assertRaises(payloads.PayloadAcquisitionError):
                payloads.acquire_two_stage_payloads(
                    cas,
                    [spec],
                    [],
                    stream_factory=lambda item: iter([raw, b"extra"]),
                )
            self.assertFalse((cas / "sha256" / str(spec["sha256"])).exists())
            self.assertEqual(
                [path.name for path in (cas / "sha256").iterdir()],
                [],
            )

    def test_https_stream_is_exact_get_tls12_no_proxy_redirect_retry_or_range(self) -> None:
        raw = b"network-payload"
        spec = _spec("network", raw)
        calls: list[tuple[object, ...]] = []

        class Response:
            status = 200

            @staticmethod
            def getheader(name: str):
                return {
                    "Content-Length": str(len(raw)),
                    "Content-Encoding": None,
                }.get(name)

            def read(self, amount: int) -> bytes:
                if not raw_parts:
                    return b""
                return raw_parts.pop(0)

        class Connection:
            def __init__(self, host: str, port: int, *, timeout: int, context: ssl.SSLContext):
                calls.append(("connect", host, port, timeout, context))

            def putrequest(self, method: str, path: str, **kwargs: object) -> None:
                calls.append(("request", method, path, kwargs))

            def putheader(self, name: str, value: str) -> None:
                calls.append(("header", name, value))

            def endheaders(self) -> None:
                calls.append(("endheaders",))

            def getresponse(self) -> Response:
                return Response()

            def close(self) -> None:
                calls.append(("close",))

        raw_parts = [raw[:5], raw[5:]]
        context = ssl.create_default_context()
        with mock.patch.dict(
            "os.environ",
            {"HTTPS_PROXY": "http://attacker.invalid:8080"},
            clear=False,
        ):
            chunks = list(
                payloads.snapshot_https_stream(
                    spec,
                    connection_factory=Connection,
                    context_factory=lambda: context,
                )
            )
        self.assertEqual(b"".join(chunks), raw)
        self.assertEqual(context.minimum_version, ssl.TLSVersion.TLSv1_2)
        self.assertEqual(context.verify_mode, ssl.CERT_REQUIRED)
        self.assertTrue(context.check_hostname)
        self.assertEqual(len([call for call in calls if call[0] == "connect"]), 1)
        self.assertIn(("header", "Accept-Encoding", "identity"), calls)
        self.assertFalse(any(call[:2] == ("header", "Range") for call in calls))
        self.assertIn(("close",), calls)

    def test_https_stream_rejects_redirect_or_encoded_response_without_retry(self) -> None:
        raw = b"network-payload"
        spec = _spec("network", raw)

        for status, encoding in ((302, None), (200, "gzip")):
            connections = 0

            class Response:
                def __init__(self) -> None:
                    self.status = status

                @staticmethod
                def getheader(name: str):
                    return {
                        "Content-Length": str(len(raw)),
                        "Content-Encoding": encoding,
                    }.get(name)

                @staticmethod
                def read(amount: int) -> bytes:
                    return raw

            class Connection:
                def __init__(self, *args: object, **kwargs: object):
                    nonlocal connections
                    connections += 1

                def putrequest(self, *args: object, **kwargs: object) -> None:
                    pass

                def putheader(self, *args: object, **kwargs: object) -> None:
                    pass

                def endheaders(self) -> None:
                    pass

                def getresponse(self) -> Response:
                    return Response()

                def close(self) -> None:
                    pass

            with self.subTest(status=status, encoding=encoding):
                with self.assertRaises(payloads.PayloadAcquisitionError):
                    list(
                        payloads.snapshot_https_stream(
                            spec,
                            connection_factory=Connection,
                        )
                    )
                self.assertEqual(connections, 1)

    def test_second_acquirer_cannot_enter_the_same_cas(self) -> None:
        raw = b"locked-payload"
        spec = _spec("baseline", raw)
        nested_was_rejected = False

        with tempfile.TemporaryDirectory() as raw_directory:
            cas = pathlib.Path(raw_directory) / "cas"

            def stream(item: dict[str, object]):
                nonlocal nested_was_rejected
                with self.assertRaisesRegex(payloads.PayloadAcquisitionError, "busy"):
                    payloads.acquire_two_stage_payloads(
                        cas,
                        [spec],
                        [],
                        stream_factory=lambda ignored: iter([raw]),
                    )
                nested_was_rejected = True
                return iter([raw])

            payloads.acquire_two_stage_payloads(
                cas,
                [spec],
                [],
                stream_factory=stream,
            )
            self.assertTrue(nested_was_rejected)
            self.assertEqual((cas / ".arm64-payload-acquisition.lock").stat().st_mode & 0o777, 0o600)

    def test_package_requests_wait_for_metadata_replay_byte_equality(self) -> None:
        metadata_raw = b"signed-packages-index"
        baseline_raw = b"baseline-package"
        delta_raw = b"delta-package"
        metadata = _spec("metadata", metadata_raw)
        baseline = [_spec("baseline", baseline_raw)]
        delta = [_spec("delta", delta_raw)]
        raw_by_id = {
            "metadata": metadata_raw,
            "baseline": baseline_raw,
            "delta": delta_raw,
        }
        requested: list[str] = []

        def stream(spec: dict[str, object]):
            identifier = str(spec["artifactId"])
            requested.append(identifier)
            return iter([raw_by_id[identifier]])

        with tempfile.TemporaryDirectory() as raw_directory:
            cas = pathlib.Path(raw_directory) / "cas"
            with self.assertRaisesRegex(payloads.PayloadAcquisitionError, "replay"):
                payloads.acquire_after_signed_replay(
                    cas,
                    metadata,
                    baseline,
                    delta,
                    replay_candidate=lambda: False,
                    stream_factory=stream,
                )
        self.assertEqual(requested, ["metadata"])

    def test_signed_replay_green_orders_metadata_baseline_delta(self) -> None:
        rows = {
            "metadata": b"signed-packages-index",
            "baseline": b"baseline-package",
            "delta": b"delta-package",
        }
        specs = {key: _spec(key, value) for key, value in rows.items()}
        events: list[str] = []

        def stream(spec: dict[str, object]):
            identifier = str(spec["artifactId"])
            events.append(f"fetch:{identifier}")
            return iter([rows[identifier]])

        def replay() -> bool:
            events.append("replay")
            return True

        with tempfile.TemporaryDirectory() as raw_directory:
            payloads.acquire_after_signed_replay(
                pathlib.Path(raw_directory) / "cas",
                specs["metadata"],
                [specs["baseline"]],
                [specs["delta"]],
                replay_candidate=replay,
                stream_factory=stream,
            )
        self.assertEqual(
            events,
            ["fetch:metadata", "replay", "fetch:baseline", "fetch:delta"],
        )


if __name__ == "__main__":
    unittest.main()
