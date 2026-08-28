#!/usr/bin/env python3
"""Gate the second step of the serving-gap successor chain: the lock generator.

The first step named the files. This step is the tool that knows how to build a
successor lock out of them and refuse one that is wrong. It deliberately seals
nothing: the tests below required the successor lock and its result to be absent,
because writing them is the third step. When that step runs, the right move is a
step-three gate that supersedes ``ChainPositionTests`` with the sealed digests --
not a quiet relaxation here.

Superseded on 2026-08-28 by the third step, which sealed both documents. The three
tests in ``ChainPositionTests`` that asserted absence now assert the sealed state
and defer the digests to
``scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py``, which is
where the byte facts live. Everything else in this file is unchanged: the tool it
gates was run by the third step, not edited.
"""
from __future__ import annotations

import copy
import json
import pathlib
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_rootfs_source_lock_arm64_v1 as predecessor_tool
from scripts import native_shadow_boot_rootfs_source_lock_arm64_v2 as tool
from scripts import native_shadow_guest_init_compatibility_arm64_v1 as guest_init

CONTAINMENT = REPO_ROOT / "native" / "containment"
SEALED_GATE = REPO_ROOT / "scripts" / "test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py"
CONTRACT_PATH = CONTAINMENT / "native-shadow-guest-init-compatibility-arm64-v1.json"
PREDECESSOR_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
PREDECESSOR_RESULT_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-source-lock-result-arm64-v1.json"
)
EXPECTATION_PATH = CONTAINMENT / "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
RUNTIME_LOCK_PATH = CONTAINMENT / "native-shadow-runtime-rootfs-source-lock-arm64-v1.json"

# The key set ``_validate_source_lock_identity`` in the frozen guest-init
# contract accepts, exactly. A successor that grows a key here stops being a
# source lock the sealed consumers can read, which is why the nested tree is
# declared in the plan and recorded in the result rather than added to the lock.
LOCK_KEYS = {
    "activationAllowed",
    "artifacts",
    "authorityBindings",
    "buildRecipe",
    "closureRoots",
    "derivedEntries",
    "platform",
    "release",
    "rust",
    "schema",
    "trackedFiles",
    "ubuntu",
}

