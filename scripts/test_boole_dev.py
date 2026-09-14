"""Public lifecycle command behavior; the only fake is the external Lima CLI."""
import copy
import json
import subprocess
import unittest

from scripts import boole_dev as dev


VM = "boole-mcp-dev-test"


def machine(state="Stopped"):
    return {"name": VM, "status": state, "config": {
        "os": "Linux", "plain": True, "mounts": [], "propagateProxyEnv": False,
        "ssh": {"forwardAgent": False, "loadDotSSHPubKeys": False},
        "portForwards": [{"guestIP": "127.0.0.1", "hostIP": "127.0.0.1",
                          "guestPort": 8082, "hostPort": 8082}],
    }}


def verifier_status(consumed=False):
    return {"schema": "boole.development.verifier-status.v1", "serviceReady": True,
            "submissionAllowed": not consumed, "taskState": "consumed" if consumed else "unused",
            "candidateBound": consumed, "checkerExecutionReserved": consumed,
            "submissionIdentity": {"schema": "boole.native-shadow.submission.v1",
                "familyVersion": "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1",
                "templateId": "a" * 64, "challengeSha256": "b" * 64, "epoch": 15},
            "maxCandidates": 1, "maxCheckerExecutions": 1,
            "redeliveryRequiresOperatorAuthorization": True, "loopbackOnly": True,
            "nonIssuable": True, "mineableNow": False, "activationAllowed": False}


class LimaBoundary:
    def __init__(self, state="Stopped"):
        self.machine = machine(state)
        self.commands = []
        self.status = verifier_status()

    def __call__(self, command, **kwargs):
        self.commands.append(command)
        if command[1:] == ["list", VM, "--json"]:
            return subprocess.CompletedProcess(command, 0, json.dumps(self.machine), "")
        if command[1:] == ["shell", VM, "--", "/usr/bin/python3", "-c", dev.GUEST_STATUS_READER]:
            return subprocess.CompletedProcess(command, 0, json.dumps(self.status), "")
        if command[1:] == ["start", "--tty=false", VM]:
            self.machine["status"] = "Running"
            return subprocess.CompletedProcess(command, 0, "", "")
        if command[1:7] == ["shell", VM, "--", "sudo", "-n", "systemctl"] and command[7] in ("start", "is-active", "stop"):
            return subprocess.CompletedProcess(command, 0, "", "")
        if command[1:] == ["stop", VM]:
            self.machine["status"] = "Stopped"
            return subprocess.CompletedProcess(command, 0, "", "")
        raise AssertionError("unexpected external action: {}".format(command))


