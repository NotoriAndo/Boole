#!/usr/bin/env python3
"""Control one already-provisioned, closed-local development Lima VM.

No VM creation, downloads, grant/key generation, state reset, model execution,
submission, or MCP configuration mutation is implemented here.
"""
import argparse
import json
import re
import shutil
import socket
import subprocess
import time
import urllib.error
import urllib.request


STATUS_URL = "http://127.0.0.1:8082/native-shadow/status"
RESPONSE_LIMIT = 65536
# Fixed program and URL; no request-provided path, environment or source is run.
GUEST_STATUS_READER = """import sys, urllib.request, urllib.error
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs): return None
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
try:
    response = opener.open('http://127.0.0.1:8082/native-shadow/status', timeout=5)
except urllib.error.HTTPError as error:
    response = error
with response:
    data = response.read(65537)
if len(data) > 65536: raise ValueError('status response too large')
sys.stdout.buffer.write(data)
"""


class DevelopmentVmError(RuntimeError):
    pass


class EndpointUnavailable(DevelopmentVmError):
    pass


def host_port_busy():
    try:
        with socket.create_connection(("127.0.0.1", 8082), timeout=0.5):
            return True
    except ConnectionRefusedError:
        return False


def _strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate status field")
        result[key] = value
    return result


def _parse_status(body):
    if len(body.encode("utf-8") if isinstance(body, str) else body) > RESPONSE_LIMIT:
        raise DevelopmentVmError("status response too large")
    return json.loads(body, object_pairs_hook=_strict_object)


def read_host_status():
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        response = opener.open(STATUS_URL, timeout=5)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return _parse_status(response.read(RESPONSE_LIMIT + 1))


def validate_status(value):
    if not isinstance(value, dict) or value.get("schema") != "boole.development.verifier-status.v1":
        raise DevelopmentVmError("development discovery is unavailable; update the prepared runtime")
    for field in ("serviceReady", "submissionAllowed", "redeliveryRequiresOperatorAuthorization",
                  "loopbackOnly", "nonIssuable", "mineableNow", "activationAllowed"):
        if type(value.get(field)) is not bool:
            raise DevelopmentVmError("invalid verifier status field: " + field)
    if (not value["redeliveryRequiresOperatorAuthorization"] or not value["loopbackOnly"]
            or not value["nonIssuable"] or value["mineableNow"] or value["activationAllowed"]
            or any(type(value.get(field)) is not int or value[field] != 1
                   for field in ("maxCandidates", "maxCheckerExecutions"))):
        raise DevelopmentVmError("verifier status exceeds closed-local single-candidate scope")
    candidate = value.get("candidateBound")
    reserved = value.get("checkerExecutionReserved")
    if any(flag is not None and type(flag) is not bool for flag in (candidate, reserved)):
        raise DevelopmentVmError("invalid candidate status")
    ready = value["serviceReady"]
    allowed = ready and candidate is False
    state = "unavailable" if not ready else "unused" if allowed else "consumed"
    if (value["submissionAllowed"] != allowed or value.get("taskState") != state
            or (ready and (candidate is None or reserved is None))
            or (reserved and candidate is not True)):
        raise DevelopmentVmError("inconsistent verifier readiness and candidate status")
    identity = value.get("submissionIdentity")
    if (not isinstance(identity, dict) or len(identity) != 5
            or identity.get("schema") != "boole.native-shadow.submission.v1"
            or identity.get("familyVersion") != "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1"
            or type(identity.get("epoch")) is not int or identity["epoch"] < 4
            or any(not isinstance(identity.get(field), str)
                   or not re.fullmatch(r"[0-9a-f]{64}", identity[field])
                   for field in ("templateId", "challengeSha256"))):
        raise DevelopmentVmError("invalid public submission identity")
    return value


