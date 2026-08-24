import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
ACTIVE = ROOT / "crates/boole-native-shadow-launcher/src/active_execution/mod.rs"


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


if __name__ == "__main__":
    unittest.main()
