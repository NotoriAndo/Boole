from __future__ import annotations

import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RESULT = (
    ROOT
    / "native/containment/native-shadow-installed-product-lifecycle-result-arm64-v1.json"
)


class InstalledProductLifecycleResultTests(unittest.TestCase):
    def test_real_mac_result_closes_install_run_health_and_stop_boundary(self) -> None:
        value = json.loads(RESULT.read_text(encoding="utf-8"))

        self.assertEqual(value["schema"], "boole.native-shadow.installed-mac-e2e.v1")
        self.assertEqual(value["status"], "INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS")
        self.assertEqual(
            value["sourceRevision"],
            "5b9dd421d2d7ae6834f35fdb3dbd8ddcd0d18bc6",
        )
        self.assertEqual(
            value["install"],
            {
                "command": "product.install-direct-boot",
                "guestReleaseSequence": 1,
                "releaseSequence": 1,
            },
        )
        self.assertEqual(value["transportLayout"], {"guestArtifacts": 11, "hostArtifacts": 4})

        health = value["health"]
        self.assertEqual(health["endpoint"], "http://127.0.0.1:8082")
        self.assertEqual(health["live"]["schema"], "boole.native-shadow.service-health.v1")
        self.assertEqual(health["live"]["probe"], "live")
        self.assertIs(health["live"]["live"], True)
        self.assertEqual(health["ready"]["schema"], "boole.native-shadow.service-health.v1")
        self.assertEqual(health["ready"]["probe"], "ready")
        self.assertIs(health["ready"]["ready"], True)
        self.assertEqual(health["ready"]["reason"], "ready")
        for probe in (health["live"], health["ready"]):
            self.assertIs(probe["loopbackOnly"], True)
            self.assertIs(probe["mineableNow"], False)
            self.assertIs(probe["activationAllowed"], False)

    def test_real_mac_result_keeps_the_frozen_verdict_matrix_and_boundaries(self) -> None:
        value = json.loads(RESULT.read_text(encoding="utf-8"))
        observed = [
            (row["caseId"], row["status"], row["outcome"], row["reasonCode"], row["passed"])
            for row in value["cases"]
        ]
        self.assertEqual(
            observed,
            [
                ("accepted", 200, "accepted", "accepted", True),
                ("tampered", 200, "deterministic_reject", "checker_rejected", True),
                ("constant", 200, "deterministic_reject", "checker_rejected", True),
                ("empty", 400, "precheck_reject", "intake_rejected", True),
            ],
        )
        self.assertIs(value["loopbackOnly"], True)
        for boundary in ("production", "testnet", "mining", "reward", "consensus", "p2p"):
            self.assertIs(value[boundary], False, boundary)
        self.assertIs(value["activationAllowed"], False)


if __name__ == "__main__":
    unittest.main()
