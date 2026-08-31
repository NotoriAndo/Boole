#!/usr/bin/env python3
"""Retry only the reversible lane's classified snapshot-status failure.

The frozen acquisition programs remain unchanged.  This development harness
restarts one whole acquisition command after the exact infrastructure failure
they report for a non-200 snapshot response.  Their content-addressed store
keeps already verified objects, while every retried object is still checked by
the original size and SHA-256 rules.  Any other error is returned immediately.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from typing import Optional


MAX_ATTEMPTS = 3
RETRYABLE_MESSAGE = "snapshot response status is not 200"


def run(
    command: list[str],
    *,
    delay_seconds: float = 5.0,
    runner: object = subprocess.run,
    sleeper: object = time.sleep,
) -> int:
    if not command:
        raise ValueError("a command is required after --")
    if delay_seconds < 0 or delay_seconds > 30:
        raise ValueError("delay-seconds must be between 0 and 30")

    for attempt in range(1, MAX_ATTEMPTS + 1):
        completed = runner(command, capture_output=True, check=False)
        stdout = bytes(completed.stdout)
        stderr = bytes(completed.stderr)
        sys.stdout.buffer.write(stdout)
        sys.stdout.buffer.flush()
        sys.stderr.buffer.write(stderr)
        sys.stderr.buffer.flush()

        if completed.returncode == 0:
            return 0
        retryable = RETRYABLE_MESSAGE.encode("utf-8") in stderr
        if not retryable or attempt == MAX_ATTEMPTS:
            return int(completed.returncode)
        next_attempt = attempt + 1
        print(
            f"bounded snapshot retry {next_attempt}/{MAX_ATTEMPTS} "
            "after classified non-200 response",
            file=sys.stderr,
            flush=True,
        )
        sleeper(delay_seconds)
    raise AssertionError("bounded attempt loop did not return")


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--delay-seconds", type=float, default=5.0)
    value.add_argument("command", nargs=argparse.REMAINDER)
    return value


def main(argv: Optional[list[str]] = None) -> int:
    args = parser().parse_args(argv)
    command = list(args.command)
    if command and command[0] == "--":
        command.pop(0)
    try:
        return run(command, delay_seconds=args.delay_seconds)
    except ValueError as exc:
        print(f"bounded-snapshot-retry: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
