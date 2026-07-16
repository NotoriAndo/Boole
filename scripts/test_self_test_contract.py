#!/usr/bin/env python3
"""Regression tests for P0.3: self-test contract requires RUST_TEST_THREADS=1
and --locked on every cargo invocation that resolves dependencies."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SELF_TEST = ROOT / "scripts" / "self-test.sh"
RUST_PARITY = ROOT / "scripts" / "check-rust-parity.sh"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class SelfTestContractTests(unittest.TestCase):
    def test_ci_workflow_declares_rust_test_threads(self) -> None:
        # P0.3 — the determinism contract must be explicit at the CI level too,
        # not only inside self-test.sh, so the workflow file alone documents the
        # single-threaded invariant. Matches `RUST_TEST_THREADS: "1"` (or '1')
        # under an `env:` block on the self-test job.
        body = _read(CI_WORKFLOW)
        pattern = re.compile(r"""^\s*RUST_TEST_THREADS:\s*["']?1["']?\s*$""", re.MULTILINE)
        self.assertRegex(
            body,
            pattern,
            ".github/workflows/ci.yml must declare `RUST_TEST_THREADS: \"1\"` "
            "at the workflow/job level so the single-threaded gate contract is "
            "explicit in CI, not only inside scripts/self-test.sh",
        )

    def test_self_test_exports_rust_test_threads(self) -> None:
        body = _read(SELF_TEST)
        pattern = re.compile(r"^\s*export\s+RUST_TEST_THREADS=1\b", re.MULTILINE)
        self.assertRegex(
            body,
            pattern,
            "scripts/self-test.sh must `export RUST_TEST_THREADS=1` so the "
            "single-threaded invariant is enforced regardless of caller env",
        )

    def test_self_test_uses_locked_on_cargo_test(self) -> None:
        body = _read(SELF_TEST)
        for line in body.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if re.search(r"\bcargo\s+test\b", stripped):
                self.assertIn(
                    "--locked",
                    stripped,
                    f"cargo test invocation in self-test.sh missing --locked: {stripped!r}",
                )

    def test_self_test_uses_locked_on_cargo_clippy(self) -> None:
        body = _read(SELF_TEST)
        for line in body.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if re.search(r"\bcargo\s+clippy\b", stripped):
                self.assertIn(
                    "--locked",
                    stripped,
                    f"cargo clippy invocation in self-test.sh missing --locked: {stripped!r}",
                )

    def test_rust_parity_uses_locked_on_cargo_test(self) -> None:
        body = _read(RUST_PARITY)
        for line in body.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if re.search(r"\bcargo\s+test\b", stripped):
                self.assertIn(
                    "--locked",
                    stripped,
                    f"cargo test invocation in check-rust-parity.sh missing --locked: {stripped!r}",
                )

    def test_self_test_has_lean_checker_build_step(self) -> None:
        # Fresh-environment regression: deep_verify_block_roundtrip re-runs the
        # Lean checker (`lake exec boole_check`) on a re-derived proof that
        # imports `Boole.Family.V0Helpers`. On a clean CI runner the checker's
        # `.lake/build` is empty (it is gitignored), so the import fails with
        # "unknown module prefix 'Boole'" and the share re-verifies as
        # accepted=false (DeepVerifyDivergence). The local gate only passes
        # because a developer's `.lake/build` is already warm. self-test.sh must
        # prebuild the checker artifacts so the local gate and a fresh CI runner
        # share the same precondition.
        body = _read(SELF_TEST)
        self.assertRegex(
            body,
            re.compile(r"^\s*run_logged\s+lean-checker-build\b", re.MULTILINE),
            "scripts/self-test.sh must declare a `lean-checker-build` stage that "
            "prebuilds the Lean checker artifacts before cargo-test re-runs them",
        )

    def test_lean_checker_build_builds_v0helpers_and_boole_check(self) -> None:
        body = _read(SELF_TEST)
        lake_build_lines = [
            line
            for line in body.splitlines()
            if "lake build" in line and not line.strip().startswith("#")
        ]
        self.assertTrue(
            any(
                "Boole.Family.V0Helpers" in line and "boole_check" in line
                for line in lake_build_lines
            ),
            "self-test.sh lean-checker-build must run "
            "`lake build Boole.Family.V0Helpers boole_check` so the proof's "
            "imported module olean and the checker exe both exist before "
            "deep_verify re-runs the checker on a fresh tree",
        )

    def test_self_test_runs_p2p_convergence_smoke(self) -> None:
        # N3.5 — the 3-peer local convergence smoke is the wave's closure
        # guard: three independently-run nodes with static peer lists must
        # reach the identical head with zero replay divergence. Constitution
        # §10: a guard protects nothing until the gate runs it.
        body = _read(SELF_TEST)
        self.assertRegex(
            body,
            re.compile(r"^\s*run_capture_json\s+p2p-convergence\b", re.MULTILINE),
            "scripts/self-test.sh must run the p2p-convergence stage "
            "(scripts/p2p-local-convergence-smoke.sh) so 3-peer convergence "
            "is gate-enforced, not just locally runnable",
        )
        self.assertIn(
            "scripts/p2p-local-convergence-smoke.sh",
            body,
            "the p2p-convergence stage must invoke "
            "scripts/p2p-local-convergence-smoke.sh",
        )

    def test_self_test_runs_verdict_corpus_stage(self) -> None:
        # SC.9c (ADR-0016 (a)/(a-1)) — the verdict corpus pins that the
        # three-state Lean verdict is a pure function of (proof bytes,
        # pinned checker, committed budget). The dedicated four-job
        # cross-platform gate lives in verdict-corpus.yml; this stage keeps
        # the corpus visible (and failing loudly, by name) inside the
        # single-command local gate as well.
        body = _read(SELF_TEST)
        self.assertRegex(
            body,
            re.compile(r"^\s*run_logged\s+verdict-corpus\b", re.MULTILINE),
            "scripts/self-test.sh must run the verdict-corpus stage",
        )
        self.assertIn(
            "--test verdict_corpus",
            body,
            "the verdict-corpus stage must run the boole-lean-runner "
            "verdict_corpus golden test",
        )

    def test_p2p_convergence_smoke_script_exists_and_asserts_convergence(self) -> None:
        smoke = ROOT / "scripts" / "p2p-local-convergence-smoke.sh"
        self.assertTrue(
            smoke.exists(),
            "scripts/p2p-local-convergence-smoke.sh must exist (N3.5)",
        )
        body = _read(smoke)
        for needle in ("replayMatchesRuntime", "--peer", "--p2p-listen"):
            self.assertIn(
                needle,
                body,
                f"p2p-local-convergence-smoke.sh must use {needle!r}: the smoke "
                "asserts identical heads AND zero replay divergence across "
                "3 statically-peered nodes",
            )

    def test_self_test_has_lean_toolchain_required_stage(self) -> None:
        # SC.10-iv-a — the required lane must FAIL, with a stage that names
        # the cause, when the Lean toolchain is absent. Without this explicit
        # probe the gate only fails *incidentally* (lean-checker-build's
        # `lake build` exits 127), and several lake-gated test suites
        # (e.g. verdict_corpus) self-skip green when lake/lean are missing —
        # so a future removal or reordering of the build stage would let the
        # required lane go green without ever executing Lean ("silent
        # skip-green", banned by the SC.10 gate condition).
        body = _read(SELF_TEST)
        self.assertRegex(
            body,
            re.compile(r"^\s*run_logged\s+lean-toolchain-required\b", re.MULTILINE),
            "scripts/self-test.sh must declare a `lean-toolchain-required` stage "
            "that fails the gate when the Lean toolchain is absent",
        )
        stage = re.search(
            r"^\s*run_logged\s+lean-toolchain-required\b.*?(?=^\s*run_logged\s|\Z)",
            body,
            re.MULTILINE | re.DOTALL,
        )
        assert stage is not None
        for probe in ("lake --version", "lean --version"):
            self.assertIn(
                probe,
                stage.group(0),
                f"the lean-toolchain-required stage must probe `{probe}` so a "
                "missing toolchain is a typed gate failure, not a silent skip",
            )

    def test_lean_toolchain_required_precedes_lean_checker_build(self) -> None:
        # The explicit probe must run before the first lake consumer so a
        # missing toolchain fails with the stage that names the real cause
        # (and before every lake-gated cargo test that would self-skip).
        body = _read(SELF_TEST)
        toolchain_idx = body.find("run_logged lean-toolchain-required")
        build_idx = body.find("run_logged lean-checker-build")
        self.assertNotEqual(
            toolchain_idx,
            -1,
            "self-test.sh is missing the lean-toolchain-required stage",
        )
        self.assertNotEqual(
            build_idx, -1, "self-test.sh is missing the lean-checker-build stage"
        )
        self.assertLess(
            toolchain_idx,
            build_idx,
            "lean-toolchain-required must run before lean-checker-build so a "
            "missing toolchain fails on the stage that names the cause",
        )

    def test_self_test_runs_testnet2_pinned_boot_smoke(self) -> None:
        # SC.10-iv-b — the required lane must boot a checker-pinned named
        # network for real (SC.9b pin + executable-toolchain gate, N5.2
        # genesis gate made passable by SC.10-iv-0) and prove the diverged-
        # genesis refusal still holds. Pinning the stage here keeps a future
        # self-test edit from silently dropping the only live pinned-boot
        # coverage in the gate.
        body = _read(SELF_TEST)
        self.assertRegex(
            body,
            re.compile(
                r"^\s*run_capture_json\s+testnet2-pinned-boot\b.*"
                r"testnet2-pinned-boot-smoke\.sh",
                re.MULTILINE,
            ),
            "scripts/self-test.sh must run scripts/testnet2-pinned-boot-smoke.sh "
            "via run_capture_json so the pinned-boot JSON verdict reaches the "
            "final aggregation",
        )
        smoke = ROOT / "scripts" / "testnet2-pinned-boot-smoke.sh"
        self.assertTrue(smoke.exists(), "pinned-boot smoke script must exist")
        smoke_body = _read(smoke)
        for marker in (
            "--network-id boole-testnet-2",
            "refusing to boot a diverged genesis",
            "bootRefusedOnDivergedGenesis",
            # SC.10-iv-b — the smoke must ALSO drive the committed
            # lean-bound share into the pinned node and prove real Lean
            # executed via the strict deep verify (no --allow-skips).
            "testnet2-lenbound-share.v1.json",
            "state verify --deep",
            "leanReverified",
        ):
            self.assertIn(
                marker,
                smoke_body,
                "the pinned-boot smoke must boot boole-testnet-2, pin the "
                "diverged-genesis refusal AND prove live Lean via strict deep "
                f"verify (missing marker: {marker!r})",
            )
        self.assertNotIn(
            "--allow-skips",
            smoke_body,
            "the pinned-boot smoke's deep verify must stay strict — "
            "--allow-skips would let a skip-green run pass",
        )

    def test_self_test_runs_testnet2_lean_invalid_injection_smoke(self) -> None:
        # SC.10-iv-c — the SC.10 completion gate (L1 master §SC.10, promoted
        # recommended→MANDATORY by the 2026-07-12 third review): a
        # structurally-valid but Lean-invalid share/block injected into a
        # checker-pinned multi-node network must be adopted by NO node. The
        # positive lane (iv-b) proves real Lean runs; this stage is the
        # negative differential — without it, the required lane never
        # observes a live rejection, so a regression that fail-opens the
        # consensus re-verification gate would stay green.
        body = _read(SELF_TEST)
        self.assertRegex(
            body,
            re.compile(
                r"^\s*run_capture_json\s+testnet2-lean-invalid-injection\b.*"
                r"testnet2-lean-invalid-injection-smoke\.sh",
                re.MULTILINE,
            ),
            "scripts/self-test.sh must run "
            "scripts/testnet2-lean-invalid-injection-smoke.sh via "
            "run_capture_json so the injection verdict reaches the final "
            "aggregation (SC.10 mandatory gate)",
        )
        smoke = ROOT / "scripts" / "testnet2-lean-invalid-injection-smoke.sh"
        self.assertTrue(
            smoke.exists(),
            "scripts/testnet2-lean-invalid-injection-smoke.sh must exist "
            "(SC.10-iv-c mandatory gate)",
        )
        smoke_body = _read(smoke)
        for marker in (
            # the injection runs on the checker-pinned named network — the
            # only lane where consensus Lean re-verification is active
            "--network-id boole-testnet-2",
            # a faulty producer boots the SAME named network checker-off, so
            # its own admission gate is inactive and it will assemble and
            # gossip a block carrying the proof-invalid share
            "--lean-checker-disabled",
            # committed Lean-invalid fixture (generator-pinned, like the
            # honest iv-b share fixture)
            "testnet2-lean-invalid.v1.json",
            # honest differential control: the same lane must still accept
            # the committed lean-bound share and converge
            "testnet2-lenbound-share.v1.json",
            # the mandatory assertion: the injected block is adopted by
            # zero honest nodes
            "invalidBlockAdoptedBy",
            # rejection must be OBSERVED live (counter moved), not inferred
            # from a block that silently never arrived
            "boole_p2p_ingress_blocks_rejected_total",
        ):
            self.assertIn(
                marker,
                smoke_body,
                "the lean-invalid injection smoke must inject on the pinned "
                "network via a checker-off producer, use the committed "
                "fixtures and assert observed zero adoption (missing marker: "
                f"{marker!r})",
            )

    def test_self_test_aggregation_gates_lean_invalid_injection(self) -> None:
        # The aggregation must HARD-GATE the injection verdict: zero honest
        # nodes adopted the invalid block, every honest node OBSERVABLY
        # rejected it at ingest re-verification, and the honest differential
        # control still converged. A smoke that merely runs (ok=true)
        # without these fields must fail the aggregate — otherwise the
        # mandatory assertion can rot silently.
        body = _read(SELF_TEST)
        for needle in (
            '"name": "testnet2-lean-invalid-injection"',
            'get("invalidBlockAdoptedBy") == 0',
            'get("invalidBlockRejectedByIngest") is True',
            'get("honestConvergedHeight") == 1',
        ):
            self.assertIn(
                needle,
                body,
                "self-test.sh final aggregation must gate the lean-invalid "
                f"injection fields (missing: {needle!r})",
            )

    def test_lean_checker_build_precedes_cargo_test(self) -> None:
        body = _read(SELF_TEST)
        lean_idx = body.find("run_logged lean-checker-build")
        # The cargo-test stage proper, not cargo-test-build / cargo-test-prewarm
        # (the trailing `cargo test` token disambiguates).
        cargo_test_match = re.search(
            r"^\s*run_logged\s+cargo-test\s+cargo\s+test\b", body, re.MULTILINE
        )
        self.assertNotEqual(
            lean_idx, -1, "self-test.sh is missing the lean-checker-build stage"
        )
        self.assertIsNotNone(
            cargo_test_match, "self-test.sh cargo-test stage not found"
        )
        self.assertLess(
            lean_idx,
            cargo_test_match.start(),
            "lean-checker-build must run before the cargo-test stage so the Lean "
            ".olean artifacts exist when deep_verify re-runs the checker",
        )


if __name__ == "__main__":
    unittest.main()
