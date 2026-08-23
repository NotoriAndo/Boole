#!/usr/bin/env python3
"""CI-only categorical observer for the fixed native checker Cargo run.

This wrapper never emits Cargo output, source, paths, arguments, environment,
counts, timings, or digests. It reports only one allowlisted category. The
production launcher never selects this script.
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys


CHECKER_PATH = pathlib.Path(
    "/usr/share/boole/native-shadow/checkers/"
    "rust-tuple-struct-project-v1/checker.py"
)
PREFIX = "boole-native-shadow-checker-cargo-diagnostic:v1"
ALLOWED_CATEGORIES = frozenset(
    {
        "success",
        "wall_limit",
        "output_limit",
        "authority_unavailable",
        "permission_denied",
        "read_only_filesystem",
        "missing_file",
        "cargo_lock_wait",
        "process_spawn_failed",
        "linker_failed",
        "temporary_directory_failed",
        "hidden_test_failed",
        "compiler_error",
        "unknown_nonzero",
    }
)


def classify_cargo_output(code: int, output: bytes) -> str:
    if code == 0:
        return "success"
    normalized = b" ".join(output.lower().split())
    if b"permission denied" in normalized:
        return "permission_denied"
    if b"read-only file system" in normalized:
        return "read_only_filesystem"
    if b"no such file or directory" in normalized:
        return "missing_file"
    if b"blocking waiting for file lock" in normalized:
        return "cargo_lock_wait"
    if b"could not execute process" in normalized or b"failed to spawn" in normalized:
        return "process_spawn_failed"
    if b"linking with" in normalized and b"failed" in normalized:
        return "linker_failed"
    if b"couldn't create a temp dir" in normalized:
        return "temporary_directory_failed"
    if b"test result: failed" in normalized or b"test failed" in normalized:
        return "hidden_test_failed"
    if b"error[" in normalized or b"error:" in normalized:
        return "compiler_error"
    return "unknown_nonzero"


def build_diagnostic_marker(*, category: str) -> str:
    if category not in ALLOWED_CATEGORIES:
        raise ValueError("diagnostic category is not allowlisted")
    return f"{PREFIX};category={category}"


def load_checker():
    spec = importlib.util.spec_from_file_location("boole_fixed_native_checker", CHECKER_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("fixed checker import specification is unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def main() -> int:
    checker = load_checker()
    original = checker._run_contained

    def observed_run(command, cwd, env, limits):
        try:
            code, output = original(command, cwd, env, limits)
        except checker.AuthorityUnavailable as error:
            category = {
                "resource_wall_limit": "wall_limit",
                "resource_output_limit": "output_limit",
            }.get(error.reason_code, "authority_unavailable")
            marker = build_diagnostic_marker(category=category)
            print(marker, file=sys.stderr, flush=True)
            raise
        marker = build_diagnostic_marker(category=classify_cargo_output(code, output))
        print(marker, file=sys.stderr, flush=True)
        return code, output

    checker._run_contained = observed_run
    return checker.main()


if __name__ == "__main__":
    raise SystemExit(main())
