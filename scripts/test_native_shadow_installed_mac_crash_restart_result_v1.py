from __future__ import annotations

import hashlib
import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = (
    ROOT
    / "native/containment/native-shadow-installed-mac-crash-restart-result-arm64-v1.json"
)


class InstalledMacCrashRestartResultTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.value = json.loads(RESULT.read_text(encoding="utf-8"))

    def test_terminal_results_redeliver_without_guest_execution(self) -> None:
        terminal = self.value["scenarios"][0]
        self.assertEqual(terminal["checkerExecutionsBeforeCrash"], 2)
        self.assertEqual(terminal["checkerExecutionsAfterRestart"], 0)
        self.assertEqual(terminal["journalRows"], 10)
        self.assertTrue(terminal["acceptedRedeliveryByteIdentical"])
        self.assertTrue(terminal["tamperedRedeliveryFlagOnlyDelta"])
        self.assertTrue(terminal["journalBytesUnchangedAfterRestart"])
        self.assertTrue(terminal["runtimeClean"])

    def test_unresolved_inflight_fails_closed_without_checker_execution(self) -> None:
        unresolved = self.value["scenarios"][1]
        self.assertEqual(unresolved["checkerExecutions"], 0)
        self.assertEqual(unresolved["journalRows"], 3)
        self.assertTrue(unresolved["restartRefused"])
        self.assertTrue(unresolved["listenerRefused"])
        self.assertTrue(unresolved["failClosedMessageObserved"])
        self.assertTrue(unresolved["runtimeClean"])

    def test_result_reuses_the_byte_identical_guest_and_keeps_all_authority_closed(self) -> None:
        self.assertEqual(self.value["guestImage"]["newImageBuilds"], 0)
        self.assertEqual(self.value["harnessAccounting"]["productRuns"], 1)
        self.assertEqual(self.value["harnessAccounting"]["harnessRetries"], 1)
        self.assertEqual(set(self.value["boundary"].values()), {False})

    def test_prior_happy_path_record_is_bound_by_digest(self) -> None:
        binding = self.value["evidence"]["installedHappyPathRecord"]
        path = ROOT / binding["path"]
        self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), binding["sha256"])


if __name__ == "__main__":
    unittest.main()
