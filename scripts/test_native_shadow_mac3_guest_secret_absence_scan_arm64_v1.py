"""The read-only search that answers one of the five stopped conditions.

The sealed condition asks that the produced image be searched for the known
secret-bearing filenames and for the host's own wallet and key directories, and
that they have no entry.  Nothing had been written that does the searching, so
the condition sat in the hard stop with the four that need the guest to speak --
even though this one needs nothing from the guest at all.

A byte search over the whole image is a superset of "has an entry": it also
reads file contents and blocks no directory points at any more.  That asymmetry
is the whole design.  Nothing found means the subset is empty and the condition
is answered.  Something found is not a failure by itself and not a pass either;
it is a hit that has to be explained before the condition can be judged, and
until it is, the answer is no.

The searching must not disturb what it reads.  The image is opened read-only,
its digest is taken before and after, and the record it writes carries offsets
and counts but never the bytes around a hit -- a report that quoted its
findings would be a report that copied the secret it was looking for.
"""

import hashlib
import importlib
import json
import os
import pathlib
import sys
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]


def scanner():
    if str(REPO / "scripts") not in sys.path:
        sys.path.insert(0, str(REPO / "scripts"))
    return importlib.import_module("native_shadow_mac3_guest_secret_absence_scan_arm64_v1")


class MarkerTableTests(unittest.TestCase):
    """What is searched for, and where each marker came from."""

    def setUp(self) -> None:
        self.module = scanner()

    def test_every_marker_says_what_it_is_and_why_it_is_searched_for(self) -> None:
        markers = self.module.markers()
        self.assertTrue(markers)
        for marker in markers:
            self.assertTrue(marker["id"])
            self.assertIn(marker["tier"], self.module.TIERS)
            self.assertTrue(marker["why"].strip(), marker["id"])
            self.assertIsInstance(marker["needle"], bytes)
            self.assertGreaterEqual(len(marker["needle"]), 4, marker["id"])

    def test_marker_ids_are_unique(self) -> None:
        identifiers = [marker["id"] for marker in self.module.markers()]
        self.assertEqual(len(identifiers), len(set(identifiers)))

    def test_the_hosts_own_directories_are_read_from_this_machine(self) -> None:
        """Not a literal in the file: the host is whoever is running it."""

        home = str(pathlib.Path.home()).encode("utf-8")
        needles = [marker["needle"] for marker in self.module.markers()]
        self.assertIn(home, needles)

    def test_the_wallet_and_key_directories_the_node_uses_are_covered(self) -> None:
        needles = b"\n".join(marker["needle"] for marker in self.module.markers())
        for expected in (b".boole/keys", b"BOOLE_WALLET_PASSPHRASE", b"BOOLE_LLM_API_KEY"):
            self.assertIn(expected, needles, expected)

    def test_a_hit_on_a_host_identity_marker_can_only_come_from_this_host(self) -> None:
        for marker in self.module.markers():
            if marker["tier"] != "host-identity":
                continue
            self.assertTrue(marker["anyHitIsAFailure"], marker["id"])


