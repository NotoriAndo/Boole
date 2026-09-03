import json
import pathlib
import subprocess
import tempfile
import unittest


class _FakeRun:
    def __init__(self, owner_pk, envelope):
        self.owner_pk = owner_pk
        self.envelope = envelope
        self.calls = []

    def __call__(self, command, **kwargs):
        self.calls.append((command, kwargs))
        if command[1:3] == ["keys", "new"]:
            stdout = json.dumps({"key": {"pk": self.owner_pk}})
        else:
            stdout = json.dumps({"result": {"envelope": self.envelope}})
        return subprocess.CompletedProcess(command, 0, stdout=stdout, stderr="")


class TestTestnet2SessionSmoke(unittest.TestCase):
    def fixture(self):
        body = {"pk": "aa" * 32, "nonceS": "bb" * 32}
        return {
            "body": body,
            "sessionState": {
                "sessionPk": "aa" * 32,
                "ownerPk": "cc" * 32,
                "fixedRewardRecipient": "dd" * 32,
            },
            "submissionSession": {
                "submittedBy": "aa" * 32,
                "rewardRecipient": "dd" * 32,
                "nonce": "work-1",
                "signedWork": {
                    "network_id": "boole-testnet-2",
                    "payload": {"requestHash": "ee" * 32},
                },
            },
        }

    def test_registration_uses_fresh_network_scoped_cli_signature(self):
        from scripts.testnet2_session_smoke import build_registration_envelope

        signed = {
            "schema": "boole.signed.v1",
            "payload": {},
            "pk": "cc" * 32,
            "signature": "11" * 64,
            "network_id": "boole-testnet-2",
        }
        fake = _FakeRun("cc" * 32, signed)
        with tempfile.TemporaryDirectory() as tmp:
            got = build_registration_envelope(
                "/fake/boole-cli",
                pathlib.Path(tmp),
                "node-a",
                self.fixture(),
                now_secs=1_800_000_000,
                run=fake,
            )
            payload = json.loads(
                pathlib.Path(fake.calls[1][0][fake.calls[1][0].index("--payload") + 1]).read_text()
            )

        self.assertEqual(got, signed)
        self.assertEqual(payload["schema"], "boole.sessions.register.v1")
        self.assertEqual(payload["validBefore"], 1_800_000_300)
        self.assertEqual(payload["session"], self.fixture()["sessionState"])
        self.assertIn("--network-id", fake.calls[1][0])
        self.assertIn("boole-testnet-2", fake.calls[1][0])

    def test_submit_and_receipt_prove_the_session_branch_was_used(self):
        from scripts.testnet2_session_smoke import assert_session_receipt, authorized_submit

        fixture = self.fixture()
        submit = authorized_submit(fixture, 1234)
        self.assertEqual(submit["body"], fixture["body"])
        self.assertEqual(submit["session"], fixture["submissionSession"])
        self.assertEqual(submit["ts"], 1234)
        assert_session_receipt(
            {
                "receipt": {
                    "sessionPk": "aa" * 32,
                    "requestHash": "ee" * 32,
                }
            },
            fixture,
        )
        with self.assertRaisesRegex(RuntimeError, "session-bound receipt"):
            assert_session_receipt({"receipt": {}}, fixture)

    def test_all_testnet2_submit_smokes_use_the_session_helper(self):
        root = pathlib.Path(__file__).resolve().parent.parent
        scripts = [
            "testnet2-pinned-boot-smoke.sh",
            "testnet2-lean-invalid-injection-smoke.sh",
            "testnet2-checkpoint-resync-skip-smoke.sh",
            "testnet2-checkpoint-divergence-discard-smoke.sh",
        ]
        for name in scripts:
            text = (root / "scripts" / name).read_text()
            with self.subTest(script=name):
                self.assertIn("build_registration_envelope", text)
                self.assertIn("authorized_submit", text)
                self.assertIn("assert_session_receipt", text)


if __name__ == "__main__":
    unittest.main()