ACCOUNT_PATHS = (
    "/etc/group",
    "/etc/gshadow",
    "/etc/nsswitch.conf",
    "/etc/passwd",
    "/etc/shadow",
)


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class Fixture(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.plan = tool.load_plan()
        cls.lock = tool.build_source_lock(cls.plan)
        cls.audit = tool.verify_source_lock(cls.plan, cls.lock)
        cls.result = tool.build_result(cls.plan, cls.lock, cls.audit)
        cls.contract = load(CONTRACT_PATH)
        cls.predecessor_lock = load(PREDECESSOR_LOCK_PATH)
        cls.predecessor_result = load(PREDECESSOR_RESULT_PATH)

    def tracked(self, document: dict | None = None) -> dict:
        rows = (document or self.lock)["trackedFiles"]
        return {row["logicalPath"]: row for row in rows}

    def fresh(self) -> tuple[dict, dict]:
        plan = copy.deepcopy(self.plan)
        return plan, tool.build_source_lock(plan)


class ChainPositionTests(Fixture):
    """This step produces a tool. Step three produces the documents."""

    def test_the_generator_pins_the_frozen_plan(self):
        raw = tool.PLAN_PATH.read_bytes()
        self.assertEqual(tool.sha256_bytes(raw), tool.PLAN_SHA256)
        self.assertEqual(len(raw), tool.PLAN_SIZE_BYTES)

    def test_no_digest_is_part_of_its_own_preimage(self):
        """The predecessor had to zero its own pin before hashing. This one does not.

        The plan is already frozen, so it names no generator, so the generator can
        pin it outright and the chain runs one way.
        """

        self.assertNotIn("authorityInputs", self.plan)
        self.assertNotIn(tool.PLAN_SHA256, self.plan.get("release", ""))
        self.assertEqual(self.result["generatorSha256"], tool.sha256_file(tool.TOOL_PATH))

    def test_the_successor_documents_are_sealed_by_the_third_step(self):
        """Superseded: these two lines required absence until the third step ran.

        The digests they were sealed at are pinned in the step-three gate, so this
        assertion stays about chain position and does not become a second, weaker
        copy of the byte facts.
        """

        self.assertTrue(tool.LOCK_PATH.is_file())
        self.assertTrue(tool.RESULT_PATH.is_file())
        self.assertTrue(SEALED_GATE.is_file())
        sealed = SEALED_GATE.read_text(encoding="utf-8")
        self.assertIn(tool.sha256_file(tool.LOCK_PATH), sealed)
        self.assertIn(tool.sha256_file(tool.RESULT_PATH), sealed)

    def test_check_accepts_once_the_sealing_step_has_run(self):
        """Superseded: --check refused while the documents were absent."""

        self.assertEqual(tool.main(["--check"]), 0)

    def test_a_dry_run_builds_and_verifies_and_writes_nothing(self):
        before = (tool.LOCK_PATH.read_bytes(), tool.RESULT_PATH.read_bytes())
        self.assertEqual(tool.main(["--dry-run"]), 0)
        self.assertEqual((tool.LOCK_PATH.read_bytes(), tool.RESULT_PATH.read_bytes()), before)

    def test_the_refusal_that_hands_sealing_to_the_third_step_is_still_reachable(self):
        """The tool still refuses --check when a sealed document is missing.

        The third step supersedes the absence, not the refusal: delete either
        document and the tool says so rather than regenerating it silently.
        """

        original = tool.LOCK_PATH.read_bytes()
        tool.LOCK_PATH.unlink()
        try:
            with self.assertRaises(tool.SourceLockError) as raised:
                tool.main(["--check"])
            self.assertIn("third step", str(raised.exception))
        finally:
            tool.LOCK_PATH.write_bytes(original)

    def test_the_predecessor_documents_are_left_byte_unchanged(self):
        unchanged = {
            row["path"]: row for row in self.plan["appendOnly"]["recordsLeftByteUnchanged"]
        }
        for path in (
            "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json",
            "native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json",
        ):
            self.assertIn(path, unchanged)
            self.assertEqual(
                tool.sha256_file(REPO_ROOT / path), unchanged[path]["sha256"], path
            )


class PlanTests(Fixture):
    def test_the_plan_schema_is_the_successor_schema(self):
        self.assertEqual(self.plan["schema"], tool.PLAN_SCHEMA)
        self.assertTrue(tool.PLAN_SCHEMA.endswith(".v2"))

    def test_a_plan_that_claims_something_was_built_is_refused(self):
        for key in sorted(self.plan["whatWasBuilt"]):
            plan = copy.deepcopy(self.plan)
            plan["whatWasBuilt"][key] = True
            with tempfile.TemporaryDirectory() as scratch:
                path = pathlib.Path(scratch) / "plan.json"
                path.write_bytes(tool.canonical_json(plan))
                with self.assertRaises(tool.SourceLockError, msg=key):
                    tool.load_plan(path)

    def test_a_plan_that_allows_activation_is_refused(self):
        plan = copy.deepcopy(self.plan)
        plan["activationAllowed"] = True
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "plan.json"
            path.write_bytes(tool.canonical_json(plan))
            with self.assertRaises(tool.SourceLockError):
                tool.load_plan(path)

    def test_a_drifted_byte_unchanged_record_is_refused(self):
        plan = copy.deepcopy(self.plan)
        plan["appendOnly"]["recordsLeftByteUnchanged"][0]["sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "plan.json"
            path.write_bytes(tool.canonical_json(plan))
            with self.assertRaises(tool.SourceLockError) as raised:
                tool.load_plan(path)
        self.assertIn("drifted", str(raised.exception))

    def test_a_non_canonical_plan_is_refused(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "plan.json"
            path.write_text(json.dumps(self.plan), encoding="utf-8")
            with self.assertRaises(tool.SourceLockError):
                tool.load_plan(path)

    def test_the_deferred_roles_are_read_back_from_the_plan(self):
        deferred = [
            row["role"] for row in self.plan["guestInitRoles"] if row["state"] == "deferred"
        ]
        self.assertEqual(tool.deferred_roles(self.plan), sorted(deferred))
        self.assertEqual(deferred, ["tracked-file:launcher-binary"])


class LockShapeTests(Fixture):
    def test_the_lock_carries_only_the_keys_the_frozen_contract_allows(self):
        self.assertEqual(set(self.lock), LOCK_KEYS)
        self.assertNotIn("nestedTrees", self.lock)

    def test_the_lock_names_the_successor_release_and_the_inherited_schema(self):
        self.assertEqual(self.lock["release"], tool.LOCK_RELEASE)
        self.assertIn("V2", tool.LOCK_RELEASE)
        self.assertIn("NOT-BOOTABLE", tool.LOCK_RELEASE)
        self.assertEqual(self.lock["schema"], self.predecessor_lock["schema"])
        self.assertNotEqual(self.lock["release"], self.predecessor_lock["release"])

    def test_the_lock_tracks_fifteen_files_sorted_and_role_free(self):
        rows = self.lock["trackedFiles"]
        self.assertEqual(len(rows), self.plan["changesFromPredecessor"]["trackedFileCountAfter"])
        self.assertEqual(len(rows), 15)
        paths = [row["logicalPath"] for row in rows]
        self.assertEqual(paths, sorted(paths))
        for row in rows:
            self.assertNotIn("role", row)
            self.assertEqual((row["uid"], row["gid"]), (0, 0))

    def test_every_tracked_source_is_on_disk_at_its_pinned_digest(self):
        for row in self.lock["trackedFiles"]:
            source = REPO_ROOT / row["sourcePath"]
            self.assertTrue(source.is_file(), row["sourcePath"])
            self.assertEqual(tool.sha256_file(source), row["sha256"], row["logicalPath"])

    def test_the_bindings_are_one_for_one_with_the_tracked_files(self):
        bindings = self.lock["authorityBindings"]
        self.assertEqual(len(bindings), len(self.lock["trackedFiles"]))
        ids = [row["id"] for row in bindings]
        self.assertEqual(ids, sorted(set(ids)))
        self.assertEqual(
            {(row["sourcePath"], row["sha256"]) for row in bindings},
            {(row["sourcePath"], row["sha256"]) for row in self.lock["trackedFiles"]},
        )

    def test_the_package_closure_is_unchanged_from_the_predecessor(self):
        for key in ("artifacts",):
            self.assertEqual(self.lock[key], self.predecessor_lock[key])
        for key in ("packages", "repositories", "seedPackageIds", "seeds", "snapshot"):
            self.assertEqual(self.lock["ubuntu"][key], self.predecessor_lock["ubuntu"][key], key)

    def test_the_lock_adds_no_closure_root_and_no_derived_entry(self):
        self.assertEqual(self.lock["closureRoots"], self.predecessor_lock["closureRoots"])
        self.assertEqual(self.lock["derivedEntries"], self.predecessor_lock["derivedEntries"])

    def test_the_lock_does_not_state_a_launcher_binary_digest(self):
        self.assertNotIn(
            predecessor_tool.LAUNCHER_BINARY_GUEST_PATH, self.tracked()
        )

    def test_the_five_added_rows_are_the_account_database(self):
        added = set(self.tracked()) - set(self.tracked(self.predecessor_lock))
        self.assertEqual(sorted(added), sorted(ACCOUNT_PATHS))
        self.assertEqual(
            len(added), self.plan["changesFromPredecessor"]["addedTrackedSources"]
        )


class SupersessionTests(Fixture):
    def setUp(self) -> None:
        self.moved = {
            row["logicalPath"]: row
            for row in self.plan["changesFromPredecessor"]["supersessions"]
        }
        self.pinned = {
            row["logicalPath"]: row for row in self.contract["trackedFileRequirements"]
        }

    def test_exactly_two_rows_moved_and_both_keep_their_guest_path(self):
        self.assertEqual(len(self.moved), 2)
        predecessor_rows = self.tracked(self.predecessor_lock)
        for path, row in self.moved.items():
            self.assertIn(path, predecessor_rows)
            self.assertIn(path, self.tracked())
            self.assertNotEqual(row["newSourcePath"], row["oldSourcePath"])

    def test_each_old_digest_is_the_one_the_frozen_contract_pins(self):
        for path, row in self.moved.items():
            self.assertEqual(self.pinned[path]["sha256"], row["oldSha256"], path)

    def test_each_new_digest_is_the_file_on_disk(self):
        for row in self.moved.values():
            source = REPO_ROOT / row["newSourcePath"]
            raw = source.read_bytes()
            self.assertEqual(tool.sha256_bytes(raw), row["newSha256"])
            self.assertEqual(len(raw), row["newSizeBytes"])

    def test_both_binding_identities_are_inherited_rather_than_reissued(self):
        bindings = {row["id"]: row for row in self.lock["authorityBindings"]}
        inherited = {row["id"]: row for row in self.predecessor_lock["authorityBindings"]}
        for row in self.moved.values():
            role = row["role"]
            self.assertIn(role, inherited)
            self.assertEqual(bindings[role]["sourcePath"], row["newSourcePath"])
            self.assertNotEqual(bindings[role]["sha256"], inherited[role]["sha256"])

    def test_both_predecessor_sources_are_still_in_the_tree(self):
        unchanged = {
            row["path"]: row for row in self.plan["appendOnly"]["recordsLeftByteUnchanged"]
        }
        for row in self.moved.values():
            path = row["oldSourcePath"]
            self.assertIn(path, unchanged)
            self.assertTrue((REPO_ROOT / path).is_file())
            self.assertEqual(tool.sha256_file(REPO_ROOT / path), row["oldSha256"])

    def test_the_unit_gains_the_console_and_nothing_else(self):
        row = self.moved["/usr/lib/systemd/system/boole-native-shadow-launcher.service"]
        was = (REPO_ROOT / row["oldSourcePath"]).read_text(encoding="utf-8").splitlines()
        now = (REPO_ROOT / row["newSourcePath"]).read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(was), len(now))
        differing = [(a, b) for a, b in zip(was, now) if a != b]
        self.assertEqual(len(differing), 2)
        for before, after in differing:
            self.assertEqual(after, before + "+console")
            self.assertIn(before.split("=", 1)[0], ("StandardOutput", "StandardError"))

    def test_the_tmpfiles_successor_drops_only_the_read_only_root_rules(self):
        row = self.moved["/usr/lib/tmpfiles.d/boole-native-shadow.conf"]
        was = (REPO_ROOT / row["oldSourcePath"]).read_text(encoding="utf-8").splitlines()
        now = (REPO_ROOT / row["newSourcePath"]).read_text(encoding="utf-8").splitlines()
        dropped = [line for line in was if line not in now]
        self.assertEqual(len(dropped), 3)
        for line in dropped:
            self.assertTrue(line.split()[1].startswith("/var/lib/boole"), line)
        for line in now:
            if not line.strip() or line.startswith("#"):
                continue
            self.assertEqual(line.split()[0], "d")
            self.assertTrue(line.split()[1].startswith("/run/"), line)

    def test_a_supersession_whose_old_digest_is_not_the_pinned_one_is_refused(self):
        plan, lock = self.fresh()
        plan["changesFromPredecessor"]["supersessions"][0]["oldSha256"] = "0" * 64
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)

    def test_a_supersession_that_moves_the_placement_as_well_is_refused(self):
        plan = copy.deepcopy(self.plan)
        path = plan["changesFromPredecessor"]["supersessions"][0]["logicalPath"]
        for row in plan["trackedFiles"]:
            if row["logicalPath"] == path:
                row["mode"] = "0400"
        lock = tool.build_source_lock(plan)
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)