class FindingTests(unittest.TestCase):
    """The search itself, over buffers small enough to reason about."""

    def setUp(self) -> None:
        self.module = scanner()
        self.marker = {
            "id": "test-marker",
            "needle": b"SECRETVALUE",
            "tier": "secret-shape",
            "why": "a test marker",
            "anyHitIsAFailure": False,
        }

    def scan(self, payload: bytes, chunk_bytes: int):
        import io

        return self.module.scan_stream(
            io.BytesIO(payload), [self.marker], chunk_bytes=chunk_bytes
        )

    def test_a_clean_buffer_reports_nothing(self) -> None:
        self.assertEqual(self.scan(b"nothing to find here" * 100, 16), [])

    def test_a_marker_is_found_at_the_offset_it_sits_at(self) -> None:
        payload = b"." * 40 + b"SECRETVALUE" + b"." * 40
        hits = self.scan(payload, 4096)
        self.assertEqual([hit["offset"] for hit in hits], [40])
        self.assertEqual(hits[0]["marker"], "test-marker")

    def test_a_marker_split_across_two_reads_is_still_found(self) -> None:
        """The bug this kind of scanner has by default.

        Read the image in blocks and search each block on its own, and anything
        lying across a block boundary is invisible.  On a two gigabyte image
        that is hundreds of blind spots, each one exactly the width of the thing
        being searched for.
        """

        payload = b"." * 10 + b"SECRETVALUE" + b"." * 10
        for chunk_bytes in range(4, 24):
            with self.subTest(chunk_bytes=chunk_bytes):
                hits = self.scan(payload, chunk_bytes)
                self.assertEqual([hit["offset"] for hit in hits], [10])

    def test_every_occurrence_is_counted_not_just_the_first(self) -> None:
        payload = b"SECRETVALUE" + b"." * 5 + b"SECRETVALUE"
        hits = self.scan(payload, 7)
        self.assertEqual([hit["offset"] for hit in hits], [0, 16])

    def test_a_hit_never_carries_the_bytes_around_it(self) -> None:
        """A report that quoted its findings would copy what it went looking for."""

        payload = b"leading context SECRETVALUE trailing context"
        hits = self.scan(payload, 4096)
        self.assertEqual(len(hits), 1)
        serialised = json.dumps(hits[0]).encode("utf-8")
        self.assertNotIn(b"SECRETVALUE", serialised)
        self.assertNotIn(b"leading context", serialised)
        self.assertEqual(sorted(hits[0]), ["marker", "offset", "tier"])


class ReadOnlyTests(unittest.TestCase):
    """It must not disturb the sealed image it is reading."""

    def setUp(self) -> None:
        self.module = scanner()
        self.temporary = tempfile.TemporaryDirectory()
        self.target = pathlib.Path(self.temporary.name) / "image"
        self.target.write_bytes(b"clean image with no markers in it" * 500)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def digest(self) -> str:
        return hashlib.sha256(self.target.read_bytes()).hexdigest()

    def test_scanning_leaves_the_file_byte_for_byte_identical(self) -> None:
        before = self.digest()
        before_stat = self.target.stat()
        self.module.scan_target(self.target, expected_sha256=before)
        self.assertEqual(self.digest(), before)
        self.assertEqual(self.target.stat().st_size, before_stat.st_size)
        self.assertEqual(self.target.stat().st_mtime, before_stat.st_mtime)

    def test_a_file_with_no_write_permission_can_still_be_scanned(self) -> None:
        os.chmod(self.target, 0o444)
        report = self.module.scan_target(self.target, expected_sha256=self.digest())
        self.assertEqual(report["hits"], [])

    def test_the_digest_is_taken_on_both_sides_and_must_agree(self) -> None:
        report = self.module.scan_target(self.target, expected_sha256=self.digest())
        self.assertEqual(report["sha256Before"], report["sha256After"])
        self.assertEqual(report["sha256Before"], self.digest())

    def test_scanning_something_that_is_not_the_sealed_file_refuses(self) -> None:
        with self.assertRaises(self.module.RefusedError):
            self.module.scan_target(self.target, expected_sha256="0" * 64)


