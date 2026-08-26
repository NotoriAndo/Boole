#!/usr/bin/env python3
"""Acceptance grounds for the frozen ARM64 Rust distribution acquisition.

These rules are written before the archives are fetched. They fix what may be
requested, what a response must look like, what may be published into the
content-addressed store, and what the result document is allowed to say. A
download that satisfies every rule still proves only that the exact frozen
bytes are held locally: no toolchain is installed, no launcher is built and no
boundary flips.
"""

from __future__ import annotations

import copy
import hashlib
import json
import os
import pathlib
import unittest

from scripts import native_shadow_boot_rustdist_acquire_arm64_v1 as rustdist

ROOT = pathlib.Path(__file__).resolve().parents[1]


def _load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class _Response:
    """A scripted HTTPS response with no redirect, retry or Range support."""

    def __init__(self, status: int, headers: dict[str, str], body: bytes) -> None:
        self.status = status
        self._headers = headers
        self._body = body
        self._offset = 0

    def getheader(self, name: str, default=None):
        return self._headers.get(name, default)

    def read(self, amount: int) -> bytes:
        chunk = self._body[self._offset : self._offset + amount]
        self._offset += len(chunk)
        return chunk


class _Connection:
    """Records the exact request the acquirer issues."""

    def __init__(self, response: _Response, journal: list) -> None:
        self._response = response
        self._journal = journal
        self.headers: dict[str, str] = {}
        self.request: tuple = ()

    def putrequest(self, method, path, skip_host=False, skip_accept_encoding=False):
        self.request = (method, path, skip_host, skip_accept_encoding)

    def putheader(self, name, value):
        self.headers[name] = value

    def endheaders(self):
        self._journal.append({"request": self.request, "headers": dict(self.headers)})

    def getresponse(self):
        return self._response

    def close(self):
        return None


def _factories(response: _Response, journal: list):
    def connection_factory(host, port, timeout=None, context=None):
        journal.append({"host": host, "port": port, "timeout": timeout})
        return _Connection(response, journal)

    class _Context:
        minimum_version = None
        verify_mode = None
        check_hostname = None

    def context_factory():
        context = _Context()
        journal.append({"context": context})
        return context

    return connection_factory, context_factory


