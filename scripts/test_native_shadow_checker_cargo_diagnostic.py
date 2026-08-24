import importlib.util
import pathlib
import re
import tempfile
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name(
    "native_shadow_checker_cargo_diagnostic.py"
)


def load_module():
    spec = importlib.util.spec_from_file_location(
        "native_shadow_checker_cargo_diagnostic", MODULE_PATH
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class NativeShadowCheckerCargoDiagnosticTests(unittest.TestCase):
    def test_categories_are_fixed_and_do_not_copy_cargo_output(self):
        module = load_module()
        secret = b"/private/answer/submission.rs"
        output = b"error: Permission denied: " + secret

        category = module.classify_cargo_output(101, output)
        marker = module.build_diagnostic_marker(category=category)

        self.assertEqual(category, "permission_denied")
        self.assertNotIn(secret.decode("ascii"), marker)
        self.assertRegex(
            marker,
            re.compile(
                r"\Aboole-native-shadow-checker-cargo-diagnostic:v1;"
                r"category=[a-z_]+\Z"
            ),
        )

    def test_unknown_output_is_reduced_to_one_allowlisted_category(self):
        module = load_module()
        output = b"private source text and arbitrary environment content"

        self.assertEqual(module.classify_cargo_output(37, output), "unknown_nonzero")
        marker = module.build_diagnostic_marker(category="unknown_nonzero")

        self.assertNotIn(output.decode("ascii"), marker)
        self.assertLessEqual(len(marker), 96)

    def test_permission_failures_are_reduced_to_fixed_execution_stages(self):
        module = load_module()
        cases = {
            b"error: could not execute process /target/debug/deps/boole_native_shadow_task-123 (never executed)\nCaused by: Permission denied (os error 13)": "cargo_test_execute_denied",
            b"error: could not execute process /opt/toolchain/rustc -vV (never executed)\nCaused by: Permission denied (os error 13)": "cargo_rustc_execute_denied",
            b"error: linking with cc failed\nnote: Permission denied": "cargo_linker_permission_denied",
            b"error: couldn't create a temp dir: Permission denied": "cargo_temp_permission_denied",
            b"error: failed to create directory /target\nCaused by: Permission denied": "cargo_directory_permission_denied",
        }
        for output, expected in cases.items():
            with self.subTest(expected=expected):
                self.assertEqual(module.classify_cargo_output(101, output), expected)

    def test_fixed_probe_distinguishes_metadata_write_from_link_permission(self):
        module = load_module()
        limits = {"wallSeconds": 5}
        env = {"RUSTC": "/fixed/rustc"}

        with tempfile.TemporaryDirectory() as temporary:
            cwd = pathlib.Path(temporary)

            def metadata_denied(command, _cwd, _env, _limits):
                if "--version" in command:
                    return 0, b""
                if "--emit=metadata" in command:
                    return 1, b"error: couldn't create a temp dir: Permission denied"
                self.fail(f"unexpected fixed-probe command: {command!r}")

            self.assertEqual(
                module.run_fixed_rust_probe(
                    object(), metadata_denied, cwd, env, limits
                ),
                "rustc_metadata_permission_denied",
            )

            def alias_denied(command, _cwd, _env, _limits):
                if "--version" in command or "--emit=metadata" in command:
                    return 0, b""
                if command[0] == "/usr/bin/cc":
                    return 1, b"error: Permission denied"
                if command[0] == "/usr/bin/x86_64-linux-gnu-gcc-13":
                    return 0, b""
                return 1, b"error: linker cc: Permission denied"

            self.assertEqual(
                module.run_fixed_rust_probe(object(), alias_denied, cwd, env, limits),
                "cc_alias_permission_denied",
            )

            def default_rust_linker_denied(command, _cwd, _env, _limits):
                if "--version" in command or "--emit=metadata" in command:
                    return 0, b""
                if command[0] == "/usr/bin/cc":
                    return 0, b""
                if "linker=/usr/bin/x86_64-linux-gnu-gcc-13" in command:
                    return 0, b""
                return 1, b"error: linker cc: Permission denied"

            self.assertEqual(
                module.run_fixed_rust_probe(
                    object(), default_rust_linker_denied, cwd, env, limits
                ),
                "rustc_default_linker_permission_denied",
            )

            def assembler_denied(command, _cwd, _env, _limits):
                if "--version" in command or "--emit=metadata" in command:
                    return 0, b""
                if command[0] == "/usr/bin/cc":
                    return 1, b"error: Permission denied"
                if command[0] == "/usr/bin/x86_64-linux-gnu-gcc-13":
                    if "-S" in command:
                        return 0, b""
                    if "-c" in command:
                        return 1, b"error: Permission denied"
                return 1, b"error: linker cc: Permission denied"

            self.assertEqual(
                module.run_fixed_rust_probe(
                    object(), assembler_denied, cwd, env, limits
                ),
                "gcc_assembler_permission_denied",
            )

    def test_gate_compares_full_landlock_and_seccomp_categories(self):
        manager = MODULE_PATH.with_name("native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        modes = manager[
            manager.index("local -a diagnostic_modes=(") : manager.index(
                ")", manager.index("local -a diagnostic_modes=(")
            )
        ]

        self.assertIn("closed-local-replay-diagnostic-full", modes)
        self.assertIn("closed-local-replay-diagnostic-without-landlock", modes)
        self.assertIn("closed-local-replay-diagnostic-without-seccomp", modes)
        self.assertIn(
            "native-shadow categorical Cargo diagnostic:%s:%s", manager
        )


if __name__ == "__main__":
    unittest.main()
