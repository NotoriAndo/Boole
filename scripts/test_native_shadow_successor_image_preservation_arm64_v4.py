"""What the preservation record has to say, and what it may not claim.

The production budget is spent.  These three files cannot be made again, the
GitHub artifacts expire, and the download that proved them sat in a directory
the operating system is free to delete.  So a copy was made somewhere that
survives a reboot, and this gate is what stops the record of that copy from
drifting into either of the two lies available to it: that the images are
safer than they are, or that having them means anything about booting them.

The archive itself lives outside the repository on one machine.  Continuous
integration cannot see it, so the checks that need it run only where it is
present, and the checks that bind the record to the repository run everywhere.
"""

import hashlib
import json
import pathlib
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO_ROOT / "native" / "containment"
PRESERVATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-preservation-arm64-v4.json"
)
RESULT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-production-result-arm64-v4.json"
)
AUTHORITY_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-production-authority-arm64-v4.json"
)
PRODUCER_FINGERPRINT_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-producer-fingerprint-arm64-v4.json"
)

IMAGE_NAMES = ("guest-kernel", "guest-initrd", "guest-root-disk")


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def digest_of(path: pathlib.Path) -> str:
    handle = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            handle.update(chunk)
    return handle.hexdigest()


class PreservationRecordTests(unittest.TestCase):
    """The record, checked against the repository it was written in."""

    def setUp(self) -> None:
        self.document = read_json(PRESERVATION_PATH)
        self.result = read_json(RESULT_PATH)

    def test_it_is_about_the_attempt_that_produced_the_images(self) -> None:
        self.assertEqual(self.document["attemptId"], self.result["attemptId"])
        self.assertEqual(self.document["source"]["runId"], self.result["runId"])
        self.assertEqual(self.document["source"]["headSha"], self.result["headSha"])

    def test_the_three_images_are_the_ones_the_run_produced(self) -> None:
        """A preservation record naming other digests preserves other files."""

        produced = self.result["outputs"]
        preserved = {row["name"]: row["sha256"] for row in self.document["images"]}
        self.assertEqual(set(preserved), set(IMAGE_NAMES))
        self.assertEqual(preserved, {name: produced[name] for name in IMAGE_NAMES})
        for row in self.document["images"]:
            self.assertGreater(row["bytes"], 0, msg=row["name"])

    def test_it_binds_the_records_that_authorised_and_recorded_the_run(self) -> None:
        for key, path in (
            ("authority", AUTHORITY_PATH),
            ("producerFingerprint", PRODUCER_FINGERPRINT_PATH),
            ("resultDocument", RESULT_PATH),
        ):
            row = self.document[key]
            self.assertEqual(REPO_ROOT / row["path"], path, msg=key)
            self.assertEqual(row["sha256"], digest_of(path), msg=key)

    def test_every_artifact_it_names_says_when_it_expires(self) -> None:
        """The expiry is the reason this record exists; it has to be in it."""

        artifacts = self.document["artifacts"]
        self.assertEqual(len(artifacts), 6)
        self.assertEqual(
            sorted(row["name"] for row in artifacts),
            [
                "successor-attempt-consumed-1",
                "successor-attempt-consumed-2",
                "successor-manifest-1",
                "successor-manifest-2",
                "successor-outputs-1",
                "successor-outputs-2",
            ],
        )
        for row in artifacts:
            self.assertIsInstance(row["id"], int)
            self.assertGreater(row["sizeInBytes"], 0)
            self.assertTrue(row["expiresAt"].endswith("Z"), msg=row["name"])
            self.assertTrue(row["githubDigest"].startswith("sha256:"), msg=row["name"])
            self.assertIn(row["archivedAs"], row["name"])

    def test_every_preserved_file_carries_a_digest_and_a_size(self) -> None:
        rows = self.document["preservedFiles"]
        self.assertEqual(len(rows), 18)
        for row in rows:
            self.assertEqual(len(row["sha256"]), 64, msg=row["path"])
            self.assertGreater(row["bytes"], 0, msg=row["path"])
        self.assertEqual(
            self.document["archive"]["totalBytes"],
            sum(row["bytes"] for row in rows),
            msg="the total is the rows added up, or it is decoration",
        )

    def test_both_replicas_are_preserved_whole(self) -> None:
        """Keeping only the summary of the second one would have been enough.

        It was not done that way: the budget is zero, the two sets are the only
        copies that will ever exist, and a second set of bytes on the same disk
        is the cheapest insurance available against one of them going bad.
        """

        identity = self.document["replicaIdentity"]
        self.assertTrue(identity["bothReplicasPreservedInFull"])
        self.assertTrue(identity["byteIdenticalAtTheArchive"])
        self.assertEqual(sorted(identity["filesCompared"]), sorted(IMAGE_NAMES))
        self.assertTrue(identity["whyBothWereKept"].strip())
        preserved = {row["path"].split("/", 1)[0] for row in self.document["preservedFiles"]}
        self.assertEqual(len(preserved), 6)

    def test_it_says_how_the_copy_was_made_and_how_often_it_was_checked(self) -> None:
        archive = self.document["archive"]
        self.assertTrue(archive["machineLocal"])
        self.assertGreaterEqual(archive["verificationPasses"], 2)
        self.assertTrue(archive["howItWasCopied"].strip())
        self.assertEqual(archive["appliedMode"]["files"], "0444")
        self.assertEqual(archive["appliedMode"]["directories"], "0555")

    def test_nothing_was_deleted_to_make_room_for_it(self) -> None:
        """Preservation that destroys its own source is not preservation."""

        kept = self.document["whatWasNotDeleted"]
        joined = " ".join(kept).lower()
        self.assertIn("artifact", joined)
        self.assertIn("temporary", joined)
        self.assertFalse(self.document["anythingDeleted"])

    def test_it_admits_what_one_copy_on_one_machine_is_not(self) -> None:
        """The honest limits, written down rather than left to be assumed.

        One disk in one building is not a backup.  A record that says it is
        preserved without saying where the single point of failure sits invites
        exactly the loss it was written to prevent.
        """

        limits = self.document["whatThisDoesNotDo"]
        joined = " ".join(limits).lower()
        for phrase in ("offsite", "single", "not a backup"):
            self.assertIn(phrase, joined)
        boundaries = self.document["boundaries"]
        self.assertFalse(boundaries["offsiteCopyExists"])
        self.assertFalse(boundaries["integrityMonitored"])

    def test_it_claims_preservation_and_nothing_further(self) -> None:
        boundaries = self.document["boundaries"]
        self.assertTrue(boundaries["imagePreservedClaim"])
        for key in (
            "activationAllowed",
            "bootableClaim",
            "guestBootVerified",
            "integrityMonitored",
            "offsiteCopyExists",
            "publicMiningOrBenchmark",
            "servingClaim",
        ):
            self.assertFalse(boundaries[key], msg=key)

    def test_it_does_not_put_the_images_in_the_repository(self) -> None:
        """Large binaries stay out of git; only their names and digests come in."""

        self.assertFalse(self.document["archive"]["committedToTheRepository"])
        self.assertFalse(
            (REPO_ROOT / "native" / "containment" / "guest-root-disk").exists()
        )
        for row in self.document["images"]:
            self.assertLess(
                len(json.dumps(row)), 4096, msg="a digest, not a payload"
            )

    def test_the_budget_is_why_this_matters_and_it_says_so(self) -> None:
        self.assertEqual(
            self.document["productionAttemptsRemaining"],
            self.result["attemptAccounting"]["productionAttemptsRemainingAfterThisRun"],
        )
        self.assertEqual(self.document["productionAttemptsRemaining"], 0)
        self.assertIn("again", self.document["whyThisExists"].lower())

    def test_it_is_canonical(self) -> None:
        self.assertEqual(
            PRESERVATION_PATH.read_bytes(),
            (json.dumps(self.document, indent=2, sort_keys=True) + "\n").encode("utf-8"),
        )


