import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = (
    ROOT
    / "native/containment/native-shadow-installed-mac-e2e-result-arm64-v1.json"
)


class InstalledMacE2EResultTests(unittest.TestCase):
    def test_result_binds_the_green_images_and_exact_four_case_route(self) -> None:
        value = json.loads(RESULT.read_text(encoding="utf-8"))

        self.assertEqual(value["schema"], "boole.native-shadow.installed-mac-e2e-result.v1")
        self.assertEqual(value["status"], "INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS")
        self.assertEqual(value["imageBuild"]["runId"], 33_652_402_930)
        self.assertEqual(value["imageBuild"]["headSha"], "6fc29d20c15072bd02337e6cb7af865cff8c59b7")
        self.assertEqual(value["imageBuild"]["comparisonStatus"], "TWO-REPLICAS-BYTE-IDENTICAL")
        self.assertEqual(
            [row["id"] for row in value["imageBuild"]["artifacts"]],
            [9_855_544_756, 9_855_772_878, 9_855_915_101],
        )
        self.assertEqual(
            [row["conclusion"] for row in value["imageBuild"]["jobs"]],
            ["success", "success", "success"],
        )
        self.assertEqual(
            [(row["name"], row["sizeBytes"], row["sha256"]) for row in value["imageBuild"]["outputs"]],
            [
                ("guest-kernel", 57_860_488, "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"),
                ("guest-initrd", 1_778_359_384, "1d50697dd890512815a3cd008f3b3d8a7330464e21cdce6aec9d93962b1a09c7"),
                ("guest-root-disk", 2_037_846_016, "3693d5bbaa85b6c5106aa8f89c3d9f6c523aa31528f312c07bca661f2a360150"),
            ],
        )

        route = value["installedRoute"]
        self.assertEqual(route["sourceRevision"], "7c8870b55e0c5fc156d69a428bd2ea44c22b89c7")
        self.assertEqual(route["resultSha256"], "c4a815ebf4ff415f536024c2335f08cecb7d76c969242e476767395a1ebd2310")
        self.assertEqual(
            [(row["caseId"], row["httpStatus"], row["outcome"], row["reasonCode"]) for row in route["cases"]],
            [
                ("accepted", 200, "accepted", "accepted"),
                ("tampered", 200, "deterministic_reject", "checker_rejected"),
                ("constant", 200, "deterministic_reject", "checker_rejected"),
                ("empty", 400, "precheck_reject", "intake_rejected"),
            ],
        )
        self.assertTrue(all(row["passed"] for row in route["cases"]))
        self.assertEqual(route["transportLayout"], {"hostArtifacts": 4, "guestArtifacts": 11})
        self.assertTrue(route["cleanShutdown"])
        self.assertTrue(route["runtimeCleaned"])

        attempts = value["executionAccounting"]
        self.assertEqual(attempts["imageBuildDispatches"], 1)
        self.assertEqual(attempts["macHarnessRuns"], 3)
        self.assertEqual(attempts["harnessRetries"], 2)
        self.assertEqual([row["classification"] for row in attempts["runs"]], [
            "HARNESS-CONTRACT-DEFECT",
            "TOOLING-SHUTDOWN-CONTRACT-DEFECT",
            "PASS",
        ])

        boundary = value["boundary"]
        for key in (
            "production",
            "testnet",
            "mining",
            "reward",
            "consensus",
            "p2p",
            "activationAllowed",
            "paidApiBenchmark",
            "leaderboardClaim",
        ):
            self.assertFalse(boundary[key], key)


if __name__ == "__main__":
    unittest.main()