class ShadowLockTests(Fixture):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls.shadow = tool.build_shadow_lock(cls.plan, cls.lock)

    def test_the_shadow_restores_exactly_the_two_predecessor_sources(self):
        moved = {
            row["logicalPath"]: row
            for row in self.plan["changesFromPredecessor"]["supersessions"]
        }
        shadow_rows = self.tracked(self.shadow)
        for path, want in moved.items():
            self.assertEqual(shadow_rows[path]["sourcePath"], want["oldSourcePath"])
            self.assertEqual(shadow_rows[path]["sha256"], want["oldSha256"])
        differing = [
            row["logicalPath"]
            for row, was in zip(self.lock["trackedFiles"], self.shadow["trackedFiles"])
            if row != was
        ]
        self.assertEqual(sorted(differing), sorted(moved))

    def test_the_frozen_contract_still_returns_the_predecessors_verdict(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "shadow.json"
            path.write_bytes(tool.canonical_json(self.shadow))
            audit = guest_init.audit_successor_source_shape(CONTRACT_PATH, path)
        sealed = self.predecessor_result["sourceShapeAudit"]
        self.assertEqual(audit["status"], sealed["status"])
        self.assertEqual(audit["missingRoles"], sealed["missingRoles"])
        self.assertEqual(
            self.audit["predecessorContractAudit"]["shadowSourceLockSha256"],
            audit["sourceLockSha256"],
        )

    def test_the_frozen_contract_refuses_the_successor_itself(self):
        """The contract pins two digests this successor moves, so it must refuse.

        Recording the refusal is the point. A successor that the frozen contract
        accepted unchanged would be one that moved nothing.
        """

        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "successor.json"
            path.write_bytes(tool.canonical_json(self.lock))
            with self.assertRaises(guest_init.GuestInitCompatibilityError) as raised:
                guest_init.audit_successor_source_shape(CONTRACT_PATH, path)
        self.assertIn("digest differs", str(raised.exception))

    def test_a_third_moved_digest_is_caught_by_the_frozen_contract(self):
        """A row that moves without being a recorded supersession is not restored.

        The shadow only restores what the plan records, so an unrecorded move
        survives into the shadow and the frozen contract answers for it.
        """

        plan = copy.deepcopy(self.plan)
        source = "native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json"
        digest = tool.sha256_file(REPO_ROOT / source)
        for rows, key in ((plan["trackedFiles"], "logicalPath"), (plan["authorityBindings"], "id")):
            for row in rows:
                if row[key] in ("/etc/machine-id", "guest-machine-id"):
                    row["sourcePath"] = source
                    row["sha256"] = digest
        lock = tool.build_source_lock(plan)
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("refused the unmoved part", str(raised.exception))
        self.assertIn("empty-machine-id", str(raised.exception))


class AccountDatabaseTests(Fixture):
    """The plan lists the clauses. Listing is not evidence, so these re-derive them."""

    def swap(self, logical_path: str, text: str) -> tuple[dict, dict]:
        """Point one account file at replacement bytes, binding and all."""

        plan, lock = self.fresh()
        role = self.tracked(plan)[logical_path]["role"]
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        target = pathlib.Path(scratch.name) / pathlib.PurePosixPath(logical_path).name
        target.write_text(text, encoding="utf-8")
        digest = tool.sha256_file(target)
        for document in (plan, lock):
            for row in document["trackedFiles"]:
                if row["logicalPath"] == logical_path:
                    row["sourcePath"] = str(target)
                    row["sha256"] = digest
            for row in document["authorityBindings"]:
                if row["id"] == role:
                    row["sourcePath"] = str(target)
                    row["sha256"] = digest
        return plan, lock

    def source(self, logical_path: str) -> str:
        row = self.tracked()[logical_path]
        return (REPO_ROOT / row["sourcePath"]).read_text(encoding="utf-8")

    def test_all_eight_clauses_hold_for_both_accounts(self):
        self.assertEqual(self.audit["identityClausesVerified"], 8)
        self.assertEqual(len(self.plan["identityContractClauses"]), 8)
        self.assertEqual(
            [row["name"] for row in self.audit["accountsVerified"]],
            ["boole-native-checker", "boole-node"],
        )

    def test_the_two_accounts_share_neither_uid_nor_gid(self):
        accounts = self.audit["accountsVerified"]
        self.assertEqual(len({row["uid"] for row in accounts}), 2)
        self.assertEqual(len({row["gid"] for row in accounts}), 2)
        for row in accounts:
            self.assertNotEqual(row["uid"], 0)
            self.assertNotEqual(row["gid"], 0)

    def test_the_password_bearing_files_are_root_only(self):
        rows = self.tracked()
        self.assertEqual(rows["/etc/shadow"]["mode"], "0400")
        self.assertEqual(rows["/etc/gshadow"]["mode"], "0400")
        for path in ("/etc/group", "/etc/nsswitch.conf", "/etc/passwd"):
            self.assertEqual(rows[path]["mode"], "0444")

    def test_a_real_shell_is_refused(self):
        text = self.source("/etc/passwd").replace("/usr/sbin/nologin", "/bin/bash")
        plan, lock = self.swap("/etc/passwd", text)
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("shell", str(raised.exception))

    def test_a_zero_user_id_is_refused(self):
        text = "\n".join(
            ":".join(["0" if index == 2 else field for index, field in enumerate(line.split(":"))])
            if line.startswith("boole-node:")
            else line
            for line in self.source("/etc/passwd").splitlines()
        )
        plan, lock = self.swap("/etc/passwd", text + "\n")
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("uid", str(raised.exception))

    def test_a_home_that_is_not_nonexistent_is_refused(self):
        text = self.source("/etc/passwd").replace("/nonexistent", "/var/lib/boole")
        plan, lock = self.swap("/etc/passwd", text)
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("home", str(raised.exception))

    def test_an_ambiguous_reverse_lookup_is_refused(self):
        text = self.source("/etc/group") + "shadow-collision:x:990:\n"
        plan, lock = self.swap("/etc/group", text)
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)

    def test_a_supplementary_group_membership_is_refused(self):
        lines = self.source("/etc/group").splitlines()
        lines.append("extra:x:993:boole-node")
        plan, lock = self.swap("/etc/group", "\n".join(lines) + "\n")
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("supplementary", str(raised.exception))

    def test_a_lock_that_drops_the_account_database_reports_the_missing_roles(self):
        plan = copy.deepcopy(self.plan)
        plan["trackedFiles"] = [
            row for row in plan["trackedFiles"] if row["logicalPath"] != "/etc/passwd"
        ]
        plan["authorityBindings"] = [
            row for row in plan["authorityBindings"] if row["id"] != "guest-passwd"
        ]
        lock = tool.build_source_lock(plan)
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("account database", str(raised.exception))