class PlanAcceptanceTests(unittest.TestCase):
    """Every frozen field of the pre-registered plan is machine-checked."""

    def setUp(self) -> None:
        self.plan = _load(rustdist.PLAN_PATH)

    def mutate(self) -> dict:
        return copy.deepcopy(self.plan)

    def assertRefused(self, plan: dict, needle: str) -> None:
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            rustdist.validate_plan(plan)
        self.assertIn(needle, str(caught.exception))

    def test_the_frozen_plan_is_accepted_as_written(self) -> None:
        rustdist.validate_plan(self.plan)
        self.assertEqual(len(self.plan["artifacts"]), 3)
        self.assertEqual(self.plan["expected"]["totalBytes"], 112995148)

    def test_plan_digest_is_pinned_inside_the_tool(self) -> None:
        raw = rustdist.PLAN_PATH.read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), rustdist.PLAN_SHA256)
        loaded = rustdist.load_plan()
        self.assertEqual(loaded, self.plan)

    def test_tool_authority_digest_normalizes_its_own_plan_pin(self) -> None:
        raw = rustdist.TOOL_PATH.read_bytes()
        pinned = self.plan["authorityInputs"]["rustdistAcquirer"]
        self.assertEqual(pinned["sizeBytes"], len(raw))
        self.assertEqual(pinned["sha256"], rustdist.rustdist_acquirer_authority_sha256(raw))
        self.assertNotEqual(pinned["sha256"], hashlib.sha256(raw).hexdigest())

    def test_missing_plan_key_is_refused(self) -> None:
        plan = self.mutate()
        del plan["networkPolicy"]
        self.assertRefused(plan, "plan keys differ")

    def test_extra_plan_key_is_refused(self) -> None:
        plan = self.mutate()
        plan["bootProven"] = True
        self.assertRefused(plan, "plan keys differ")

    def test_activation_or_boot_claim_is_refused(self) -> None:
        for key in ("activationAllowed", "bootableClaim"):
            plan = self.mutate()
            plan[key] = True
            self.assertRefused(plan, "must not claim activation or boot")

    def test_flipped_boundary_is_refused(self) -> None:
        for name in sorted(rustdist.BOUNDARY_KEYS):
            plan = self.mutate()
            plan["boundaries"][name] = True
            self.assertRefused(plan, f"boundary {name} must stay false")

    def test_dropped_boundary_is_refused(self) -> None:
        plan = self.mutate()
        del plan["boundaries"]["toolchainInstalled"]
        self.assertRefused(plan, "boundary keys differ")

    def test_relaxed_network_policy_is_refused(self) -> None:
        for key in (
            "allowEnvironmentProxy",
            "allowRangeRequests",
            "allowRedirects",
            "allowRetries",
        ):
            plan = self.mutate()
            plan["networkPolicy"][key] = True
            self.assertRefused(plan, "network policy differs")

    def test_widened_host_allowlist_is_refused(self) -> None:
        plan = self.mutate()
        plan["networkPolicy"]["allowedHosts"] = [
            "ci-artifacts.rust-lang.org",
            "mirror.example.invalid",
        ]
        self.assertRefused(plan, "network policy differs")

    def test_parallel_download_is_refused(self) -> None:
        plan = self.mutate()
        plan["networkPolicy"]["concurrency"] = 4
        self.assertRefused(plan, "network policy differs")

    def test_dropped_tls_requirement_is_refused(self) -> None:
        for key in (
            "requireCertificateValidation",
            "requireContentLengthMatch",
            "requireHostnameValidation",
            "httpsOnly",
        ):
            plan = self.mutate()
            plan["networkPolicy"][key] = False
            self.assertRefused(plan, "network policy differs")

    def test_plaintext_url_is_refused(self) -> None:
        plan = self.mutate()
        row = plan["artifacts"][0]
        row["url"] = row["url"].replace("https://", "http://", 1)
        self.assertRefused(plan, "frozen host policy")

    def test_foreign_host_is_refused(self) -> None:
        plan = self.mutate()
        row = plan["artifacts"][0]
        row["url"] = row["url"].replace("ci-artifacts.rust-lang.org", "mirror.example.invalid", 1)
        self.assertRefused(plan, "frozen host policy")

    def test_url_credentials_are_refused(self) -> None:
        plan = self.mutate()
        row = plan["artifacts"][0]
        row["url"] = row["url"].replace(
            "https://ci-artifacts.rust-lang.org",
            "https://user:secret@ci-artifacts.rust-lang.org",
            1,
        )
        self.assertRefused(plan, "frozen host policy")

    def test_url_query_or_fragment_is_refused(self) -> None:
        for suffix in ("?mirror=1", "#fragment"):
            plan = self.mutate()
            plan["artifacts"][0]["url"] += suffix
            self.assertRefused(plan, "frozen host policy")

    def test_url_outside_the_commit_prefix_is_refused(self) -> None:
        plan = self.mutate()
        row = plan["artifacts"][0]
        row["url"] = "https://ci-artifacts.rust-lang.org/rustc-builds/master/cargo.tar.xz"
        self.assertRefused(plan, "frozen host policy")

    def test_commit_path_prefix_must_be_commit_derived(self) -> None:
        plan = self.mutate()
        plan["toolchain"]["commitPathPrefix"] = "/rustc-builds/nightly/"
        self.assertRefused(plan, "commit path prefix is not commit-derived")

    def test_non_aarch64_target_is_refused(self) -> None:
        plan = self.mutate()
        plan["toolchain"]["rustTarget"] = "x86_64-unknown-linux-gnu"
        self.assertRefused(plan, "target is not aarch64")

    def test_date_nightly_commit_is_refused(self) -> None:
        plan = self.mutate()
        plan["toolchain"]["rustcCommitHash"] = "nightly-2026-07-22"
        self.assertRefused(plan, "is not a commit hash")

    def test_unsorted_artifacts_are_refused(self) -> None:
        plan = self.mutate()
        plan["artifacts"].reverse()
        self.assertRefused(plan, "not sorted by artifactId")

    def test_duplicated_artifact_is_refused(self) -> None:
        plan = self.mutate()
        plan["artifacts"].append(copy.deepcopy(plan["artifacts"][0]))
        plan["artifacts"].sort(key=lambda row: row["artifactId"])
        self.assertRefused(plan, "identity is duplicated")

    def test_tampered_digest_is_refused(self) -> None:
        plan = self.mutate()
        plan["artifacts"][0]["sha256"] = "0" * 64
        self.assertRefused(plan, "frozen acquisition plan")

    def test_tampered_size_is_refused(self) -> None:
        plan = self.mutate()
        plan["artifacts"][0]["sizeBytes"] += 1
        self.assertRefused(plan, "frozen acquisition plan")

    def test_artifact_absent_from_the_frozen_plan_is_refused(self) -> None:
        plan = self.mutate()
        row = copy.deepcopy(plan["artifacts"][0])
        row["artifactId"] = "aaa-invented-rustdist"
        row["sha256"] = "1" * 64
        row["url"] = row["url"].replace("cargo-nightly", "invented-nightly", 1)
        plan["artifacts"].append(row)
        plan["artifacts"].sort(key=lambda item: item["artifactId"])
        plan["expected"]["artifactCount"] = len(plan["artifacts"])
        plan["expected"]["fetchArtifactIds"] = sorted(
            item["artifactId"] for item in plan["artifacts"]
        )
        plan["expected"]["totalBytes"] = sum(item["sizeBytes"] for item in plan["artifacts"])
        plan["expected"]["fetchBytes"] = plan["expected"]["totalBytes"]
        self.assertRefused(plan, "absent from the frozen acquisition plan")

    def test_dropped_artifact_is_refused_against_the_sealed_lock(self) -> None:
        plan = self.mutate()
        dropped = plan["artifacts"].pop()
        plan["expected"]["artifactCount"] = len(plan["artifacts"])
        plan["expected"]["fetchArtifactIds"] = [
            item for item in plan["expected"]["fetchArtifactIds"] if item != dropped["artifactId"]
        ]
        plan["expected"]["presentArtifactIds"] = [
            item
            for item in plan["expected"]["presentArtifactIds"]
            if item != dropped["artifactId"]
        ]
        plan["expected"]["totalBytes"] -= dropped["sizeBytes"]
        plan["expected"]["fetchBytes"] = sum(
            item["sizeBytes"]
            for item in plan["artifacts"]
            if item["artifactId"] in plan["expected"]["fetchArtifactIds"]
        )
        self.assertRefused(plan, "differs from the sealed lock")

    def test_broken_authority_pin_is_refused(self) -> None:
        for name in ("bootSourceLock", "runtimeAcquisitionPlan", "rustdistAcquirer"):
            plan = self.mutate()
            plan["authorityInputs"][name]["sha256"] = "2" * 64
            self.assertRefused(plan, "differs from the pin")

    def test_expected_counts_must_match_the_artifact_set(self) -> None:
        plan = self.mutate()
        plan["expected"]["artifactCount"] = 99
        self.assertRefused(plan, "artifactCount differs")

    def test_expected_total_bytes_must_match_the_artifact_set(self) -> None:
        plan = self.mutate()
        plan["expected"]["totalBytes"] += 1
        self.assertRefused(plan, "totalBytes differs")

    def test_expected_partition_must_cover_every_artifact(self) -> None:
        plan = self.mutate()
        plan["expected"]["fetchArtifactIds"] = plan["expected"]["fetchArtifactIds"][:1]
        plan["expected"]["fetchBytes"] = sum(
            row["sizeBytes"]
            for row in plan["artifacts"]
            if row["artifactId"] in plan["expected"]["fetchArtifactIds"]
        )
        self.assertRefused(plan, "partition is not the artifact set")

    def test_expected_fetch_bytes_must_match_the_partition(self) -> None:
        plan = self.mutate()
        plan["expected"]["fetchBytes"] += 1
        self.assertRefused(plan, "fetchBytes differs")

    def test_cas_root_is_pinned(self) -> None:
        plan = self.mutate()
        plan["cas"]["relativeRoot"] = "/tmp/anywhere"
        self.assertRefused(plan, "cas root differs")