class DevelopmentVm:
    def __init__(self, vm, limactl, *, run=subprocess.run, fetch=read_host_status,
                 port_busy=host_port_busy, ready_timeout=60):
        if len(vm) > 63 or not re.fullmatch(r"boole-mcp-dev-[A-Za-z0-9][A-Za-z0-9_-]*", vm):
            raise DevelopmentVmError("select an explicit boole-mcp-dev-* VM name")
        self.vm = vm
        self.limactl = str(limactl)
        self.run = run
        self.fetch = fetch
        self.port_busy = port_busy
        self.ready_timeout = ready_timeout

    def _command(self, *args, timeout=30):
        result = self.run([self.limactl, *args], capture_output=True, text=True, timeout=timeout)
        if result.returncode:
            raise DevelopmentVmError("Lima command failed: {}".format(args[0]))
        return result.stdout

    def _machine(self):
        try:
            value = json.loads(self._command("list", self.vm, "--json"))
        except (ValueError, TypeError) as error:
            raise DevelopmentVmError("exact existing VM not found") from error
        if not isinstance(value, dict) or value.get("name") != self.vm:
            raise DevelopmentVmError("exact existing VM not found")
        return value

    @staticmethod
    def _safe_config(machine):
        config = machine.get("config", {})
        if not isinstance(config, dict):
            raise DevelopmentVmError("unsafe VM configuration")
        ssh = config.get("ssh", {})
        forwards = config.get("portForwards", [])
        if not isinstance(ssh, dict) or not isinstance(forwards, list):
            raise DevelopmentVmError("unsafe VM configuration")
        active = [rule for rule in forwards if isinstance(rule, dict) and rule.get("ignore") is not True]
        safe = (config.get("os") == "Linux" and config.get("plain") is True
                and config.get("mounts", []) == [] and config.get("env", {}) == {}
                and config.get("propagateProxyEnv") is False
                and ssh.get("forwardAgent") is False and ssh.get("loadDotSSHPubKeys") is False
                and not ssh.get("forwardX11") and not ssh.get("forwardX11Trusted")
                and len(active) == 1 and all(isinstance(rule, dict) for rule in forwards))
        if safe:
            rule = active[0]
            safe = (rule.get("hostIP") == rule.get("guestIP") == "127.0.0.1"
                    and rule.get("hostPort") == rule.get("guestPort") == 8082
                    and rule.get("hostPortRange", [8082, 8082]) == [8082, 8082]
                    and rule.get("guestPortRange", [8082, 8082]) == [8082, 8082]
                    and rule.get("proto", "any") in ("any", "tcp")
                    and not rule.get("hostSocket") and not rule.get("guestSocket"))
        if not safe:
            raise DevelopmentVmError("unsafe or unsupported VM configuration; use the prepared closed-local profile")

    def _snapshot(self, machine, action):
        result = {"schema": "boole.development.vm-control.v1", "ok": True,
                  "command": action, "vm": self.vm, "vmState": machine["status"],
                  "serviceReady": False, "submissionAllowed": False, "taskState": "unknown"}
        if machine["status"] == "Running":
            self._safe_config(machine)
            try:
                raw = self._command("shell", self.vm, "--", "/usr/bin/python3", "-c", GUEST_STATUS_READER)
            except (DevelopmentVmError, OSError, subprocess.TimeoutExpired) as error:
                raise EndpointUnavailable("guest verifier endpoint is unavailable") from error
            guest = validate_status(_parse_status(raw))
            try:
                host_value = self.fetch()
            except OSError as error:
                raise EndpointUnavailable("host verifier forwarding is unavailable") from error
            host = validate_status(host_value)
            if host != guest:
                raise DevelopmentVmError("host/guest verifier identity or status mismatch; do not submit")
            result.update({field: host[field] for field in ("serviceReady", "submissionAllowed", "taskState")})
            result["verifier"] = host
        return result

    def _start(self, machine):
        self._safe_config(machine)
        if machine["status"] == "Running":
            try:
                current = self._snapshot(machine, "start")
                if current["serviceReady"]:
                    return current
            except EndpointUnavailable:
                pass
        else:
            if self.port_busy():
                raise DevelopmentVmError("host port 8082 is already occupied; no VM was started")
            self._command("start", "--tty=false", self.vm, timeout=180)
            machine = self._machine()
            if machine.get("status") != "Running":
                raise DevelopmentVmError("VM did not enter Running state")
            self._safe_config(machine)
        self._command("shell", self.vm, "--", "sudo", "-n", "systemctl", "start", "boole-dev-network.service")
        self._command("shell", self.vm, "--", "sudo", "-n", "systemctl", "is-active", "--quiet", "boole-dev-network.service")
        self._command("shell", self.vm, "--", "sudo", "-n", "systemctl", "start",
                      "boole-native-shadow-launcher.service", "boole-native-shadow-replay-node.service", timeout=180)
        deadline = time.monotonic() + self.ready_timeout
        while True:
            try:
                result = self._snapshot(machine, "start")
                if result["serviceReady"]:
                    return result
            except (DevelopmentVmError, OSError, ValueError, subprocess.TimeoutExpired):
                pass
            if time.monotonic() >= deadline:
                raise DevelopmentVmError("readiness deadline exceeded; VM is retained, inspect status or stop it")
            time.sleep(0.25)

    def _stop(self, machine):
        warning = None
        if machine["status"] == "Running":
            try:
                self._command("shell", self.vm, "--", "sudo", "-n", "systemctl", "stop",
                              "boole-native-shadow-replay-node.service", "boole-native-shadow-launcher.service", timeout=180)
            except (DevelopmentVmError, OSError, subprocess.TimeoutExpired):
                warning = "guest service stop failed; used normal VM shutdown, never --force"
            self._command("stop", self.vm, timeout=180)
            machine = self._machine()
            if machine.get("status") != "Stopped":
                raise DevelopmentVmError("VM shutdown not confirmed; no force stop or deletion attempted")
        result = self._snapshot(machine, "stop")
        if warning:
            result["warning"] = warning
        return result

    def execute(self, action):
        if action not in ("start", "status", "stop"):
            raise DevelopmentVmError("unknown development VM command")
        machine = self._machine()
        if machine.get("status") not in ("Stopped", "Running"):
            raise DevelopmentVmError("VM state is ambiguous; no lifecycle action was taken")
        if action == "start":
            return self._start(machine)
        if action == "stop":
            return self._stop(machine)
        return self._snapshot(machine, action)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("start", "status", "stop"))
    parser.add_argument("--vm", required=True, help="exact existing boole-mcp-dev-* VM name")
    parser.add_argument("--limactl", default="limactl", help="Lima executable (default: PATH lookup)")
    args = parser.parse_args(argv)
    try:
        binary = shutil.which(args.limactl)
        if binary is None:
            raise DevelopmentVmError("limactl not found; provide --limactl with its executable path")
        result = DevelopmentVm(args.vm, binary).execute(args.action)
    except (DevelopmentVmError, OSError, ValueError, TypeError, subprocess.TimeoutExpired) as error:
        print(json.dumps({"schema": "boole.development.vm-control.v1", "ok": False,
                          "command": args.action, "vm": args.vm, "serviceReady": False,
                          "submissionAllowed": False, "taskState": "unknown",
                          "error": str(error)}, indent=2))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