class NestedTreeTests(Fixture):
    def setUp(self) -> None:
        self.tree = self.plan["nestedTrees"][0]
        self.manifest = self.tree["contentManifest"]

    def test_the_manifest_digest_is_the_one_the_launcher_compiles_against(self):
        self.assertEqual(
            tool._arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256"),
            self.manifest["sha256"],
        )

    def test_the_manifest_size_and_schema_are_the_arm64_gated_values(self):
        self.assertEqual(
            tool._arm64_number("RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE"),
            self.manifest["sizeBytes"],
        )
        self.assertEqual(
            tool._arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA"),
            self.manifest["schema"],
        )

    def test_the_arm64_reader_does_not_return_the_ungated_value(self):
        """Every one of these names is declared twice, with different values."""

        text = tool.AUTHORITY_ARCH_PATH.read_text(encoding="utf-8")
        self.assertEqual(
            text.count("pub(crate) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256:"), 2
        )
        self.assertIn("957761ceaeca18e0af516ed200c7587aa57a609b16ebfe63dacb1371df489763", text)
        self.assertNotEqual(
            tool._arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256"),
            "957761ceaeca18e0af516ed200c7587aa57a609b16ebfe63dacb1371df489763",
        )

    def test_the_manifest_digest_is_the_one_the_replay_expectation_seals(self):
        expectation = load(EXPECTATION_PATH)
        self.assertEqual(
            expectation["expectedOutput"]["rootfsContentManifestSha256"],
            self.manifest["sha256"],
        )

    def test_the_driving_lock_is_the_sealed_runtime_lock(self):
        driver = self.tree["drivenBy"]
        self.assertEqual(REPO_ROOT / driver["path"], RUNTIME_LOCK_PATH)
        raw = RUNTIME_LOCK_PATH.read_bytes()
        self.assertEqual(tool.sha256_bytes(raw), driver["sha256"])
        self.assertEqual(len(raw), driver["sizeBytes"])
        runtime = json.loads(raw.decode("utf-8"))
        self.assertEqual(len(runtime["artifacts"]), driver["artifactCount"])
        self.assertEqual(len(runtime["closureRoots"]), driver["closureRootCount"])

    def test_the_tree_is_declared_and_not_assembled(self):
        self.assertEqual(self.audit["nestedTree"]["state"], "declared-not-assembled")
        self.assertFalse(self.audit["nestedTree"]["assembled"])
        self.assertFalse(self.result["boundaries"]["nestedRuntimeTreeAssembled"])
        self.assertTrue(self.tree["requiresBuilderChange"])

    def test_the_lock_tracks_nothing_inside_the_nested_prefix(self):
        for path in self.tracked():
            self.assertFalse(path.startswith(self.tree["guestPrefix"]), path)
        self.assertNotIn(self.manifest["guestPath"], self.tracked())
        self.assertFalse(self.manifest["isATrackedSourceRow"])

    def test_a_manifest_digest_that_drifts_from_the_launcher_is_refused(self):
        plan, lock = self.fresh()
        plan["nestedTrees"][0]["contentManifest"]["sha256"] = "0" * 64
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("compiles against", str(raised.exception))

    def test_a_tree_claimed_as_assembled_is_refused(self):
        plan, lock = self.fresh()
        plan["nestedTrees"][0]["state"] = "assembled"
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)


