"""RED for the writer set acquisition: two packages, by digest, beside the 191.

The approval permits fetching the selected e2fsprogs into permanent storage
only after the official repository metadata, signature, package size and
SHA-256 are pinned.  They were, in a record sealed before anything was fetched,
so what is left to prove here is that this acquirer reads that record rather
than deciding anything of its own, and that what it brings in is strictly
additional: neither digest and neither name is one the guest closure already
holds, and the lock those 191 live in still measures as it did.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import ssl
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk  # noqa: E402
from scripts import (  # noqa: E402
    native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload,
)
from scripts import (  # noqa: E402
    native_shadow_boot_writer_set_acquire_arm64_v1 as mod,
)

REPO = pathlib.Path(__file__).resolve().parent.parent
SELECTION_RECORD = (
    REPO / "native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json"
)
BOOT_SOURCE_LOCK = (
    REPO / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)
MODULE_SOURCE = (
    REPO / "scripts/native_shadow_boot_writer_set_acquire_arm64_v1.py"
).read_text(encoding="utf-8")


def selection_record() -> dict:
    return json.loads(SELECTION_RECORD.read_text(encoding="utf-8"))


def source_lock() -> dict:
    return json.loads(BOOT_SOURCE_LOCK.read_text(encoding="utf-8"))


class DerivedPlanTests(unittest.TestCase):
    """RED 1: the plan is read out of the sealed record, not written here."""

    def setUp(self) -> None:
        self.plan = mod.derive_plan()
        self.sealed = selection_record()["writerToolSet"]["packages"]

    def test_the_two_packages_are_the_ones_the_record_selected(self) -> None:
        self.assertEqual(
            sorted(str(row["sha256"]) for row in self.plan["artifacts"]),
            sorted(str(row["sha256"]) for row in self.sealed),
        )

    def test_each_size_is_the_record_s_and_not_a_number_from_here(self) -> None:
        sizes = {str(row["sha256"]): int(row["sizeBytes"]) for row in self.sealed}
        for row in self.plan["artifacts"]:
            self.assertEqual(row["sizeBytes"], sizes[str(row["sha256"])])
            self.assertNotIn(str(row["sizeBytes"]), MODULE_SOURCE)

    def test_no_sealed_digest_is_written_down_in_this_module(self) -> None:
        """A second copy of a sealed digest is a second thing that can drift."""

        for row in self.plan["artifacts"]:
            self.assertNotIn(str(row["sha256"]), MODULE_SOURCE)

    def test_the_urls_are_built_from_the_snapshot_the_record_pinned(self) -> None:
        base = selection_record()["writerToolSet"]["index"]["snapshotBase"]
        by_digest = {str(row["sha256"]): row for row in self.sealed}
        for row in self.plan["artifacts"]:
            sealed = by_digest[str(row["sha256"])]
            self.assertEqual(row["url"], f"{base}/{sealed['poolPath']}")

    def test_the_plan_pins_the_record_it_was_derived_from(self) -> None:
        raw = SELECTION_RECORD.read_bytes()
        self.assertEqual(
            self.plan["authorityInputs"]["selectionRecord"],
            {"sha256": hashlib.sha256(raw).hexdigest(), "sizeBytes": len(raw)},
        )

    def test_the_plan_says_it_claims_no_boot_and_no_activation(self) -> None:
        self.assertIs(self.plan["bootableClaim"], False)
        self.assertIs(self.plan["activationAllowed"], False)
        self.assertNotIn(True, set(self.plan["boundaries"].values()))


class AgreementTests(unittest.TestCase):
    """RED 2: the record and the production plan must still say the same thing."""

    def test_both_packages_are_the_ones_the_production_plan_pins(self) -> None:
        digests = {str(row["sha256"]) for row in mod.derive_plan()["artifacts"]}
        self.assertIn(root_disk.WRITER_PACKAGE_SHA256, digests)
        self.assertIn(root_disk.WRITER_LIBRARY_PACKAGE_SHA256, digests)

    def test_a_record_that_drifted_from_the_production_plan_is_refused(self) -> None:
        record = selection_record()
        record["writerToolSet"]["packages"][0]["sha256"] = "0" * 64
        with self.assertRaises(mod.WriterSetAcquisitionError) as caught:
            mod.derive_plan(record=record)
        self.assertIn("disagree", str(caught.exception))

    def test_a_package_from_another_version_is_refused(self) -> None:
        record = selection_record()
        record["writerToolSet"]["packages"][0]["version"] = "1.47.3-1"
        with self.assertRaises(mod.WriterSetAcquisitionError) as caught:
            mod.derive_plan(record=record)
        self.assertIn("selected", str(caught.exception))

    def test_a_record_naming_some_other_package_is_refused(self) -> None:
        record = selection_record()
        record["writerToolSet"]["packages"][0]["name"] = "coreutils"
        with self.assertRaises(mod.WriterSetAcquisitionError) as caught:
            mod.derive_plan(record=record)
        self.assertIn("does not know", str(caught.exception))

    def test_the_selected_version_is_the_one_the_record_calls_fixed(self) -> None:
        record = selection_record()
        self.assertEqual(record["selection"]["selected"]["verdict"], "FIXED")
        plan = mod.derive_plan()
        self.assertEqual(plan["selectedVersion"], record["selection"]["selected"]["version"])
        self.assertEqual(plan["selectedSuite"], record["selection"]["selected"]["suite"])


class DisjointFromTheGuestTests(unittest.TestCase):
    """RED 3: additional, never a substitution -- the 191 are untouched."""

    def setUp(self) -> None:
        self.plan = mod.derive_plan()
        self.lock = source_lock()

    def test_neither_package_is_one_the_guest_closure_already_holds(self) -> None:
        locked = {str(row["sha256"]) for row in self.lock["artifacts"]}
        for row in self.plan["artifacts"]:
            self.assertNotIn(str(row["sha256"]), locked, row["artifactId"])

    def test_neither_name_collides_with_a_locked_one(self) -> None:
        """A colliding name would be a second artifact claiming a sealed identity."""

        locked = {str(row["id"]) for row in self.lock["artifacts"]}
        for row in self.plan["artifacts"]:
            self.assertNotIn(str(row["artifactId"]), locked)

    def test_a_package_the_lock_already_seals_is_refused(self) -> None:
        record = selection_record()
        already = str(self.lock["artifacts"][0]["sha256"])
        record["writerToolSet"]["packages"][0]["sha256"] = already
        with self.assertRaises(mod.WriterSetAcquisitionError):
            mod.derive_plan(record=record)

    def test_the_guest_lock_digest_is_recorded_as_read_and_unchanged(self) -> None:
        raw = BOOT_SOURCE_LOCK.read_bytes()
        self.assertEqual(
            self.plan["authorityInputs"]["bootSourceLock"],
            {"sha256": hashlib.sha256(raw).hexdigest(), "sizeBytes": len(raw)},
        )
        self.assertIs(self.plan["boundaries"]["replacesAGuestPackage"], False)
        self.assertIs(self.plan["boundaries"]["deletesAGuestPackage"], False)

    def test_a_lock_that_is_not_the_measured_one_is_refused(self) -> None:
        record = selection_record()
        record["guestPackages"]["sourceLockSha256"] = "0" * 64
        with self.assertRaises(mod.WriterSetAcquisitionError) as caught:
            mod.derive_plan(record=record)
        self.assertIn("not the one the selection record measured", str(caught.exception))

    def test_a_record_that_admits_replacing_a_guest_package_is_refused(self) -> None:
        record = selection_record()
        record["guestPackages"]["replaced"] = True
        with self.assertRaises(mod.WriterSetAcquisitionError) as caught:
            mod.derive_plan(record=record)
        self.assertIn("untouched", str(caught.exception))

    def test_the_count_of_guest_packages_is_the_one_the_record_states(self) -> None:
        self.assertEqual(
            self.plan["guestArtifactCount"], selection_record()["guestPackages"]["count"]
        )


class SealedAcquirerUntouchedTests(unittest.TestCase):
    """RED 4: the frozen acquirer is pinned inside a sealed plan, so it stays put.

    Its source bytes are one of that plan's authority inputs.  Reaching into it
    for a shared request path -- even behind a default that preserves its
    behaviour exactly -- would put the sealed plan out of date, and bringing a
    sealed plan up to date is not a step this run is allowed to take.  So the
    request path here is a second copy on purpose, and these tests are what
    keeps the copy from becoming a loophole.
    """

    def test_the_frozen_acquirer_still_admits_only_its_own_snapshot(self) -> None:
        base = selection_record()["writerToolSet"]["index"]["snapshotBase"]
        with self.assertRaises(payload.PayloadAcquisitionError):
            payload._spec(
                {
                    "artifactId": "x",
                    "sha256": "a" * 64,
                    "sizeBytes": 1,
                    "url": f"{base}/pool/main/e/e2fsprogs/x.deb",
                }
            )

    def test_the_frozen_acquirer_still_matches_the_digest_its_plan_pins(self) -> None:
        source = REPO / "scripts/native_shadow_boot_rootfs_payload_acquire_arm64_v1.py"
        plan = json.loads(
            (
                REPO
                / "native/containment/native-shadow-boot-rootfs-payload-acquisition-plan-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        raw = source.read_bytes()
        self.assertEqual(
            plan["authorityInputs"]["payloadAcquirer"],
            {
                "sha256": payload.payload_acquirer_authority_sha256(raw),
                "sizeBytes": len(raw),
            },
        )


class SnapshotPolicyTests(unittest.TestCase):
    """RED 5: one acquirer, one snapshot, one request, nothing negotiable."""

    NOBLE = f"{payload.SNAPSHOT_BASE}/pool/main/e/e2fsprogs/x.deb"

    def setUp(self) -> None:
        self.prefix = mod.derive_plan()["networkPolicy"]["snapshotPathPrefix"]

    def spec(self, url: str, *, size: int = 1) -> dict:
        return {"artifactId": "x", "sha256": "a" * 64, "sizeBytes": size, "url": url}

    def plucky(self, path: str = "pool/main/e/e2fsprogs/x.deb") -> str:
        base = selection_record()["writerToolSet"]["index"]["snapshotBase"]
        return f"{base}/{path}"

    def test_the_selected_snapshot_is_admitted_and_the_frozen_one_is_not(self) -> None:
        mod._spec(self.spec(self.plucky()), snapshot_prefix=self.prefix)
        with self.assertRaises(mod.WriterSetAcquisitionError):
            mod._spec(self.spec(self.NOBLE), snapshot_prefix=self.prefix)

    def test_a_url_that_climbs_or_hides_is_refused(self) -> None:
        for url in (
            self.plucky("pool/../../etc/passwd"),
            self.plucky("pool//main/x.deb"),
            self.plucky("pool/main/%2e%2e/x.deb"),
            "http://snapshot.ubuntu.com/ubuntu/20260801T000000Z/pool/x.deb",
            "https://snapshot.ubuntu.com.invalid/ubuntu/20260801T000000Z/pool/x.deb",
            "https://user@snapshot.ubuntu.com/ubuntu/20260801T000000Z/pool/x.deb",
            self.plucky("pool/x.deb?mirror=elsewhere"),
        ):
            with self.subTest(url=url):
                with self.assertRaises(mod.WriterSetAcquisitionError):
                    mod._spec(self.spec(url), snapshot_prefix=self.prefix)

    def test_two_artifacts_with_one_identity_are_refused(self) -> None:
        rows = [self.spec(self.plucky("pool/a.deb")), self.spec(self.plucky("pool/b.deb"))]
        with self.assertRaises(mod.WriterSetAcquisitionError):
            mod._ordered_unique(rows, snapshot_prefix=self.prefix)

    def test_the_fetcher_refuses_an_off_snapshot_url_before_connecting(self) -> None:
        opened = 0

        def factory(*args: object, **kwargs: object):
            nonlocal opened
            opened += 1

        fetch = mod.snapshot_stream_for(mod.derive_plan())
        with self.assertRaises(mod.WriterSetAcquisitionError):
            list(fetch(self.spec(self.NOBLE)))
        self.assertEqual(opened, 0)

    def test_one_plain_get_over_tls12_with_no_proxy_redirect_retry_or_range(self) -> None:
        raw = b"writer-set-payload"
        calls: list[tuple] = []
        remaining = [raw[:6], raw[6:]]

        class Response:
            status = 200

            @staticmethod
            def getheader(name: str):
                return {"Content-Length": str(len(raw)), "Content-Encoding": None}.get(name)

            def read(self, amount: int) -> bytes:
                return remaining.pop(0) if remaining else b""

        class Connection:
            def __init__(self, host, port, *, timeout, context):
                calls.append(("connect", host, port, timeout))

            def putrequest(self, method, path, **kwargs):
                calls.append(("request", method, path, kwargs))

            def putheader(self, name, value):
                calls.append(("header", name, value))

            def endheaders(self):
                calls.append(("endheaders",))

            def getresponse(self):
                return Response()

            def close(self):
                calls.append(("close",))

        context = ssl.create_default_context()
        with mock.patch.dict(
            "os.environ", {"HTTPS_PROXY": "http://attacker.invalid:8080"}, clear=False
        ):
            chunks = list(
                mod.snapshot_https_stream(
                    self.spec(self.plucky(), size=len(raw)),
                    snapshot_prefix=self.prefix,
                    connection_factory=Connection,
                    context_factory=lambda: context,
                )
            )
        self.assertEqual(b"".join(chunks), raw)
        self.assertEqual(context.minimum_version, ssl.TLSVersion.TLSv1_2)
        self.assertEqual(context.verify_mode, ssl.CERT_REQUIRED)
        self.assertTrue(context.check_hostname)
        self.assertEqual(len([call for call in calls if call[0] == "connect"]), 1)
        self.assertEqual(
            [call for call in calls if call[0] == "request"],
            [("request", "GET", "/ubuntu/20260801T000000Z/pool/main/e/e2fsprogs/x.deb",
              {"skip_host": True, "skip_accept_encoding": True})],
        )
        self.assertIn(("header", "Accept-Encoding", "identity"), calls)
        self.assertFalse(any(call[:2] == ("header", "Range") for call in calls))
        self.assertIn(("close",), calls)

    def test_a_redirect_a_recoding_or_a_wrong_length_stops_without_a_retry(self) -> None:
        raw = b"writer-set-payload"
        for status, encoding, length in (
            (302, None, len(raw)),
            (200, "gzip", len(raw)),
            (200, None, len(raw) + 1),
        ):
            opened = 0

            class Response:
                @staticmethod
                def getheader(name: str):
                    return {"Content-Length": str(length), "Content-Encoding": encoding}.get(
                        name
                    )

                @staticmethod
                def read(amount: int) -> bytes:
                    return raw

            Response.status = status

            class Connection:
                def __init__(self, *args, **kwargs):
                    nonlocal opened
                    opened += 1

                def putrequest(self, *args, **kwargs):
                    pass

                def putheader(self, *args, **kwargs):
                    pass

                def endheaders(self):
                    pass

                def getresponse(self):
                    return Response()

                def close(self):
                    pass

            with self.subTest(status=status, encoding=encoding, length=length):
                with self.assertRaises(mod.WriterSetAcquisitionError):
                    list(
                        mod.snapshot_https_stream(
                            self.spec(self.plucky(), size=len(raw)),
                            snapshot_prefix=self.prefix,
                            connection_factory=Connection,
                        )
                    )
                self.assertEqual(opened, 1)

    def test_only_the_snapshot_host_is_allowed(self) -> None:
        self.assertEqual(
            mod.derive_plan()["networkPolicy"]["allowedHosts"], [mod.SNAPSHOT_HOST]
        )


class AcquireTests(unittest.TestCase):
    """RED 5: fetch what is missing, reuse what is there, refuse the rest.

    The plan used here is a stand-in with digests of bytes that can actually be
    produced.  The sealed plan's digests belong to packages nobody has fetched
    yet, and a store cannot be seeded with bytes that hash to them -- which is
    the property the store is for.  What this exercises is the fetch, reuse and
    refusal behaviour; that the sealed plan is the one acquired is the subject
    of the tests above.
    """

    def enterContext(self, cm):  # Python 3.9 has no unittest.enterContext
        entered = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return entered

    def setUp(self) -> None:
        self.cas = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory())) / "cas"
        self.bodies = {}
        artifacts = []
        base = selection_record()["writerToolSet"]["index"]["snapshotBase"]
        for name in ("e2fsprogs", "libext2fs2t64"):
            raw = f"pretend {name} package".encode("utf-8")
            digest = hashlib.sha256(raw).hexdigest()
            self.bodies[digest] = raw
            artifacts.append(
                {
                    "artifactId": mod.ARTIFACT_ID_PREFIX + name,
                    "sha256": digest,
                    "sizeBytes": len(raw),
                    "url": f"{base}/pool/main/e/e2fsprogs/{name}.deb",
                }
            )
        self.plan = dict(
            mod.derive_plan(),
            artifacts=artifacts,
            expected={
                "fetchBytes": sum(len(raw) for raw in self.bodies.values()),
                "fetchCount": len(artifacts),
            },
        )

    def stream(self, spec):
        return [self.bodies[str(spec["sha256"])]]

    def refuse(self, spec):
        raise AssertionError(f"{spec['artifactId']} was fetched from a store hit")

    def test_what_the_store_lacks_is_fetched_and_lands_under_its_digest(self) -> None:
        result = mod.acquire(cas=self.cas, plan=self.plan, stream_factory=self.stream)
        self.assertEqual(result["summary"]["fetchedCount"], 2)
        self.assertEqual(result["summary"]["reusedCount"], 0)
        for digest, raw in self.bodies.items():
            self.assertEqual((self.cas / "sha256" / digest).read_bytes(), raw)

    def test_a_store_that_already_holds_them_is_not_asked_to_fetch(self) -> None:
        """The standing rule is that a store hit costs no network request."""

        mod.acquire(cas=self.cas, plan=self.plan, stream_factory=self.stream)
        result = mod.acquire(cas=self.cas, plan=self.plan, stream_factory=self.refuse)
        self.assertEqual(result["summary"]["fetchedCount"], 0)
        self.assertEqual(result["summary"]["reusedCount"], 2)
        self.assertEqual(result["summary"]["fetchedBytes"], 0)

    def test_bytes_that_do_not_reproduce_the_sealed_digest_are_refused(self) -> None:
        def wrong(spec):
            return [b"something else entirely"]

        with self.assertRaises(mod.WriterSetAcquisitionError):
            mod.acquire(cas=self.cas, plan=self.plan, stream_factory=wrong)
        for digest in self.bodies:
            self.assertFalse((self.cas / "sha256" / digest).exists())

    def test_the_result_records_the_plan_it_acted_on(self) -> None:
        result = mod.acquire(cas=self.cas, plan=self.plan, stream_factory=self.stream)
        self.assertEqual(
            result["planSha256"],
            hashlib.sha256(payload.canonical_json(self.plan)).hexdigest(),
        )
        self.assertIs(result["bootableClaim"], False)
        self.assertIs(result["activationAllowed"], False)
        self.assertIn("NOT-BOOT-AUTHORITY", result["status"])

    def test_holding_the_bytes_raises_nothing_beyond_holding_them(self) -> None:
        result = mod.acquire(cas=self.cas, plan=self.plan, stream_factory=self.stream)
        raised = {key for key, value in result["boundaries"].items() if value}
        self.assertEqual(
            raised, {"writerSetPayloadsAcquired", "writerSetPayloadsVerified"}
        )

    def test_the_result_is_written_as_canonical_json_when_asked(self) -> None:
        path = self.cas.parent / "result.json"
        result = mod.acquire(
            cas=self.cas, plan=self.plan, stream_factory=self.stream, result=path
        )
        self.assertEqual(path.read_bytes(), payload.canonical_json(result))


class GateTests(unittest.TestCase):
    """The two scripts that add a writer are wired into both gates.

    Everything else that reaches the network on the production runner is named
    in the docs gate.  These two are the newest things that do, and they are
    the ones that decide which bytes write the image, so being unlisted there
    is the wrong kind of quiet.
    """

    def smoke(self) -> str:
        return (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")

    def test_both_scripts_are_pinned_by_the_docs_gate(self) -> None:
        smoke = self.smoke()
        for name in (
            "scripts/native_shadow_boot_writer_set_acquire_arm64_v1.py",
            "scripts/native_shadow_boot_writer_tree_arm64_v1.py",
        ):
            self.assertTrue(name in smoke, f"{name} is not pinned by docs-smoke")

    def test_both_test_modules_are_required_to_stay_registered(self) -> None:
        smoke = self.smoke()
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        for name in (
            "scripts/test_native_shadow_boot_writer_set_acquire_arm64_v1.py",
            "scripts/test_native_shadow_boot_writer_tree_arm64_v1.py",
        ):
            self.assertTrue(name in self_test, f"{name} is not run by scripts/self-test.sh")
            self.assertTrue(
                name in smoke, f"docs-smoke does not require {name} to stay registered"
            )


if __name__ == "__main__":
    unittest.main()
