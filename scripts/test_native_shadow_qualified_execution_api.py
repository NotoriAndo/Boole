import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
ACTIVE = ROOT / "crates/boole-native-shadow-launcher/src/active_execution/mod.rs"
MANAGER = ROOT / "crates/boole-native-shadow-launcher/tests/manager_cgroup_linux.rs"
GATE = ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
REPLAY_SERVICE = ROOT / "crates/boole-node/src/native_shadow_replay_service.rs"


class NativeShadowQualifiedExecutionApiTests(unittest.TestCase):
    def test_production_entrypoint_qualifies_then_executes_for_the_exact_peer_pid(self):
        source = ACTIVE.read_text(encoding="utf-8")

        self.assertIn("pub fn serve_qualified_three_fixed_unix_executions(", source)
        self.assertIn("qualified_peer.pid()", source)
        self.assertIn("Some(qualified_peer.pid())", source)
        self.assertIn("peer.pid != expected_node_pid", source)

    def test_uid_gid_only_three_execution_entrypoint_is_gate_only(self):
        source = ACTIVE.read_text(encoding="utf-8")
        legacy = source.index("pub fn serve_three_fixed_unix_executions(")
        feature_gate = source.rfind(
            '#[cfg(feature = "manager-cgroup-linux-gate")]', 0, legacy
        )

        self.assertGreater(feature_gate, source.rfind("\n}", 0, legacy))

    def test_linux_production_gate_qualifies_and_executes_in_one_node_process(self):
        manager = MANAGER.read_text(encoding="utf-8")
        gate = GATE.read_text(encoding="utf-8")
        replay_service = REPLAY_SERVICE.read_text(encoding="utf-8")

        self.assertIn("serve_qualified_three_fixed_unix_executions", manager)
        self.assertIn(
            'sudo -u boole-node python3 "$http_replay_gate_path"', gate
        )
        self.assertIn("[[ ${#peer_pids[@]} -eq 3 ]]", gate)
        self.assertIn('[[ "$peer_pid" == "$node_pid_before" ]]', gate)
        self.assertIn("production replay node process changed during the matrix", gate)
        self.assertIn("execution_launcher_pid_drift", replay_service)
        self.assertIn("execution_launcher_instance_drift", replay_service)


if __name__ == "__main__":
    unittest.main()