class ArchiveOnDiskTests(unittest.TestCase):
    """The archive itself, where it exists.

    It lives on one machine outside the repository, so these checks skip rather
    than fail elsewhere.  Skipping is the honest outcome: a continuous
    integration runner has nothing to say about a disk it cannot see.
    """

    def setUp(self) -> None:
        self.document = read_json(PRESERVATION_PATH)
        self.root = pathlib.Path(self.document["archive"]["root"])
        if not self.root.is_dir():
            raise unittest.SkipTest(f"archive not present on this machine: {self.root}")

    def test_every_preserved_file_is_there_with_the_digest_recorded(self) -> None:
        for row in self.document["preservedFiles"]:
            path = self.root / row["path"]
            self.assertTrue(path.exists(), msg=row["path"])
            self.assertEqual(path.stat().st_size, row["bytes"], msg=row["path"])
            self.assertEqual(digest_of(path), row["sha256"], msg=row["path"])

    def test_the_archive_holds_nothing_the_record_does_not_name(self) -> None:
        """An extra file in an archive is an unrecorded claim about it."""

        named = {row["path"] for row in self.document["preservedFiles"]}
        named.add(self.document["archive"]["selfDescribingCopy"])
        found = {
            str(path.relative_to(self.root))
            for path in self.root.rglob("*")
            if path.is_file()
        }
        self.assertEqual(found, named)

    def test_it_is_read_only(self) -> None:
        for path in self.root.rglob("*"):
            mode = path.stat().st_mode & 0o777
            self.assertEqual(mode, 0o555 if path.is_dir() else 0o444, msg=str(path))
        self.assertEqual(self.root.stat().st_mode & 0o777, 0o555)

    def test_the_archive_carries_its_own_copy_of_this_record(self) -> None:
        """Separated from the repository, the archive still explains itself."""

        copy = self.root / self.document["archive"]["selfDescribingCopy"]
        self.assertTrue(copy.exists())
        self.assertEqual(digest_of(copy), digest_of(PRESERVATION_PATH))

    def test_the_two_replicas_are_still_byte_identical_on_disk(self) -> None:
        for name in IMAGE_NAMES:
            first = (self.root / "successor-outputs-1" / name).read_bytes()
            second = (self.root / "successor-outputs-2" / name).read_bytes()
            self.assertEqual(
                hashlib.sha256(first).hexdigest(),
                hashlib.sha256(second).hexdigest(),
                msg=name,
            )


if __name__ == "__main__":
    unittest.main()
