import importlib.util
import pathlib
import re
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


if __name__ == "__main__":
    unittest.main()
