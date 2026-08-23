#!/usr/bin/env python3
"""CI-only categorical observer for the fixed native checker Cargo run.

This wrapper never emits Cargo output, source, paths, arguments, environment,
counts, timings, or digests. It reports only one allowlisted category. The
production launcher never selects this script.
"""

from __future__ import annotations

import importlib.util
import pathlib
import subprocess
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
        "rustc_version_permission_denied",
        "rustc_version_failed",
        "rustc_metadata_permission_denied",
        "rustc_metadata_failed",
        "rustc_link_permission_denied",
        "rustc_linker_failed",
        "rustc_link_failed",
        "rustc_probe_permission_denied",
        "rustc_probe_linker_failed",
        "rustc_probe_failed",
        "workspace_execute_denied",
        "workspace_execute_failed",
        "cargo_test_execute_denied",
        "cargo_rustc_execute_denied",
        "cargo_linker_permission_denied",
        "cargo_temp_permission_denied",
        "cargo_directory_permission_denied",
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
        if (
            b"could not execute process" in normalized
            and b"boole_native_shadow_task" in normalized
        ):
            return "cargo_test_execute_denied"
        if b"could not execute process" in normalized and b"rustc" in normalized:
            return "cargo_rustc_execute_denied"
        if b"linking with" in normalized:
            return "cargo_linker_permission_denied"
        if b"couldn't create a temp dir" in normalized:
            return "cargo_temp_permission_denied"
        if b"failed to create directory" in normalized:
            return "cargo_directory_permission_denied"
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


def run_fixed_rust_probe(checker, original, cwd, env, limits) -> str | None:
    source = cwd / "boole-native-shadow-diagnostic.rs"
    metadata = cwd / "boole-native-shadow-diagnostic.rmeta"
    executable = cwd / "boole-native-shadow-diagnostic"
    source.write_text("fn main() {}\n", encoding="utf-8")

    code, output = original([env["RUSTC"], "--version"], cwd, env, limits)
    if code != 0:
        category = classify_cargo_output(code, output)
        if "permission_denied" in category:
            return "rustc_version_permission_denied"
        return "rustc_version_failed"

    code, output = original(
        [
            env["RUSTC"],
            "--crate-name",
            "boole_native_shadow_diagnostic",
            "--edition=2021",
            "--crate-type=lib",
            "--emit=metadata",
            str(source),
            "-o",
            str(metadata),
        ],
        cwd,
        env,
        limits,
    )
    if code != 0:
        category = classify_cargo_output(code, output)
        if "permission_denied" in category:
            return "rustc_metadata_permission_denied"
        return "rustc_metadata_failed"

    code, output = original(
        [
            env["RUSTC"],
            "--crate-name",
            "boole_native_shadow_diagnostic",
            "--edition=2021",
            str(source),
            "-o",
            str(executable),
        ],
        cwd,
        env,
        limits,
    )
    if code != 0:
        category = classify_cargo_output(code, output)
        if "permission_denied" in category:
            return "rustc_link_permission_denied"
        if category in {"linker_failed", "cargo_linker_permission_denied"}:
            return "rustc_linker_failed"
        return "rustc_link_failed"
    try:
        result = subprocess.run(
            [str(executable)],
            check=False,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=5,
            env=env,
            start_new_session=True,
        )
    except PermissionError:
        return "workspace_execute_denied"
    except (OSError, subprocess.SubprocessError):
        return "workspace_execute_failed"
    if result.returncode != 0:
        return "workspace_execute_failed"
    return None


def main() -> int:
    checker = load_checker()
    original = checker._run_contained

    def observed_run(command, cwd, env, limits):
        probe_category = run_fixed_rust_probe(checker, original, cwd, env, limits)
        if probe_category is not None:
            print(
                build_diagnostic_marker(category=probe_category),
                file=sys.stderr,
                flush=True,
            )
            raise checker.AuthorityUnavailable("contained_process_unavailable")
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