class DevelopmentVmTests(unittest.TestCase):
    def test_stopped_status_is_read_only_and_does_not_claim_unused_budget(self):
        lima = LimaBoundary()
        controller = dev.DevelopmentVm(VM, "/test/limactl", run=lima)
        result = controller.execute("status")
        self.assertEqual(result["vmState"], "Stopped")
        self.assertFalse(result["serviceReady"])
        self.assertFalse(result["submissionAllowed"])
        self.assertEqual(result["taskState"], "unknown")
        self.assertEqual(lima.commands, [["/test/limactl", "list", VM, "--json"]])

    def test_running_status_matches_selected_guest_and_keeps_consumed_task_unavailable(self):
        lima = LimaBoundary("Running")
        lima.status = verifier_status(consumed=True)
        fetched = []
        def fetch():
            fetched.append(True)
            return copy.deepcopy(lima.status)
        result = dev.DevelopmentVm(VM, "/test/limactl", run=lima, fetch=fetch).execute("status")
        self.assertTrue(result["serviceReady"])
        self.assertFalse(result["submissionAllowed"])
        self.assertEqual(result["taskState"], "consumed")
        self.assertEqual(result["verifier"]["submissionIdentity"], lima.status["submissionIdentity"])
        self.assertEqual(len(fetched), 1)
        self.assertEqual(len(lima.commands), 2)
        wrong = copy.deepcopy(lima.status)
        wrong["submissionIdentity"]["epoch"] += 1
        with self.assertRaisesRegex(dev.DevelopmentVmError, "host/guest"):
            dev.DevelopmentVm(VM, "/test/limactl", run=lima, fetch=lambda: wrong).execute("status")

    def test_start_refuses_unknown_or_host_shared_vm_before_any_mutation(self):
        for mutate in (
            lambda c: c.update(mounts=[{"location": "/Users/example"}]),
            lambda c: c["ssh"].update(forwardAgent=True),
            lambda c: c["ssh"].update(loadDotSSHPubKeys=True),
            lambda c: c.update(propagateProxyEnv=True),
            lambda c: c["portForwards"][0].update(hostIP="0.0.0.0"),
            lambda c: c["portForwards"][0].update(guestPort=8080),
            lambda c: c.update(plain=False),
            lambda c: c.update(env={"PRIVATE_TOKEN": "test-only"}),
        ):
            lima = LimaBoundary()
            mutate(lima.machine["config"])
            with self.assertRaises(dev.DevelopmentVmError):
                dev.DevelopmentVm(VM, "/test/limactl", run=lima).execute("start")
            self.assertEqual(len(lima.commands), 1)
        for name in ("default", "../vm", "--delete", "boole-mcp-dev-../../other"):
            with self.assertRaises(dev.DevelopmentVmError):
                dev.DevelopmentVm(name, "/test/limactl")

    def test_start_orders_network_before_services_and_never_reopens_a_spent_task(self):
        lima = LimaBoundary()
        lima.status = verifier_status(consumed=True)
        controller = dev.DevelopmentVm(VM, "/test/limactl", run=lima,
                                       fetch=lambda: copy.deepcopy(lima.status), port_busy=lambda: False)
        result = controller.execute("start")
        self.assertEqual(result["vmState"], "Running")
        self.assertTrue(result["serviceReady"])
        self.assertFalse(result["submissionAllowed"])
        self.assertEqual(result["taskState"], "consumed")
        changes = [command[1:] for command in lima.commands if command[1] == "start" or "systemctl" in command]
        self.assertEqual(changes, [
            ["start", "--tty=false", VM],
            ["shell", VM, "--", "sudo", "-n", "systemctl", "start", "boole-dev-network.service"],
            ["shell", VM, "--", "sudo", "-n", "systemctl", "is-active", "--quiet", "boole-dev-network.service"],
            ["shell", VM, "--", "sudo", "-n", "systemctl", "start",
             "boole-native-shadow-launcher.service", "boole-native-shadow-replay-node.service"],
        ])
        lima.commands.clear()
        self.assertEqual(controller.execute("start")["taskState"], "consumed")
        self.assertFalse(any("systemctl" in command or command[1] == "start" for command in lima.commands))

    def test_stop_is_graceful_and_idempotent_without_deleting_or_resetting_vm(self):
        lima = LimaBoundary("Running")
        lima.status = verifier_status(consumed=True)
        before = copy.deepcopy(lima.status)
        controller = dev.DevelopmentVm(VM, "/test/limactl", run=lima)
        result = controller.execute("stop")
        self.assertEqual(result["vmState"], "Stopped")
        self.assertFalse(result["submissionAllowed"])
        self.assertEqual(lima.status, before)
        self.assertEqual([command[1:] for command in lima.commands if command[1] != "list"], [
            ["shell", VM, "--", "sudo", "-n", "systemctl", "stop",
             "boole-native-shadow-replay-node.service", "boole-native-shadow-launcher.service"],
            ["stop", VM],
        ])
        lima.commands.clear()
        self.assertEqual(controller.execute("stop")["vmState"], "Stopped")
        self.assertEqual(lima.commands, [["/test/limactl", "list", VM, "--json"]])

    def test_start_can_resume_services_in_an_already_running_vm(self):
        class NotYetListening(LimaBoundary):
            def __init__(self):
                super().__init__("Running")
                self.first_read = True
            def __call__(self, command, **kwargs):
                if dev.GUEST_STATUS_READER in command and self.first_read:
                    self.first_read = False
                    self.commands.append(command)
                    return subprocess.CompletedProcess(command, 1, "", "connection refused")
                return super().__call__(command, **kwargs)
        lima = NotYetListening()
        controller = dev.DevelopmentVm(VM, "/test/limactl", run=lima, fetch=lambda: lima.status)
        self.assertTrue(controller.execute("start")["submissionAllowed"])
        self.assertFalse(any(command[1] == "start" for command in lima.commands))
        self.assertTrue(any("boole-dev-network.service" in command for command in lima.commands))

    def test_occupied_port_or_failed_network_gate_never_starts_verifier(self):
        lima = LimaBoundary()
        with self.assertRaises(dev.DevelopmentVmError):
            dev.DevelopmentVm(VM, "/test/limactl", run=lima, port_busy=lambda: True).execute("start")
        self.assertEqual(len(lima.commands), 1)
        class BadNetwork(LimaBoundary):
            def __call__(self, command, **kwargs):
                if "boole-dev-network.service" in command:
                    self.commands.append(command)
                    return subprocess.CompletedProcess(command, 1, "", "failed")
                return super().__call__(command, **kwargs)
        lima = BadNetwork()
        with self.assertRaises(dev.DevelopmentVmError):
            dev.DevelopmentVm(VM, "/test/limactl", run=lima, port_busy=lambda: False).execute("start")
        self.assertEqual(lima.machine["status"], "Running")
        self.assertFalse(any("boole-native-shadow-launcher.service" in command for command in lima.commands))
        self.assertFalse(any("stop" in command or "delete" in command for command in lima.commands))

    def test_unready_deadline_preserves_vm_and_never_resets_budget(self):
        lima = LimaBoundary()
        lima.status.update(serviceReady=False, submissionAllowed=False, taskState="unavailable")
        with self.assertRaisesRegex(dev.DevelopmentVmError, "deadline"):
            dev.DevelopmentVm(VM, "/test/limactl", run=lima, fetch=lambda: lima.status,
                              port_busy=lambda: False, ready_timeout=0).execute("start")
        self.assertEqual(lima.machine["status"], "Running")
        self.assertFalse(any("reset-failed" in command or "delete" in command or "stop" in command for command in lima.commands))

    def test_status_contract_rejects_scope_identity_and_ambiguous_json(self):
        for field, replacement in (("maxCheckerExecutions", 2), ("maxCandidates", True),
                                   ("mineableNow", True), ("activationAllowed", True),
                                   ("candidateBound", True), ("serviceReady", "true")):
            value = verifier_status()
            value[field] = replacement
            with self.assertRaises(dev.DevelopmentVmError):
                dev.validate_status(value)
        value = verifier_status()
        value["submissionIdentity"]["epoch"] = 15.0
        with self.assertRaises(dev.DevelopmentVmError):
            dev.validate_status(value)
        with self.assertRaises(ValueError):
            dev._parse_status('{"submissionAllowed":false,"submissionAllowed":true}')
        with self.assertRaises(dev.DevelopmentVmError):
            dev._parse_status("x" * 65537)


if __name__ == "__main__":
    unittest.main()
