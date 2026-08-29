"""The runner for the third attempt, and the contract that says how it judges.

The criteria were sealed first and deliberately say nothing about the program
that would carry them out.  Between the two sits a gap wide enough to spend the
one boot in: a runner that knows only the first two attempts, judging rules for
six of the twenty-one conditions, a one-use mark written after the machine
stops rather than before it starts, and an after-run re-hash covering one file
of three.  Any of those turns an approved boot into a spent boot with no
verdict.

So this gate holds four things.  The contract is append-only beside the sealed
criteria and never edits them.  Every condition has a rule, checked before a
machine is built rather than after one has run.  The one-use mark is claimed
before the machine starts, so a crash costs the attempt rather than hiding it.
And a condition this image cannot show evidence for is a hard stop, not a
guess: the runner refuses, and the boot stays unspent.
"""

import hashlib
import importlib
import json
import pathlib
import sys
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native" / "containment"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
)
CONTRACT_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-execution-contract-arm64-v3.json"
)
PRESERVATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-successor-image-preservation-arm64-v4.json"
)

ATTEMPT = "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3"

EVIDENCE_SOURCES = (
    "console-transcript",
    "host-receipt",
    "not-observable-with-this-image",
    "run-receipt-and-host-check",
    "runner-computed",
    "sealed-record",
)

IMAGE_NAMES = ("guest-initrd", "guest-kernel", "guest-root-disk")


def read(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def digest(path: pathlib.Path) -> str:
    handle = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            handle.update(chunk)
    return handle.hexdigest()


def contract() -> dict:
    return read(CONTRACT_PATH)


def qualification() -> dict:
    return read(QUALIFICATION_PATH)


def condition_ids() -> set:
    return {row["id"] for row in qualification()["passConditions"]}


def driver():
    if str(REPO / "scripts") not in sys.path:
        sys.path.insert(0, str(REPO / "scripts"))
    return importlib.import_module("native_shadow_mac3_closed_local_boot_arm64_v3")


class ContractIsAppendOnlyTests(unittest.TestCase):
    """It sits beside the sealed criteria and does not touch them."""

    def test_it_binds_the_sealed_criteria_at_the_digest_on_disk(self) -> None:
        row = contract()["qualification"]
        self.assertEqual(row["path"], QUALIFICATION_PATH.relative_to(REPO).as_posix())
        self.assertEqual(row["sha256"], digest(QUALIFICATION_PATH))

    def test_it_says_in_words_that_it_changes_no_condition(self) -> None:
        record = contract()
        self.assertFalse(record["changesAnyPassCondition"])
        self.assertTrue(record["whyTheCriteriaAreNotEdited"].strip())

    def test_it_is_its_own_record_rather_than_a_second_edition(self) -> None:
        record = contract()
        sealed = qualification()
        for field in ("record", "release", "schema"):
            self.assertNotEqual(record[field], sealed[field], field)
        self.assertEqual(record["attemptId"], ATTEMPT)
        self.assertEqual(record["attemptId"], sealed["attemptId"])

    def test_it_grants_no_boot_authorisation_either(self) -> None:
        """Neither document opens the run; the approval is still separate."""

        authorisation = contract()["bootAuthorisation"]
        self.assertFalse(authorisation["grantedByThisRecord"])
        self.assertTrue(authorisation["requiredBefore"].strip())

    def test_the_records_it_leans_on_are_bound_and_unedited(self) -> None:
        for row in contract()["appendOnly"]["recordsLeftByteUnchanged"]:
            path = REPO / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])

    def test_it_is_canonical(self) -> None:
        self.assertEqual(
            CONTRACT_PATH.read_bytes(),
            (json.dumps(contract(), indent=2, sort_keys=True) + "\n").encode("utf-8"),
        )


class EvidencePlanTests(unittest.TestCase):
    """Every sealed condition is answered by a named source of evidence."""

    def plan(self) -> list:
        return contract()["evidencePlan"]

    def test_every_sealed_condition_appears_exactly_once(self) -> None:
        listed = [row["id"] for row in self.plan()]
        self.assertEqual(len(listed), len(set(listed)))
        self.assertEqual(set(listed), condition_ids())

    def test_each_row_names_a_source_from_the_fixed_vocabulary(self) -> None:
        for row in self.plan():
            self.assertIn(row["evidenceSource"], EVIDENCE_SOURCES, row["id"])
            self.assertTrue(row["how"].strip(), row["id"])

    def test_rows_resting_on_a_sealed_record_name_it_and_it_matches(self) -> None:
        """A sealed check is evidence only if the file still reads that way."""

        for row in self.plan():
            if row["evidenceSource"] != "sealed-record":
                continue
            sealed = row["sealedEvidence"]
            path = REPO / sealed["path"]
            self.assertTrue(path.is_file(), row["id"])
            self.assertEqual(digest(path), sealed["sha256"], row["id"])
            self.assertTrue(sealed["check"].strip(), row["id"])

    def test_the_console_rows_say_what_line_would_show_it(self) -> None:
        for row in self.plan():
            if row["evidenceSource"] == "console-transcript":
                self.assertTrue(row["markers"], row["id"])


