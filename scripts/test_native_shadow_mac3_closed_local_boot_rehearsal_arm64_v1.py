"""What the rehearsal has to prove, and what it must be unable to do.

Two halves.  The first is that the flow connects: the steps happen, they happen
in the order the real run needs them in, the mark is claimed before anything
starts, the host waits for readiness before asking for shutdown, and the
evidence that comes back is read by the same reader and judged by the same
rules a real run would use.

The second is the half that matters more.  A rehearsal that could accidentally
become a real run would be worse than no rehearsal, because it would spend the
one attempt while everyone believed it was free.  So the tests below try to make
it do exactly that -- claim the real mark, write the real sealed result, start
something without being handed a stand-in -- and require it to refuse.
"""

import hashlib
import json
import pathlib
import sys
import tempfile
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]

if str(REPO / "scripts") not in sys.path:
    sys.path.insert(0, str(REPO / "scripts"))

import native_shadow_mac3_closed_local_boot_arm64_v3 as runner  # noqa: E402
import native_shadow_mac3_closed_local_boot_rehearsal_arm64_v1 as rehearsal  # noqa: E402
import native_shadow_mac3_guest_evidence_protocol_arm64_v1 as protocol  # noqa: E402

SELF_TEST = REPO / "scripts/self-test.sh"
DOCS_SMOKE = REPO / "scripts/docs-smoke.sh"


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Scratch(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = pathlib.Path(
            self.enterContext(tempfile.TemporaryDirectory())
            if hasattr(self, "enterContext")
            else self._legacy_temporary()
        )

    def _legacy_temporary(self) -> str:
        holder = tempfile.TemporaryDirectory()
        self.addCleanup(holder.cleanup)
        return holder.name


class StepOrderTests(Scratch):
    def test_the_steps_run_in_the_order_a_real_run_would_need(self) -> None:
        host = rehearsal.FakeHost(self.directory)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertEqual(driven["steps"], list(rehearsal.STEPS))

    def test_the_mark_is_claimed_before_the_machine_starts(self) -> None:
        host = rehearsal.FakeHost(self.directory)
        driven = rehearsal.drive(self.directory, host=host)
        steps = driven["steps"]
        self.assertLess(
            steps.index("claim-the-one-use-mark"),
            steps.index("start-the-machine"),
            "a machine that starts before the mark is a spent attempt nothing records",
        )
        self.assertLess(
            host.order.index("claim_mark"),
            host.order.index("start"),
            "the collaborator saw them in the wrong order",
        )

    def test_the_archive_is_hashed_on_both_sides_of_the_run(self) -> None:
        host = rehearsal.FakeHost(self.directory)
        rehearsal.drive(self.directory, host=host)
        self.assertLess(host.order.index("hash_archive"), host.order.index("start"))
        self.assertLess(
            host.order.index("start"), len(host.order) - 1 - host.order[::-1].index(
                "hash_archive"
            ),
            "the archive was never hashed again after the run",
        )

    def test_shutdown_is_asked_for_only_after_readiness_was_seen(self) -> None:
        host = rehearsal.FakeHost(self.directory)
        driven = rehearsal.drive(self.directory, host=host)
        steps = driven["steps"]
        self.assertLess(
            steps.index("wait-for-readiness"), steps.index("ask-for-shutdown")
        )
        self.assertLess(
            host.order.index("console"), host.order.index("request_shutdown")
        )

    def test_the_host_confirms_the_machine_is_gone_rather_than_assuming(self) -> None:
        host = rehearsal.FakeHost(self.directory)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertIn("confirm-the-machine-is-gone", driven["steps"])
        self.assertIn("has_stopped", host.order)


class RefusalTests(Scratch):
    def test_a_rehearsal_claims_no_real_one_use_mark(self) -> None:
        before = runner.ledger_path().exists()
        host = rehearsal.FakeHost(self.directory)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertEqual(runner.ledger_path().exists(), before)
        self.assertEqual(driven["report"]["oneUseMarksCreated"], 0)

    def test_a_rehearsal_writes_no_sealed_result(self) -> None:
        before = runner.sealed_result_path().exists()
        rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        self.assertEqual(runner.sealed_result_path().exists(), before)

    def test_the_rehearsal_path_is_never_the_sealed_result_path(self) -> None:
        self.assertNotEqual(
            rehearsal.rehearsal_result_path(self.directory).resolve(),
            runner.sealed_result_path().resolve(),
        )

    def test_writing_the_sealed_result_path_is_refused(self) -> None:
        with self.assertRaises(rehearsal.RehearsalError):
            rehearsal.refuse_the_sealed_path(runner.sealed_result_path())

    def test_the_flow_cannot_run_without_being_handed_a_stand_in(self) -> None:
        with self.assertRaises(TypeError):
            rehearsal.drive(self.directory)

    def test_no_machine_is_started_and_the_report_says_so(self) -> None:
        driven = rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        self.assertEqual(driven["report"]["machinesStarted"], 0)
        self.assertTrue(driven["report"]["isARehearsal"])
        self.assertFalse(driven["report"]["bootAuthorisation"]["grantedByThisRecord"])

    def test_a_preflight_that_is_not_ready_stops_before_the_mark(self) -> None:
        host = rehearsal.FakeHost(self.directory, free_refusals_pass=False)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertEqual(driven["steps"], ["refuse-what-can-be-refused-for-free"])
        self.assertNotIn("claim_mark", host.order)
        self.assertNotIn("start", host.order)


class UnhappyGuestTests(Scratch):
    def test_a_guest_that_never_reaches_readiness_is_not_asked_to_shut_down(
        self,
    ) -> None:
        host = rehearsal.FakeHost(self.directory, reaches_readiness=False)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertNotIn("request_shutdown", host.order)
        self.assertIn("wait-for-readiness", driven["steps"])
        self.assertNotIn("ask-for-shutdown", driven["steps"])

    def test_a_guest_that_never_reaches_readiness_still_reaches_a_verdict(self) -> None:
        host = rehearsal.FakeHost(self.directory, reaches_readiness=False)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertIn("judge-the-conditions", driven["steps"])
        self.assertGreater(driven["report"]["notMetCount"], 0)

    def test_a_guest_that_ignores_the_shutdown_request_is_not_called_clean(self) -> None:
        host = rehearsal.FakeHost(self.directory, obeys_shutdown=False)
        driven = rehearsal.drive(self.directory, host=host)
        self.assertIn("request_shutdown", host.order)
        self.assertFalse(driven["report"]["machineStopped"])
        rows = {row["id"]: row for row in driven["report"]["conditions"]}
        self.assertEqual(
            rows["nothing-of-the-guest-survives-the-shutdown"]["verdict"], "NOT MET"
        )


class JudgementTests(Scratch):
    def test_the_same_rules_judge_the_rehearsal_as_would_judge_a_run(self) -> None:
        driven = rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        rows = driven["report"]["conditions"]
        self.assertEqual(
            sorted(row["id"] for row in rows), sorted(runner.condition_ids())
        )

    def test_a_healthy_rehearsal_still_cannot_reach_a_full_pass(self) -> None:
        driven = rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        rows = {row["id"]: row for row in driven["report"]["conditions"]}
        for stopped in runner.hard_stop_conditions():
            self.assertEqual(
                rows[stopped]["verdict"],
                "NOT MET",
                "%s reads MET in a rehearsal, which no image change earned" % stopped,
            )
        self.assertFalse(runner.overall_verdict(driven["report"]["conditions"]))

    def test_the_console_is_read_by_the_real_protocol_reader(self) -> None:
        driven = rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        read = driven["report"]["guestRecordsRead"]
        self.assertEqual(read["malformed"], [])
        self.assertIn("readiness", read["found"])

    def test_the_argument_list_is_the_one_a_real_run_would_build(self) -> None:
        host = rehearsal.FakeHost(self.directory)
        rehearsal.drive(self.directory, host=host)
        self.assertNotIn(
            "--dry-run",
            host.argv,
            "a rehearsal that only ever builds a dry-run argument list has never "
            "rehearsed the argument list a real run uses",
        )
        for flag in ("--kernel-sha256", "--root-disk-sha256", "--console", "--receipt"):
            self.assertIn(flag, host.argv)


class WrittenRecordTests(Scratch):
    def test_what_is_written_says_a_rehearsal_wrote_it(self) -> None:
        driven = rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        path = rehearsal.write_rehearsal(self.directory, driven["report"])
        self.assertIn("REHEARSAL", path.name)
        written = json.loads(path.read_text(encoding="utf-8"))
        self.assertTrue(written["isARehearsal"])
        self.assertEqual(written["overallVerdict"], "REHEARSAL ONLY -- NOT A RESULT")
        self.assertFalse(written["sealedResultWritten"])

    def test_the_written_record_names_which_steps_were_only_shaped(self) -> None:
        driven = rehearsal.drive(self.directory, host=rehearsal.FakeHost(self.directory))
        report = driven["report"]
        self.assertTrue(report["whatWasFaked"])
        for claim in report["whatWasFaked"]:
            self.assertTrue(claim.strip())


class RegistrationTests(unittest.TestCase):
    def test_the_rehearsal_tests_run_in_the_full_gate(self) -> None:
        text = SELF_TEST.read_text(encoding="utf-8")
        name = "scripts/test_native_shadow_mac3_closed_local_boot_rehearsal_arm64_v1.py"
        self.assertTrue(name in text, "%s is not registered in self-test.sh" % name)

    def test_the_docs_smoke_pins_that_no_machine_has_run(self) -> None:
        text = DOCS_SMOKE.read_text(encoding="utf-8")
        self.assertTrue(
            "BOOT-FLOW-REHEARSAL" in text,
            "docs-smoke.sh does not pin the rehearsal's standing",
        )


if __name__ == "__main__":
    unittest.main()