class AuditTests(Fixture):
    def test_the_only_missing_role_is_the_deferred_launcher_binary(self):
        self.assertEqual(self.audit["missingRoles"], ["tracked-file:launcher-binary"])

    def test_the_status_is_the_predecessors_blocked_status(self):
        self.assertEqual(
            self.audit["status"], self.predecessor_result["sourceShapeAudit"]["status"]
        )
        self.assertEqual(self.audit["status"], "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS")

    def test_the_successor_requirements_are_stricter_than_the_contracts(self):
        """Every contract row is kept, two get a digest, five rows are added."""

        rows = tool._successor_requirements(self.plan)
        contract_paths = {row["logicalPath"] for row in self.contract["trackedFileRequirements"]}
        paths = {row["logicalPath"] for row in rows}
        self.assertTrue(contract_paths.issubset(paths))
        self.assertEqual(len(paths - contract_paths), 5)
        moved = {
            row["logicalPath"]: row
            for row in self.plan["changesFromPredecessor"]["supersessions"]
        }
        for row in rows:
            want = moved.get(row["logicalPath"])
            if want is not None:
                self.assertEqual(row["sha256"], want["newSha256"])
        null_digests = [row["logicalPath"] for row in rows if row["sha256"] is None]
        self.assertEqual(null_digests, [predecessor_tool.LAUNCHER_BINARY_GUEST_PATH])

    def test_the_supersessions_the_audit_verified_are_the_two_the_plan_records(self):
        self.assertEqual(
            self.audit["supersessionsVerified"],
            sorted(
                row["role"] for row in self.plan["changesFromPredecessor"]["supersessions"]
            ),
        )

    def test_the_audit_digest_is_the_digest_of_the_canonical_lock(self):
        self.assertEqual(
            self.audit["sourceLockSha256"],
            tool.sha256_bytes(tool.canonical_json(self.lock)),
        )