class HardStopTests(unittest.TestCase):
    """What this image cannot show is refused, not estimated."""

    def stopped(self) -> list:
        return contract()["hardStop"]["conditions"]

    def test_the_stopped_conditions_are_the_unobservable_ones(self) -> None:
        plan = {row["id"]: row for row in contract()["evidencePlan"]}
        unobservable = {
            identifier
            for identifier, row in plan.items()
            if row["evidenceSource"] == "not-observable-with-this-image"
        }
        self.assertEqual({row["id"] for row in self.stopped()}, unobservable)

    def test_each_stopped_condition_says_what_is_missing_and_what_would_fix_it(
        self,
    ) -> None:
        for row in self.stopped():
            self.assertIn(row["id"], condition_ids())
            self.assertTrue(row["whatIsMissing"].strip(), row["id"])
            self.assertTrue(row["whatWouldMakeItObservable"].strip(), row["id"])

    def test_the_stop_refuses_the_run_rather_than_softening_the_condition(self) -> None:
        stop = contract()["hardStop"]
        self.assertEqual(stop["effect"], "refuse-before-any-machine-is-built")
        self.assertFalse(stop["conditionsWaived"])
        self.assertFalse(stop["conditionsReworded"])

    def test_the_runner_refuses_while_any_condition_is_unobservable(self) -> None:
        module = driver()
        stopped = [row["id"] for row in self.stopped()]
        self.assertEqual(sorted(module.hard_stop_conditions()), sorted(stopped))
        if not stopped:
            return
        with self.assertRaises(module.HardStop) as raised:
            module.assert_every_condition_is_observable()
        for identifier in stopped:
            self.assertIn(identifier, str(raised.exception))


class RunnerKnowsTheThirdAttemptTests(unittest.TestCase):
    """The blocker that would have refused the run at the first step."""

    def test_it_runs_as_the_third_attempt(self) -> None:
        module = driver()
        self.assertEqual(module.ATTEMPT, ATTEMPT)
        self.assertEqual(module.qualification()["attemptId"], ATTEMPT)

    def test_it_reads_the_sealed_criteria_rather_than_a_path_it_was_given(self) -> None:
        module = driver()
        self.assertEqual(module.QUALIFICATION_PATH, QUALIFICATION_PATH)

    def test_the_receipt_path_comes_from_the_record(self) -> None:
        module = driver()
        self.assertEqual(
            module.sealed_result_path(),
            REPO / qualification()["resultPath"],
        )
        self.assertFalse(module.sealed_result_path().exists())

    def test_it_refuses_once_a_receipt_for_this_attempt_exists(self) -> None:
        module = driver()
        with self.assertRaises(module.RefusedError):
            module.assert_no_run_has_been_sealed(CONTRACT_PATH)


class EveryConditionHasARuleTests(unittest.TestCase):
    """Twenty-one conditions, twenty-one rules, checked before the boot."""

    def test_the_runner_has_a_rule_for_every_sealed_condition(self) -> None:
        module = driver()
        self.assertEqual(set(module.RULES), condition_ids())

    def test_a_missing_rule_is_refused_and_not_discovered_afterwards(self) -> None:
        """The old runner raised on an unknown condition after the machine ran.

        That is the shape that spends the attempt and writes nothing: the boot
        happens, the judging loop meets a condition it does not know, and the
        run ends in a traceback with no verdict document.  Here the same
        absence is found while nothing has been started.
        """

        module = driver()
        rules = dict(module.RULES)
        rules.pop(sorted(condition_ids())[0])
        with self.assertRaises(module.RefusedError) as raised:
            module.assert_every_condition_has_a_rule(rules)
        self.assertIn(sorted(condition_ids())[0], str(raised.exception))

    def test_judging_answers_every_condition_and_nothing_else(self) -> None:
        module = driver()
        rows = module.judge_pass_conditions(module.empty_evidence())
        self.assertEqual({row["id"] for row in rows}, condition_ids())
        for row in rows:
            self.assertIn(row["verdict"], ("MET", "NOT MET"))
            self.assertTrue(row["evidence"].strip(), row["id"])

    def test_an_empty_run_does_not_pass(self) -> None:
        module = driver()
        rows = module.judge_pass_conditions(module.empty_evidence())
        self.assertFalse(module.overall_verdict(rows))


