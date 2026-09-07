#!/usr/bin/env python3
"""Regression tests for P1.1b: the state-dir contract is wired into the
boole-node runtime.

L7 contract: when a `boole-node run-local --state-dir <PATH>` invocation
boots, the runtime must:

1. expose `state_dir: Option<PathBuf>` (and a companion `network_id`) on
   `LocalNodeConfig` so the CLI flag can plumb through,
2. acquire the state-dir advisory lock via `acquire_state_dir` (or the
   re-exported `acquire`) BEFORE opening any per-store path so a second
   process is rejected before it can touch a ledger,
3. write/verify `state.manifest.json` via `ensure_manifest`,
4. carry the resulting `StateDirGuard` for the process lifetime (a field
   on `LocalNodeState`), and
5. accept a `--state-dir <PATH>` flag on the `run-local` subcommand.

The Rust integration test (`state_dir_lock_blocks_second_node.rs`)
covers the dynamic behavior; this contract test pins the static surface
so removing any of the above wiring is caught by a python-script-tests
run instead of waiting for a slow cargo test.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL_NODE = ROOT / "crates" / "boole-node" / "src" / "local_node.rs"
MAIN = ROOT / "crates" / "boole-node" / "src" / "main.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class StateDirRuntimeContractTests(unittest.TestCase):
    def test_local_node_config_carries_state_dir_field(self) -> None:
        body = _read(LOCAL_NODE)
        self.assertRegex(
            body,
            re.compile(
                r"pub\s+state_dir\s*:\s*Option\s*<\s*PathBuf\s*>",
                re.MULTILINE,
            ),
            "P1.1b: LocalNodeConfig must carry "
            "`pub state_dir: Option<PathBuf>` so the CLI flag can plumb "
            "the L7 state directory through to boot.",
        )

    def test_local_node_config_carries_network_id_field(self) -> None:
        body = _read(LOCAL_NODE)
        self.assertRegex(
            body,
            re.compile(
                r"pub\s+network_id\s*:\s*(?:Option\s*<\s*String\s*>|String)",
                re.MULTILINE,
            ),
            "P1.1b: LocalNodeConfig must carry `pub network_id` (String or "
            "Option<String>) so the manifest can pin which network this "
            "directory was created against.",
        )

    def test_local_node_state_holds_state_dir_guard(self) -> None:
        body = _read(LOCAL_NODE)
        self.assertRegex(
            body,
            re.compile(
                r"state_dir_guard\s*:\s*Option\s*<\s*StateDirGuard\s*>",
                re.MULTILINE,
            ),
            "P1.1b: LocalNodeState must hold "
            "`state_dir_guard: Option<StateDirGuard>` so the flock "
            "survives for the process lifetime.",
        )

    def test_from_config_calls_acquire(self) -> None:
        function = re.search(
            r"(?ms)^    fn from_config\([^)]*\)[^{]*\{(.*?)^    \}",
            _read(LOCAL_NODE),
        )
        self.assertIsNotNone(function, "LocalNodeState::from_config must exist")
        body = re.sub(r"(?m)//[^\n]*", "", function.group(1))
        acquisition = re.search(r"\blet\s+state_dir_guard\b[^;]*;", body)
        self.assertIsNotNone(acquisition, "from_config must acquire its state-dir guard")
        expression = acquisition.group()
        # Option::map passes the same locking function as a value; requiring
        # an opening parenthesis after its name rejects this valid call path.
        self.assertRegex(
            expression,
            r"\.map\s*\(\s*state_dir::acquire\s*\)\s*\.transpose\s*\(\s*\)\s*\?",
            "optional state-dir acquisition must propagate a lock failure",
        )
        consumers = list(re.finditer(
            r"\b(?:LedgerLockSet::acquire|state_dir::ensure_manifest|"
            r"File\w+::(?:recover|rebuild\w*)|"
            r"RuntimeAdmissionState::boot_from_store\w*)\s*\(",
            body,
        ))
        self.assertTrue(consumers, "from_config must retain durable-store consumers")
        for consumer in consumers:
            self.assertLess(
                acquisition.end(), consumer.start(),
                f"state-dir lock must precede {consumer.group()}",
            )
        returned_state = re.search(r"(?s)\bOk\s*\(\s*Self\s*\{(.*)\}\s*\)", body)
        self.assertIsNotNone(returned_state, "from_config must return its node state")
        self.assertTrue(
            re.search(r"\b_state_dir_guard\s*:\s*state_dir_guard\b", returned_state.group(1)),
            "the acquired guard must move into the returned node state",
        )

    def test_from_config_calls_ensure_manifest(self) -> None:
        body = _read(LOCAL_NODE)
        self.assertRegex(
            body,
            re.compile(r"\bensure_manifest\s*\(", re.MULTILINE),
            "P1.1b: LocalNodeState::from_config must call "
            "`ensure_manifest` so first boot writes the manifest and "
            "later boots reject a directory built for a different "
            "network.",
        )

    def test_run_local_cli_carries_state_dir_flag(self) -> None:
        body = _read(MAIN)
        # The clap derive attribute pair: a `state_dir` Rust field and a
        # `long = "state-dir"` arg attribute. Both must be present.
        self.assertRegex(
            body,
            re.compile(
                r"long\s*=\s*\"state-dir\"",
                re.MULTILINE,
            ),
            "P1.1b: `boole-node run-local` clap args must expose a "
            "`--state-dir <PATH>` flag.",
        )
        self.assertRegex(
            body,
            re.compile(
                r"\bstate_dir\s*:\s*Option\s*<\s*PathBuf\s*>",
                re.MULTILINE,
            ),
            "P1.1b: `RunLocalArgs` must carry "
            "`state_dir: Option<PathBuf>`.",
        )


if __name__ == "__main__":
    unittest.main()