class ResultTests(Fixture):
    def test_the_result_pins_the_plan_and_the_generator(self):
        self.assertEqual(self.result["planSha256"], tool.PLAN_SHA256)
        self.assertEqual(self.result["generatorSha256"], tool.sha256_file(tool.TOOL_PATH))
        self.assertEqual(
            self.result["predecessorBootSourceLockSha256"],
            tool.sha256_file(PREDECESSOR_LOCK_PATH),
        )

    def test_every_boundary_is_false(self):
        self.assertTrue(self.result["boundaries"])
        for key, value in sorted(self.result["boundaries"].items()):
            self.assertFalse(value, key)

    def test_the_result_claims_no_boot_no_image_and_no_activation(self):
        self.assertFalse(self.result["activationAllowed"])
        self.assertFalse(self.result["bootableClaim"])
        self.assertEqual(self.result["bootArtifactsWritten"], 0)
        self.assertFalse(self.result["productionByteProvenanceComplete"])

    def test_the_counts_match_the_lock(self):
        counts = self.result["counts"]
        self.assertEqual(counts["trackedFiles"], len(self.lock["trackedFiles"]))
        self.assertEqual(counts["authorityBindings"], len(self.lock["authorityBindings"]))
        self.assertEqual(counts["artifacts"], len(self.lock["artifacts"]))
        self.assertEqual(counts["packages"], len(self.lock["ubuntu"]["packages"]))
        self.assertEqual(counts["packages"], self.predecessor_result["counts"]["packages"])
        self.assertEqual(counts["packageBytes"], self.predecessor_result["counts"]["packageBytes"])

    def test_the_result_names_the_three_gaps_the_chain_closes(self):
        self.assertEqual(
            self.result["gapsAddressed"],
            [
                "account-database",
                "refusal-is-not-readable",
                "runtime-rootfs-and-its-content-manifest",
            ],
        )

    def test_the_result_records_the_account_database_as_baked_in(self):
        account = self.result["accountDatabase"]
        self.assertTrue(account["bakedIntoTheImage"])
        self.assertFalse(account["provisionedAtBoot"])
        self.assertEqual(account["identityClausesVerified"], 8)

    def test_the_result_carries_the_frozen_contracts_verdict_on_the_unmoved_part(self):
        audit = self.result["predecessorContractAudit"]
        self.assertEqual(audit["contractSha256"], tool.sha256_file(CONTRACT_PATH))
        self.assertEqual(audit["status"], self.predecessor_result["sourceShapeAudit"]["status"])
        self.assertIn("refuses the successor lock itself", audit["note"])

    def test_the_result_schema_is_a_successor_schema(self):
        self.assertEqual(self.result["schema"], tool.RESULT_SCHEMA)
        self.assertTrue(tool.RESULT_SCHEMA.endswith(".v2"))
        self.assertNotEqual(self.result["schema"], self.predecessor_result["schema"])

    def test_the_result_is_canonical_json(self):
        raw = tool.canonical_json(self.result)
        self.assertEqual(json.loads(raw.decode("utf-8")), self.result)