class OneUseMarkIsClaimedFirstTests(unittest.TestCase):
    """A crash mid-boot must cost the attempt, not hide it."""

    def setUp(self) -> None:
        self.module = driver()
        self.previous = self.module.LEDGER_DIR
        import tempfile

        self.temporary = tempfile.TemporaryDirectory()
        self.module.LEDGER_DIR = pathlib.Path(self.temporary.name) / "ledger"

    def tearDown(self) -> None:
        self.module.LEDGER_DIR = self.previous
        self.temporary.cleanup()

    def test_the_mark_lives_outside_the_working_directory(self) -> None:
        """A wiped scratch directory must not buy a second attempt."""

        self.assertNotIn("scratch", str(self.previous))
        self.assertNotIn("/tmp/", str(self.previous))
        self.assertTrue(self.module.ledger_path().name.startswith(ATTEMPT))

    def test_claiming_creates_the_mark_and_claiming_twice_refuses(self) -> None:
        path = self.module.claim_one_use({"approval": "test"})
        self.assertTrue(path.is_file())
        self.assertEqual(read(path)["attemptId"], ATTEMPT)
        with self.assertRaises(self.module.RefusedError):
            self.module.claim_one_use({"approval": "test"})

    def test_an_unclaimed_attempt_passes_the_check_and_a_claimed_one_does_not(
        self,
    ) -> None:
        self.module.assert_one_use_is_unclaimed()
        self.module.claim_one_use({"approval": "test"})
        with self.assertRaises(self.module.RefusedError):
            self.module.assert_one_use_is_unclaimed()

    def test_the_mark_is_written_before_the_machine_is_started(self) -> None:
        """The ordering the previous runner had backwards, proven by running it."""

        started = []

        def runner(argv):
            started.append(argv)
            self.assertTrue(
                self.module.ledger_path().is_file(),
                "the machine was started before the attempt was marked as used",
            )
            raise RuntimeError("stop here; no machine is actually built in a test")

        with self.assertRaises(RuntimeError):
            self.module.start_the_machine(
                host=pathlib.Path("/nonexistent/host"),
                kernel=pathlib.Path("/nonexistent/kernel"),
                root_disk=pathlib.Path("/nonexistent/root-disk"),
                console=pathlib.Path("/nonexistent/console"),
                receipt=pathlib.Path("/nonexistent/receipt"),
                approval={"approval": "test"},
                runner=runner,
            )
        self.assertEqual(len(started), 1)
        self.assertTrue(self.module.ledger_path().is_file())

    def test_the_free_preflight_starts_nothing_and_marks_nothing(self) -> None:
        """Item five: prove both counts are zero before anything is approved."""

        started = []
        report = self.module.preflight(runner=lambda argv: started.append(argv))
        self.assertEqual(started, [])
        self.assertEqual(report["machinesStarted"], 0)
        self.assertEqual(report["oneUseMarksCreated"], 0)
        self.assertFalse(self.module.ledger_path().exists())