class VerdictTests(unittest.TestCase):
    """Nothing found answers the condition. Anything found does not."""

    def setUp(self) -> None:
        self.module = scanner()

    def test_no_hits_answers_the_condition(self) -> None:
        verdict = self.module.verdict([])
        self.assertTrue(verdict["noEntryFound"])
        self.assertTrue(verdict["why"].strip())

    def test_a_host_identity_hit_is_a_failure_outright(self) -> None:
        verdict = self.module.verdict(
            [{"marker": "host-home-directory", "offset": 17, "tier": "host-identity"}]
        )
        self.assertFalse(verdict["noEntryFound"])
        self.assertIn("host-home-directory", verdict["why"])

    def test_a_generic_hit_is_not_a_pass_until_it_is_explained(self) -> None:
        """Fail-closed: an unexplained hit reads as no, never as probably fine."""

        verdict = self.module.verdict(
            [{"marker": "openssh-private-key-header", "offset": 5, "tier": "secret-shape"}]
        )
        self.assertFalse(verdict["noEntryFound"])
        self.assertIn("explained", verdict["why"])

    def test_the_verdict_counts_each_tier_on_its_own(self) -> None:
        """The two tiers mean different things, so one total hides the answer.

        A reader of this record needs the host-identity count without having to
        add the hits up themselves: it is the number that says whether anything
        of this machine's got in, and a page of generic matches must not be
        able to bury it.
        """

        verdict = self.module.verdict(
            [
                {"marker": "netrc-credentials-file", "offset": 3, "tier": "secret-shape"},
                {"marker": "bip39-mnemonic-field", "offset": 9, "tier": "secret-shape"},
            ]
        )
        self.assertEqual(verdict["hitsByTier"]["host-identity"], 0)
        self.assertEqual(verdict["hitsByTier"]["secret-shape"], 2)

    def test_every_tier_is_counted_even_when_it_found_nothing(self) -> None:
        """A missing key reads as 'not checked'; a zero reads as 'checked, none'."""

        verdict = self.module.verdict([])
        self.assertEqual(sorted(verdict["hitsByTier"]), sorted(self.module.TIERS))
        self.assertEqual(set(verdict["hitsByTier"].values()), {0})

    def test_the_search_being_wider_than_the_condition_is_stated(self) -> None:
        """Why zero hits settles it: the search covers more than 'has an entry'."""

        self.assertIn("superset", self.module.WHY_A_BYTE_SEARCH_SETTLES_IT)


class RecordTests(unittest.TestCase):
    """What it writes down, and what it must never write down."""

    def setUp(self) -> None:
        self.module = scanner()
        self.temporary = tempfile.TemporaryDirectory()
        self.target = pathlib.Path(self.temporary.name) / "image"
        self.target.write_bytes(b"clean image" * 100)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_the_record_names_the_host_directory_without_disclosing_it(self) -> None:
        """The operator's home path is evidence, not something to publish."""

        record = self.module.build_record(
            target="guest-root-disk",
            path=self.target,
            expected_sha256=hashlib.sha256(self.target.read_bytes()).hexdigest(),
        )
        serialised = json.dumps(record)
        self.assertNotIn(str(pathlib.Path.home()), serialised)
        redacted = [
            row
            for row in record["markersSearched"]
            if row["id"] == "host-home-directory"
        ]
        self.assertEqual(len(redacted), 1)
        self.assertEqual(
            redacted[0]["needleSha256"],
            hashlib.sha256(str(pathlib.Path.home()).encode("utf-8")).hexdigest(),
        )

    def test_the_record_states_it_authorises_no_boot(self) -> None:
        record = self.module.build_record(
            target="guest-root-disk",
            path=self.target,
            expected_sha256=hashlib.sha256(self.target.read_bytes()).hexdigest(),
        )
        self.assertFalse(record["bootAuthorisation"]["grantedByThisRecord"])

    def test_the_record_says_every_byte_was_read(self) -> None:
        record = self.module.build_record(
            target="guest-root-disk",
            path=self.target,
            expected_sha256=hashlib.sha256(self.target.read_bytes()).hexdigest(),
        )
        self.assertEqual(record["bytesRead"], self.target.stat().st_size)
        self.assertTrue(record["wholeFileRead"])


class GateTests(unittest.TestCase):
    """It stays wired into the checks that run on every push."""

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        name = pathlib.Path(__file__).name
        text = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        # Asserted as a boolean rather than with assertIn: the gate script is
        # one very long line, and a failure that prints all of it buries the
        # one word that says what went wrong.
        self.assertTrue(name in text, "%s is not registered in self-test.sh" % name)


if __name__ == "__main__":
    unittest.main()