class InheritedGroundTests(Fixture):
    """The predecessor's grounds are run, not reworded. These prove they still bite."""

    def test_a_lock_that_tracks_the_launcher_binary_is_refused(self):
        plan, lock = self.fresh()
        row = dict(lock["trackedFiles"][0])
        row["logicalPath"] = predecessor_tool.LAUNCHER_BINARY_GUEST_PATH
        lock["trackedFiles"] = sorted(
            [*lock["trackedFiles"], row], key=lambda item: item["logicalPath"]
        )
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("launcher binary", str(raised.exception))

    def test_a_dropped_package_is_refused(self):
        plan, lock = self.fresh()
        lock["ubuntu"]["packages"] = lock["ubuntu"]["packages"][1:]
        with self.assertRaises(tool.SourceLockError) as raised:
            tool.verify_source_lock(plan, lock)
        self.assertIn("drops a package", str(raised.exception))

    def test_a_tracked_digest_that_differs_from_disk_is_refused(self):
        plan, lock = self.fresh()
        for document in (plan, lock):
            for row in document["trackedFiles"]:
                if row["logicalPath"] == "/etc/passwd":
                    row["sha256"] = "0" * 64
            for row in document.get("authorityBindings", []):
                if row["id"] == "guest-passwd":
                    row["sha256"] = "0" * 64
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)

    def test_an_unsorted_tracked_list_is_refused(self):
        plan, lock = self.fresh()
        lock["trackedFiles"] = list(reversed(lock["trackedFiles"]))
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)

    def test_a_binding_that_covers_no_tracked_file_is_refused(self):
        plan, lock = self.fresh()
        lock["authorityBindings"] = [
            row for row in lock["authorityBindings"] if row["id"] != "guest-shadow"
        ]
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)

    def test_a_lock_that_permits_maintainer_scripts_is_refused(self):
        plan, lock = self.fresh()
        lock["buildRecipe"] = dict(lock["buildRecipe"])
        lock["buildRecipe"]["maintainerScripts"] = "execute"
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)

    def test_a_lock_that_drops_the_systemd_seed_is_refused(self):
        plan, lock = self.fresh()
        lock["ubuntu"]["seeds"] = [row for row in lock["ubuntu"]["seeds"] if row != "systemd"]
        with self.assertRaises(tool.SourceLockError):
            tool.verify_source_lock(plan, lock)


if __name__ == "__main__":
    unittest.main()
