#!/usr/bin/env python3
"""Classify a Git diff as process-only or full-validation work.

The classifier is deliberately fail-closed: every changed path must be on the
small process/documentation allowlist.  An empty, absolute, parent-traversing,
or otherwise unknown path selects the full CI lane.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import PurePosixPath


EXACT_PROCESS_PATHS = {
    ".github/workflows/ci.yml",
    ".github/workflows/verdict-corpus.yml",
    "AGENTS.md",
    "CLAUDE.md",
    "scripts/ci_change_scope.py",
    "scripts/docs-smoke.sh",
    "scripts/self-test.sh",
    "scripts/test_ci_change_scope.py",
    "scripts/test_ci_workflow_contract.py",
    "scripts/test_development_throughput_policy.py",
    "scripts/test_self_test_contract.py",
}


def _is_safe_relative_path(path: str) -> bool:
    if not path or "\\" in path or path.startswith("/"):
        return False
    parts = path.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        return False
    return not PurePosixPath(path).is_absolute()


def _is_process_path(path: str) -> bool:
    if not _is_safe_relative_path(path):
        return False
    if path in EXACT_PROCESS_PATHS:
        return True
    if path.startswith("docs/") or path.startswith("tasks/"):
        return True
    return "/" not in path and path.endswith(".md")


def classify(paths: list[str]) -> bool:
    return bool(paths) and all(_is_process_path(path) for path in paths)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--github-output")
    args = parser.parse_args()

    paths = [line.rstrip("\n") for line in sys.stdin]
    result = f"process_only={'true' if classify(paths) else 'false'}\n"
    sys.stdout.write(result)
    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as output:
            output.write(result)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