class TransportAcceptanceTests(unittest.TestCase):
    """The response itself must match the frozen identity before it is stored."""

    def setUp(self) -> None:
        self.body = b"boole-rustdist-transport-fixture"
        self.spec = {
            "artifactId": "cargo-rustdist",
            "sha256": hashlib.sha256(self.body).hexdigest(),
            "sizeBytes": len(self.body),
            "url": (
                "https://ci-artifacts.rust-lang.org/rustc-builds/"
                "e7795af6d2449fb05a6393c3320ced873a999eb3/"
                "cargo-nightly-aarch64-unknown-linux-gnu.tar.xz"
            ),
        }

    def drain(self, status: int, headers: dict[str, str], body: bytes) -> tuple[bytes, list]:
        journal: list = []
        connection_factory, context_factory = _factories(_Response(status, headers, body), journal)
        chunks = rustdist.rustdist_https_stream(
            self.spec,
            connection_factory=connection_factory,
            context_factory=context_factory,
        )
        return b"".join(chunks), journal

    def test_exact_response_is_streamed_without_proxy_or_range(self) -> None:
        headers = {"Content-Length": str(len(self.body))}
        received, journal = self.drain(200, headers, self.body)
        self.assertEqual(received, self.body)
        connect = journal[1]
        self.assertEqual(connect["host"], "ci-artifacts.rust-lang.org")
        self.assertEqual(connect["port"], 443)
        request = journal[2]
        self.assertEqual(request["request"][0], "GET")
        self.assertNotIn("Range", request["headers"])
        self.assertNotIn("Proxy-Authorization", request["headers"])
        self.assertEqual(request["headers"]["Accept-Encoding"], "identity")
        self.assertEqual(request["headers"]["Host"], "ci-artifacts.rust-lang.org")

    def test_tls_floor_and_validation_are_requested(self) -> None:
        import ssl

        headers = {"Content-Length": str(len(self.body))}
        _, journal = self.drain(200, headers, self.body)
        context = journal[0]["context"]
        self.assertEqual(context.minimum_version, ssl.TLSVersion.TLSv1_2)
        self.assertEqual(context.verify_mode, ssl.CERT_REQUIRED)
        self.assertIs(context.check_hostname, True)

    def test_redirect_status_is_refused(self) -> None:
        headers = {"Content-Length": str(len(self.body)), "Location": "https://elsewhere.invalid/x"}
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            self.drain(302, headers, self.body)
        self.assertIn("status is not 200", str(caught.exception))

    def test_compressed_response_is_refused(self) -> None:
        headers = {"Content-Length": str(len(self.body)), "Content-Encoding": "gzip"}
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            self.drain(200, headers, self.body)
        self.assertIn("encoding is forbidden", str(caught.exception))

    def test_content_length_mismatch_is_refused(self) -> None:
        headers = {"Content-Length": str(len(self.body) + 1)}
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            self.drain(200, headers, self.body)
        self.assertIn("Content-Length differs", str(caught.exception))

    def test_missing_content_length_is_refused(self) -> None:
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            self.drain(200, {}, self.body)
        self.assertIn("Content-Length differs", str(caught.exception))

    def test_overlong_body_is_refused(self) -> None:
        headers = {"Content-Length": str(len(self.body))}
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            self.drain(200, headers, self.body + b"tail")
        self.assertIn("exceeds frozen size", str(caught.exception))

    def test_short_body_is_refused(self) -> None:
        headers = {"Content-Length": str(len(self.body))}
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            self.drain(200, headers, self.body[:-1])
        self.assertIn("shorter than frozen size", str(caught.exception))

    def test_foreign_host_is_never_dialled(self) -> None:
        spec = dict(self.spec)
        spec["url"] = spec["url"].replace("ci-artifacts.rust-lang.org", "evil.invalid", 1)
        journal: list = []
        connection_factory, context_factory = _factories(_Response(200, {}, b""), journal)
        with self.assertRaises(rustdist.RustDistAcquisitionError) as caught:
            list(
                rustdist.rustdist_https_stream(
                    spec,
                    connection_factory=connection_factory,
                    context_factory=context_factory,
                )
            )
        self.assertIn("host is not allowlisted", str(caught.exception))
        self.assertEqual(journal, [])