class ArchiveIsRecheckedOnBothSidesTests(unittest.TestCase):
    """Three images and two manifests, before the boot and after it."""

    def setUp(self) -> None:
        self.module = driver()

    def test_the_targets_are_the_three_images_and_both_manifests(self) -> None:
        targets = self.module.archive_targets()
        names = [row["name"] for row in targets]
        for name in IMAGE_NAMES:
            self.assertIn(name, names)
        self.assertIn("preservation-manifest-at-the-archive", names)
        self.assertIn("preservation-record-in-the-repository", names)
        self.assertEqual(len(targets), 5)

    def test_every_target_carries_the_digest_the_sealed_records_hold(self) -> None:
        sealed = {row["name"]: row for row in qualification()["subject"]["images"]}
        preservation = qualification()["subject"]["preservationRecord"]["sha256"]
        for row in self.module.archive_targets():
            if row["name"] in sealed:
                self.assertEqual(row["sha256"], sealed[row["name"]]["sha256"], row["name"])
                self.assertEqual(row["bytes"], sealed[row["name"]]["bytes"], row["name"])
            else:
                self.assertEqual(row["sha256"], preservation, row["name"])

    def test_recomputing_reads_the_files_and_reports_what_it_found(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            targets = [
                {"name": "one", "relativePath": "a", "sha256": digest_of_bytes(b"a"), "bytes": 1},
            ]
            (root / "a").write_bytes(b"a")
            rows = self.module.recompute(targets, root)
            self.assertEqual(len(rows), 1)
            self.assertTrue(rows[0]["ok"])
            self.module.assert_archive_matches(rows)

    def test_a_changed_byte_stops_the_run(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            targets = [
                {"name": "one", "relativePath": "a", "sha256": digest_of_bytes(b"a"), "bytes": 1},
            ]
            (root / "a").write_bytes(b"b")
            rows = self.module.recompute(targets, root)
            self.assertFalse(rows[0]["ok"])
            with self.assertRaises(self.module.RefusedError) as raised:
                self.module.assert_archive_matches(rows)
            self.assertIn("one", str(raised.exception))

    def test_a_missing_file_stops_the_run(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as temporary:
            targets = [
                {"name": "one", "relativePath": "a", "sha256": digest_of_bytes(b"a"), "bytes": 1},
            ]
            rows = self.module.recompute(targets, pathlib.Path(temporary))
            self.assertFalse(rows[0]["ok"])

    def test_the_after_run_check_covers_every_target_not_just_the_root_disk(
        self,
    ) -> None:
        """The gap the previous runner had: one file re-hashed out of three."""

        before = [
            {"name": name, "sha256": "a" * 64, "bytes": 1, "ok": True}
            for name in ("guest-kernel", "guest-root-disk", "guest-initrd")
        ]
        after = [dict(row) for row in before]
        met, why = self.module.RULES["sealed-image-unchanged-after-the-run"](
            self.module.evidence_from(archive_before=before, archive_after=after)
        )
        self.assertTrue(met, why)
        after[0]["sha256"] = "b" * 64
        met, why = self.module.RULES["sealed-image-unchanged-after-the-run"](
            self.module.evidence_from(archive_before=before, archive_after=after)
        )
        self.assertFalse(met)
        self.assertIn("guest-kernel", why)

    def test_the_contract_says_both_sides_are_taken(self) -> None:
        row = contract()["recomputeOnBothSides"]
        self.assertTrue(row["beforeTheMachineIsConfigured"])
        self.assertTrue(row["afterTheMachineStops"])
        self.assertEqual(sorted(row["targets"]), sorted(IMAGE_NAMES) + [
            "preservation-manifest-at-the-archive",
            "preservation-record-in-the-repository",
        ])
        self.assertEqual(row["onMismatch"], "abort")


class JudgingRulesTests(unittest.TestCase):
    """The rules that decide a verdict, exercised without a Mac."""

    def setUp(self) -> None:
        self.module = driver()

    def rule(self, identifier, **evidence):
        return self.module.RULES[identifier](self.module.evidence_from(**evidence))

    def test_a_failed_unit_is_a_failure(self) -> None:
        met, _ = self.rule(
            "no-failed-unit-and-no-freeze-in-the-transcript",
            transcript="[  OK  ] Reached target multi-user.target\nreadiness reached\n",
            readiness_seen=True,
        )
        self.assertTrue(met)
        met, why = self.rule(
            "no-failed-unit-and-no-freeze-in-the-transcript",
            transcript="[FAILED] Failed to start boole-native-shadow-launcher-v2.service\n",
            readiness_seen=True,
        )
        self.assertFalse(met)
        self.assertIn("FAILED", why)

    def test_a_transcript_that_stops_short_is_a_stall(self) -> None:
        met, why = self.rule(
            "no-failed-unit-and-no-freeze-in-the-transcript",
            transcript="[  OK  ] Reached target multi-user.target\n",
            readiness_seen=False,
        )
        self.assertFalse(met)
        self.assertIn("stall", why.lower())

    def test_a_start_job_timeout_is_a_failure(self) -> None:
        met, why = self.rule(
            "no-failed-unit-and-no-freeze-in-the-transcript",
            transcript="Timed out waiting for device dev-vda.device\n",
            readiness_seen=True,
        )
        self.assertFalse(met)
        self.assertIn("Timed out", why)

    def test_emergency_mode_is_a_failure_and_the_target_is_compared(self) -> None:
        """The target compared is the guest's own default, not the unit's.

        The launcher unit is wanted by multi-user.target, and the second
        attempt's transcript passed through multi-user on its way to
        graphical.target, which is where the image actually settles.  Reading
        multi-user as the finishing line would accept a boot that stopped
        early.
        """

        met, _ = self.rule(
            "systemd-does-not-enter-emergency-and-reaches-its-target",
            transcript="[  OK  ] Reached target graphical.target - Graphical Interface.\n",
        )
        self.assertTrue(met)
        met, why = self.rule(
            "systemd-does-not-enter-emergency-and-reaches-its-target",
            transcript="[  OK  ] Reached target multi-user.target - Multi-User System.\n",
        )
        self.assertFalse(met)
        self.assertIn("graphical.target", why)
        met, why = self.rule(
            "systemd-does-not-enter-emergency-and-reaches-its-target",
            transcript="Starting emergency.service...\n",
        )
        self.assertFalse(met)
        self.assertIn("emergency", why)

    def test_the_launcher_unit_has_to_start_without_being_asked(self) -> None:
        met, _ = self.rule(
            "launcher-service-starts-without-being-asked",
            transcript="Starting boole-native-shadow-launcher-v2.service...\n",
        )
        self.assertTrue(met)
        met, _ = self.rule("launcher-service-starts-without-being-asked", transcript="")
        self.assertFalse(met)

    def test_reaching_for_a_network_or_a_node_is_a_failure(self) -> None:
        closed = {
            "machine": {
                "networkDevices": 0,
                "sharedDirectories": 0,
                "socketDevices": 0,
                "storageDevices": 1,
            },
            "nodeProcessStarted": False,
            "authorityGrantedToTheGuest": [],
        }
        met, _ = self.rule(
            "nothing-beyond-the-closed-local-boot-is-attempted",
            receipt=closed,
            transcript="",
        )
        self.assertTrue(met)
        reaching = dict(closed, nodeProcessStarted=True)
        met, why = self.rule(
            "nothing-beyond-the-closed-local-boot-is-attempted",
            receipt=reaching,
            transcript="",
        )
        self.assertFalse(met)
        self.assertIn("node", why.lower())

    def test_one_boot_is_counted_off_the_filesystem(self) -> None:
        met, _ = self.rule(
            "exactly-one-boot-of-this-image",
            ledger={"claimedByThisRun": True, "priorClaims": 0},
        )
        self.assertTrue(met)
        met, why = self.rule(
            "exactly-one-boot-of-this-image",
            ledger={"claimedByThisRun": True, "priorClaims": 1},
        )
        self.assertFalse(met)
        self.assertIn("1", why)

    def test_the_archive_recompute_condition_reads_both_manifests(self) -> None:
        rows = [
            {"name": name, "ok": True, "sha256": "a" * 64, "bytes": 1}
            for name in (
                "guest-initrd",
                "guest-kernel",
                "guest-root-disk",
                "preservation-manifest-at-the-archive",
                "preservation-record-in-the-repository",
            )
        ]
        met, _ = self.rule(
            "archive-digests-recomputed-immediately-before-boot", archive_before=rows
        )
        self.assertTrue(met)
        met, why = self.rule(
            "archive-digests-recomputed-immediately-before-boot",
            archive_before=rows[:3],
        )
        self.assertFalse(met)
        self.assertIn("manifest", why)

    def test_nothing_of_the_guest_may_survive_the_shutdown(self) -> None:
        met, _ = self.rule(
            "nothing-of-the-guest-survives-the-shutdown",
            host_after={"virtualMachines": 0, "hostProcesses": 0, "temporaryMounts": 0},
        )
        self.assertTrue(met)
        met, why = self.rule(
            "nothing-of-the-guest-survives-the-shutdown",
            host_after={"virtualMachines": 1, "hostProcesses": 0, "temporaryMounts": 0},
        )
        self.assertFalse(met)
        self.assertIn("virtualMachines", why)

    def test_an_unobservable_condition_never_reports_met(self) -> None:
        """A stop is not a pass, and silence is not evidence.

        These rules exist so the runner is complete, not so the run can be
        judged today.  If one of them ever answered MET, the hard stop would be
        the only thing standing between a silent transcript and a PASS.
        """

        for identifier in self.module.hard_stop_conditions():
            met, why = self.rule(identifier, transcript="anything at all\n")
            self.assertFalse(met, identifier)
            self.assertTrue(why.strip(), identifier)


class GateTests(unittest.TestCase):
    """Both new files are held by gates that run on every push."""

    def test_the_contract_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(CONTRACT_PATH.relative_to(REPO).as_posix(), smoke)

    def test_the_runner_is_pinned_by_the_docs_gate(self) -> None:
        smoke = (REPO / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn("native_shadow_mac3_closed_local_boot_arm64_v3.py", smoke)

    def test_this_module_stays_registered_in_the_self_test(self) -> None:
        self_test = (REPO / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, self_test)


def digest_of_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


if __name__ == "__main__":
    unittest.main()