class StorageAcceptanceTests(unittest.TestCase):
    """Publication into the CAS is atomic, verified and never re-fetched."""

    def setUp(self) -> None:
        import tempfile

        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.cas = pathlib.Path(self.temporary.name) / "cas"
        self.body = b"boole-rustdist-storage-fixture"
        self.plan = {
            "artifacts": [
                {
                    "artifactId": "cargo-rustdist",
                    "sha256": hashlib.sha256(self.body).hexdigest(),
                    "sizeBytes": len(self.body),
                    "url": "https://ci-artifacts.rust-lang.org/rustc-builds/x/cargo.tar.xz",
                }
            ],
            "boundaries": {name: False for name in sorted(rustdist.BOUNDARY_KEYS)},
        }

    def stored(self, digest: str) -> pathlib.Path:
        return self.cas / "sha256" / digest

    def test_fetch_publishes_exact_bytes_with_private_mode(self) -> None:
        acquired = rustdist.acquire(
            self.plan,
            stream_factory=lambda spec: iter([self.body]),
            cas_root=self.cas,
        )
        digest = self.plan["artifacts"][0]["sha256"]
        path = self.stored(digest)
        self.assertEqual(path.read_bytes(), self.body)
        self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
        self.assertEqual(acquired["artifacts"][0]["disposition"], "fetched")
        self.assertEqual(acquired["fetchedBytes"], len(self.body))

    def test_a_present_artifact_is_never_re_fetched(self) -> None:
        rustdist.acquire(
            self.plan,
            stream_factory=lambda spec: iter([self.body]),
            cas_root=self.cas,
        )

        def forbidden(spec):
            raise AssertionError("a CAS hit must not issue any network request")

        acquired = rustdist.acquire(self.plan, stream_factory=forbidden, cas_root=self.cas)
        self.assertEqual(acquired["artifacts"][0]["disposition"], "cas-hit")
        self.assertEqual(acquired["fetchedBytes"], 0)

    def test_wrong_bytes_are_never_published(self) -> None:
        digest = self.plan["artifacts"][0]["sha256"]
        with self.assertRaises(Exception):
            rustdist.acquire(
                self.plan,
                stream_factory=lambda spec: iter([b"x" * len(self.body)]),
                cas_root=self.cas,
            )
        self.assertFalse(self.stored(digest).exists())
        leftovers = [
            name for name in os.listdir(self.cas / "sha256") if not name.startswith(digest)
        ]
        self.assertEqual(leftovers, [])

    def test_result_reports_the_acquisition_without_widening_it(self) -> None:
        acquired = rustdist.acquire(
            self.plan,
            stream_factory=lambda spec: iter([self.body]),
            cas_root=self.cas,
        )
        result = rustdist.build_result(self.plan, acquired)
        self.assertEqual(result["status"], rustdist.RESULT_STATUS)
        self.assertEqual(result["verifiedCount"], 1)
        self.assertEqual(result["fetchedCount"], 1)
        self.assertEqual(result["casHitCount"], 0)
        self.assertIs(result["activationAllowed"], False)
        self.assertIs(result["bootableClaim"], False)
        for name, flag in sorted(result["boundaries"].items()):
            self.assertIs(flag, False, name)
        self.assertNotIn("toolchain", result["status"].lower().split("-not-")[0])


class SealedDocumentTests(unittest.TestCase):
    """The registered documents keep the deferral honest."""

    def test_plan_is_registered_in_the_self_test_and_docs_smoke_gates(self) -> None:
        self_test = (ROOT / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        needle = "scripts/test_native_shadow_boot_rustdist_acquire_arm64_v1.py"
        self.assertTrue(needle in self_test, f"{needle} is not run by scripts/self-test.sh")
        smoke = (ROOT / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        plan = "native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json"
        self.assertTrue(plan in smoke, f"{plan} is not pinned by scripts/docs-smoke.sh")

    def test_plan_carries_the_frozen_rust_identity_only(self) -> None:
        plan = _load(rustdist.PLAN_PATH)
        source = _load(
            ROOT
            / "native"
            / "containment"
            / "native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json"
        )
        frozen = {row["artifactId"]: row for row in source["rustArtifacts"]}
        self.assertEqual(
            sorted(row["artifactId"] for row in plan["artifacts"]),
            sorted(frozen),
        )
        for row in plan["artifacts"]:
            origin = frozen[row["artifactId"]]
            self.assertEqual(row["url"], origin["url"])
            self.assertEqual(row["sha256"], origin["sha256"])
            self.assertEqual(row["sizeBytes"], origin["sizeBytes"])

    def test_pre_registration_records_the_cas_state_before_any_download(self) -> None:
        plan = _load(rustdist.PLAN_PATH)
        self.assertEqual(plan["expected"]["presentArtifactIds"], [])
        self.assertEqual(
            plan["expected"]["fetchArtifactIds"],
            sorted(row["artifactId"] for row in plan["artifacts"]),
        )
        self.assertEqual(plan["expected"]["fetchBytes"], plan["expected"]["totalBytes"])
        self.assertLess(plan["expected"]["fetchBytes"], 2 * 1024**3)

    def test_sealed_result_answers_exactly_the_pre_registered_request(self) -> None:
        plan = _load(rustdist.PLAN_PATH)
        raw = rustdist.PLAN_PATH.read_bytes()
        result = _load(rustdist.RESULT_PATH)
        self.assertEqual(result["planSha256"], rustdist.sha256_bytes(raw))
        self.assertEqual(result["schema"], rustdist.RESULT_SCHEMA)
        self.assertEqual(result["release"], rustdist.RELEASE)
        self.assertEqual(result["status"], rustdist.RESULT_STATUS)
        # The result may not answer a request that was never registered, and it
        # may not quietly drop one that was.
        self.assertEqual(
            [row["artifactId"] for row in result["artifacts"]],
            [row["artifactId"] for row in plan["artifacts"]],
        )
        frozen = {row["artifactId"]: row for row in plan["artifacts"]}
        for row in result["artifacts"]:
            origin = frozen[row["artifactId"]]
            self.assertEqual(row["sha256"], origin["sha256"])
            self.assertEqual(row["sizeBytes"], origin["sizeBytes"])
            self.assertIn(row["disposition"], {"fetched", "present"})
        self.assertEqual(result["verifiedCount"], len(plan["artifacts"]))
        self.assertEqual(
            result["fetchedCount"] + result["casHitCount"], result["verifiedCount"]
        )
        self.assertEqual(result["totalBytes"], plan["expected"]["totalBytes"])
        self.assertEqual(result["fetchedBytes"], plan["expected"]["fetchBytes"])

    def test_sealed_result_claims_no_toolchain_launcher_or_boot_authority(self) -> None:
        result = _load(rustdist.RESULT_PATH)
        self.assertEqual(sorted(result["boundaries"]), sorted(rustdist.BOUNDARY_KEYS))
        for name, value in result["boundaries"].items():
            self.assertIs(value, False, f"boundary {name} must stay false")
        self.assertIs(result["bootableClaim"], False)
        self.assertIs(result["activationAllowed"], False)
        # Verified bytes in a store are not an installed toolchain: nothing here
        # has executed, so runtime compatibility cannot have been observed.
        self.assertIs(result["boundaries"]["toolchainInstalled"], False)
        self.assertIs(result["boundaries"]["runtimeCompatibilityVerified"], False)


if __name__ == "__main__":
    unittest.main()
