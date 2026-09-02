#!/usr/bin/env python3
"""Acceptance tests for the ARM64 boot rootfs source lock plan successor.

The successor plan is step one of the chain that closes the three serving gaps.
It generates no lock, changes no builder and produces no image, so what it can
be held to is narrow: that it names the right files, that the account database
it names actually answers every clause the launcher checks, that the nested
runtime tree it declares is pinned to the digest the sealed launcher compiles
against, and that nothing it supersedes was edited in place.
"""
from __future__ import annotations

import hashlib
import json
import pathlib
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO_ROOT / "native" / "containment"
PLAN_V2_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json"
PLAN_V1_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json"
INPUTS_PATH = CONTAINMENT / "native-shadow-mac3-guest-runtime-inputs-arm64-v1.json"
CLOSURE_PLAN_PATH = CONTAINMENT / "native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json"
MOUNT_POINTS_PATH = CONTAINMENT / "native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json"
GUEST_INIT_PATH = CONTAINMENT / "native-shadow-guest-init-compatibility-arm64-v1.json"
EXPECTATION_PATH = CONTAINMENT / "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
AUTHORITY_ARCH_PATH = (
    REPO_ROOT / "crates/boole-native-shadow-launcher/src/authority_arch.rs"
)

REQUIRED_HOME = "/nonexistent"
ALLOWED_SHELLS = ("/usr/sbin/nologin", "/bin/false")
FIXED_ACCOUNTS = ("boole-node", "boole-native-checker")


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def canonical(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False
    ).encode("utf-8") + b"\n"


def digest_of(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def arm64_constant(name: str) -> str:
    """Read one arm64-gated constant out of the launcher's architecture module.

    The module declares each constant twice, once behind the arm64 feature and
    once behind its negation, so the gate on the preceding line is what picks
    the right one.
    """
    lines = AUTHORITY_ARCH_PATH.read_text(encoding="utf-8").splitlines()
    head = f"pub(crate) const {name}:"
    for index, line in enumerate(lines):
        if not line.startswith(head):
            continue
        gate = lines[index - 1]
        if 'feature = "linux-arm64-authority"' not in gate:
            continue
        if 'not(feature = "linux-arm64-authority")' in gate:
            continue
        cursor, text = index, line
        while not text.rstrip().endswith(";"):
            cursor += 1
            text += lines[cursor]
        value = text.split("=", 1)[1].strip().rstrip(";").strip()
        return value.strip('"').replace("_", "")
    raise AssertionError(f"{name} has no arm64-gated declaration")


class Fixture(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.plan = load(PLAN_V2_PATH)
        cls.previous = load(PLAN_V1_PATH)
        cls.inputs = load(INPUTS_PATH)


class ShapeTests(Fixture):
    def test_the_record_is_canonical_json(self) -> None:
        self.assertEqual(canonical(self.plan), PLAN_V2_PATH.read_bytes())

    def test_it_declares_itself_a_successor_that_generated_nothing(self) -> None:
        self.assertEqual(
            self.plan["schema"], "boole.native-shadow.boot-rootfs-source-lock-plan.arm64.v2"
        )
        self.assertEqual(
            self.plan["status"],
            "BOOT-ROOTFS-SOURCE-LOCK-PLAN-SUCCESSOR-FROZEN-LOCK-NOT-GENERATED",
        )
        self.assertFalse(self.plan["activationAllowed"])
        for field, value in self.plan["whatWasBuilt"].items():
            self.assertFalse(value, f"{field} must be false in a plan that built nothing")

    def test_it_is_step_one_of_the_chain_the_closure_plan_fixed(self) -> None:
        position = self.plan["successorChainPosition"]
        self.assertEqual(position["step"], 1)
        closure = load(CLOSURE_PLAN_PATH)
        expected = closure["successorChainRequired"]["steps"]
        self.assertEqual(position["what"], expected[0]["what"])
        self.assertEqual(
            [row["what"] for row in position["remaining"]],
            [row["what"] for row in expected[1:]],
        )
        for row in position["remaining"]:
            self.assertEqual(row["state"], "not-started")


class PredecessorTests(Fixture):
    def test_the_predecessor_is_pinned_and_unedited(self) -> None:
        predecessor = self.plan["predecessor"]
        self.assertTrue(predecessor["leftByteUnchanged"])
        self.assertEqual(predecessor["sha256"], digest_of(PLAN_V1_PATH))
        self.assertEqual(predecessor["sizeBytes"], PLAN_V1_PATH.stat().st_size)
        self.assertEqual(predecessor["release"], self.previous["release"])

    def test_every_record_it_promises_not_to_edit_is_unedited(self) -> None:
        rows = self.plan["appendOnly"]["recordsLeftByteUnchanged"]
        self.assertGreaterEqual(len(rows), 8)
        for row in rows:
            path = REPO_ROOT / row["path"]
            self.assertTrue(path.is_file(), f"{row['path']} is gone")
            self.assertEqual(digest_of(path), row["sha256"], row["path"])
            self.assertEqual(path.stat().st_size, row["sizeBytes"], row["path"])

    def test_the_superseded_sources_stay_in_the_tree(self) -> None:
        promised = {row["path"] for row in self.plan["appendOnly"]["recordsLeftByteUnchanged"]}
        for row in self.plan["changesFromPredecessor"]["supersessions"]:
            self.assertTrue(row["predecessorLeftInTree"])
            self.assertIn(row["oldSourcePath"], promised)
            old = REPO_ROOT / row["oldSourcePath"]
            self.assertEqual(digest_of(old), row["oldSha256"])

    def test_the_carried_sections_are_carried_verbatim(self) -> None:
        for name in self.plan["carriedForwardVerbatim"]:
            self.assertEqual(
                canonical(self.plan[name]), canonical(self.previous[name]), name
            )


class TrackedFileTests(Fixture):
    def test_every_tracked_source_is_present_at_the_pinned_bytes(self) -> None:
        for row in self.plan["trackedFiles"]:
            path = REPO_ROOT / row["sourcePath"]
            self.assertTrue(path.is_file(), row["sourcePath"])
            self.assertEqual(digest_of(path), row["sha256"], row["sourcePath"])
            self.assertEqual(row["uid"], 0)
            self.assertEqual(row["gid"], 0)
            self.assertIn(row["mode"], ("0400", "0444"))

    def test_the_rows_are_sorted_and_unique_by_guest_path(self) -> None:
        paths = [row["logicalPath"] for row in self.plan["trackedFiles"]]
        self.assertEqual(paths, sorted(set(paths)))

    def test_the_count_moves_exactly_as_the_change_summary_says(self) -> None:
        changes = self.plan["changesFromPredecessor"]
        self.assertEqual(changes["trackedFileCountBefore"], len(self.previous["trackedFiles"]))
        self.assertEqual(changes["trackedFileCountAfter"], len(self.plan["trackedFiles"]))
        self.assertEqual(
            changes["trackedFileCountAfter"],
            changes["trackedFileCountBefore"] + changes["addedTrackedSources"],
        )
        self.assertEqual(changes["supersededTrackedSources"], len(changes["supersessions"]))

    def test_no_predecessor_guest_path_was_dropped(self) -> None:
        after = {row["logicalPath"] for row in self.plan["trackedFiles"]}
        for row in self.previous["trackedFiles"]:
            self.assertIn(row["logicalPath"], after)

    def test_the_added_sources_are_exactly_the_frozen_input_files(self) -> None:
        frozen = {row["path"]: row["sha256"] for row in self.inputs["inputs"]}
        carried = {row["logicalPath"] for row in self.previous["trackedFiles"]}
        added = [
            row for row in self.plan["trackedFiles"] if row["logicalPath"] not in carried
        ]
        self.assertEqual(len(added), self.plan["changesFromPredecessor"]["addedTrackedSources"])
        for row in added:
            self.assertIn(row["sourcePath"], frozen, row["sourcePath"])
            self.assertEqual(row["sha256"], frozen[row["sourcePath"]])

    def test_each_supersession_points_at_a_frozen_successor_file(self) -> None:
        frozen = {row["path"]: row for row in self.inputs["inputs"]}
        placed = {row["sourcePath"]: row for row in self.plan["trackedFiles"]}
        for row in self.plan["changesFromPredecessor"]["supersessions"]:
            self.assertIn(row["newSourcePath"], frozen)
            source = frozen[row["newSourcePath"]]
            self.assertEqual(source["sha256"], row["newSha256"])
            self.assertEqual(source["guestPath"], row["logicalPath"])
            self.assertNotEqual(row["newSha256"], row["oldSha256"])
            self.assertNotIn(row["oldSourcePath"], placed)
            self.assertEqual(placed[row["newSourcePath"]]["logicalPath"], row["logicalPath"])

    def test_the_password_files_are_not_world_readable(self) -> None:
        modes = {row["logicalPath"]: row["mode"] for row in self.plan["trackedFiles"]}
        self.assertEqual(modes["/etc/shadow"], "0400")
        self.assertEqual(modes["/etc/gshadow"], "0400")
        self.assertEqual(modes["/etc/passwd"], "0444")
        self.assertEqual(modes["/etc/group"], "0444")
        rationale = self.plan["modeRationale"]["perFile"]
        for guest_path in ("/etc/shadow", "/etc/gshadow", "/etc/passwd", "/etc/group"):
            self.assertIn(guest_path, rationale)


class SupersededContentTests(Fixture):
    """The two replacements are small enough to check by reading them."""

    def source_for(self, guest_path: str) -> pathlib.Path:
        for row in self.plan["trackedFiles"]:
            if row["logicalPath"] == guest_path:
                return REPO_ROOT / row["sourcePath"]
        raise AssertionError(f"no tracked row for {guest_path}")

    def test_every_directory_the_tmpfiles_rules_ask_for_is_memory_backed(self) -> None:
        path = self.source_for("/usr/lib/tmpfiles.d/boole-native-shadow.conf")
        rules = [line.split() for line in path.read_text().splitlines() if line.strip()]
        self.assertTrue(rules)
        for rule in rules:
            self.assertEqual(rule[0], "d", " ".join(rule))
            self.assertTrue(
                rule[1] == "/run" or rule[1].startswith("/run/"),
                f"{rule[1]} is not under the memory-backed run directory",
            )

    def test_the_dropped_rules_are_the_ones_a_read_only_root_refuses(self) -> None:
        old = REPO_ROOT / "native/tmpfiles.d/boole-native-shadow.conf"
        new = self.source_for("/usr/lib/tmpfiles.d/boole-native-shadow.conf")
        before = [line for line in old.read_text().splitlines() if line.strip()]
        after = [line for line in new.read_text().splitlines() if line.strip()]
        dropped = [line for line in before if line not in after]
        self.assertEqual(len(dropped), 3)
        for line in dropped:
            self.assertTrue(line.split()[1].startswith("/var/lib/boole"), line)
        self.assertEqual(after, [line for line in before if line in after])

    def test_the_unit_gains_the_console_and_nothing_else(self) -> None:
        old = REPO_ROOT / "native/systemd/boole-native-shadow-launcher.service"
        new = self.source_for("/usr/lib/systemd/system/boole-native-shadow-launcher.service")
        before = old.read_text().splitlines()
        after = new.read_text().splitlines()
        self.assertEqual(len(before), len(after))
        moved = [(a, b) for a, b in zip(before, after) if a != b]
        self.assertEqual(len(moved), 2)
        for was, now in moved:
            self.assertEqual(now, was + "+console")
            self.assertIn(was.split("=")[0], ("StandardOutput", "StandardError"))


class AuthorityBindingTests(Fixture):
    def test_bindings_and_tracked_rows_agree_one_for_one(self) -> None:
        bindings = self.plan["authorityBindings"]
        tracked = self.plan["trackedFiles"]
        self.assertEqual(len(bindings), len(tracked))
        self.assertEqual([row["id"] for row in bindings], sorted(row["id"] for row in bindings))
        self.assertEqual(
            {row["sourcePath"]: row["sha256"] for row in bindings},
            {row["sourcePath"]: row["sha256"] for row in tracked},
        )

    def test_the_predecessor_binding_identities_survive_the_supersession(self) -> None:
        moved = {
            row["oldSourcePath"]: row["newSourcePath"]
            for row in self.plan["changesFromPredecessor"]["supersessions"]
        }
        after = {row["sourcePath"]: row["id"] for row in self.plan["authorityBindings"]}
        for row in self.previous["authorityBindings"]:
            source = moved.get(row["sourcePath"], row["sourcePath"])
            self.assertEqual(after.get(source), row["id"], source)


class AccountDatabaseTests(Fixture):
    """Re-derive, from the bytes themselves, every clause resolve_one checks."""

    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        sources = {row["logicalPath"]: row["sourcePath"] for row in cls.plan["trackedFiles"]}
        cls.passwd = [
            line.split(":")
            for line in (REPO_ROOT / sources["/etc/passwd"]).read_text().splitlines()
            if line
        ]
        cls.group = [
            line.split(":")
            for line in (REPO_ROOT / sources["/etc/group"]).read_text().splitlines()
            if line
        ]

    def account(self, name: str) -> list[str]:
        rows = [row for row in self.passwd if row[0] == name]
        self.assertEqual(len(rows), 1, f"{name} must appear exactly once in passwd")
        return rows[0]

    def test_both_fixed_accounts_exist_with_a_non_root_identity(self) -> None:
        for name in FIXED_ACCOUNTS:
            row = self.account(name)
            self.assertNotEqual(int(row[2]), 0, name)
            self.assertNotEqual(int(row[3]), 0, name)

    def test_the_home_and_shell_are_the_ones_the_launcher_accepts(self) -> None:
        for name in FIXED_ACCOUNTS:
            row = self.account(name)
            self.assertEqual(row[5], REQUIRED_HOME, name)
            self.assertIn(row[6], ALLOWED_SHELLS, name)

    def test_a_same_named_group_matches_the_primary_group_id(self) -> None:
        for name in FIXED_ACCOUNTS:
            primary = int(self.account(name)[3])
            named = [row for row in self.group if row[0] == name]
            self.assertEqual(len(named), 1, name)
            self.assertEqual(int(named[0][2]), primary, name)

    def test_the_primary_group_id_resolves_back_to_that_group(self) -> None:
        for name in FIXED_ACCOUNTS:
            primary = int(self.account(name)[3])
            by_number = [row for row in self.group if int(row[2]) == primary]
            self.assertEqual(len(by_number), 1, name)
            self.assertEqual(by_number[0][0], name, name)

    def test_neither_account_holds_a_supplementary_group(self) -> None:
        for name in FIXED_ACCOUNTS:
            for row in self.group:
                members = [item for item in row[3].split(",") if item]
                self.assertNotIn(name, members, f"{name} is a member of {row[0]}")

    def test_the_two_accounts_alias_neither_number(self) -> None:
        node = self.account("boole-node")
        checker = self.account("boole-native-checker")
        self.assertNotEqual(node[2], checker[2])
        self.assertNotEqual(node[3], checker[3])

    def test_the_plan_carries_the_clause_list_the_input_record_froze(self) -> None:
        self.assertEqual(
            canonical(self.plan["identityContractClauses"]),
            canonical(self.inputs["identityContractClauses"]),
        )
        self.assertEqual(len(self.plan["identityContractClauses"]), 8)


class GuestInitRoleTests(Fixture):
    def test_the_roles_stay_sorted_and_unique(self) -> None:
        roles = [row["role"] for row in self.plan["guestInitRoles"]]
        self.assertEqual(roles, sorted(set(roles)))

    def test_every_predecessor_role_keeps_its_state(self) -> None:
        after = {row["role"]: row["state"] for row in self.plan["guestInitRoles"]}
        for row in self.previous["guestInitRoles"]:
            self.assertEqual(after.get(row["role"]), row["state"], row["role"])

    def test_the_account_database_role_is_the_one_that_was_added(self) -> None:
        before = {row["role"] for row in self.previous["guestInitRoles"]}
        after = {row["role"] for row in self.plan["guestInitRoles"]}
        self.assertEqual(after - before, {"tracked-file:account-database"})
        added = [
            row
            for row in self.plan["guestInitRoles"]
            if row["role"] == "tracked-file:account-database"
        ][0]
        self.assertEqual(added["state"], "closed")

    def test_a_reworded_role_is_either_a_correction_or_a_supersession(self) -> None:
        """Two reasons a sentence may move, and each has to leave its own trace.

        A sentence that was wrong carries the sentence it replaces and why. A
        sentence that merely describes a file that has been superseded needs no
        correction block, but the supersession has to be in the change summary.
        Anything else is an unexplained rewrite.
        """
        earlier = {row["role"]: row["closedBy"] for row in self.previous["guestInitRoles"]}
        superseded = {
            "tracked-file:" + row["role"]
            for row in self.plan["changesFromPredecessor"]["supersessions"]
        }
        for row in self.plan["guestInitRoles"]:
            role, correction = row["role"], row.get("correctsThePredecessor")
            if role not in earlier:
                self.assertIsNone(correction, role)
                continue
            if correction is not None:
                self.assertEqual(correction["earlierClosedBy"], earlier[role], role)
                self.assertNotEqual(row["closedBy"], correction["earlierClosedBy"], role)
                self.assertTrue(correction["why"].strip(), role)
            elif row["closedBy"] != earlier[role]:
                self.assertIn(role, superseded, role)


class NestedTreeTests(Fixture):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        trees = cls.plan["nestedTrees"]
        assert len(trees) == 1
        cls.tree = trees[0]

    def test_it_is_declared_and_not_assembled(self) -> None:
        self.assertEqual(self.tree["state"], "declared-not-assembled")
        self.assertTrue(self.tree["requiresBuilderChange"])
        self.assertEqual(
            self.tree["guestPrefix"], "/var/lib/boole/native-shadow/runtime-rootfs"
        )

    def test_the_manifest_digest_is_the_one_the_sealed_launcher_compiles_against(self) -> None:
        manifest = self.tree["contentManifest"]
        self.assertEqual(
            manifest["sha256"], arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256")
        )
        self.assertEqual(
            str(manifest["sizeBytes"]),
            arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE"),
        )
        self.assertEqual(
            manifest["schema"], arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA")
        )

    def test_the_replay_expectation_seals_the_same_digest(self) -> None:
        expected = load(EXPECTATION_PATH)["expectedOutput"]
        manifest = self.tree["contentManifest"]
        self.assertEqual(manifest["sha256"], expected["rootfsContentManifestSha256"])
        self.assertEqual(manifest["sizeBytes"], expected["rootfsContentManifestSizeBytes"])
        self.assertEqual(self.tree["layerSizeBytes"], expected["layerSizeBytes"])

    def test_the_manifest_is_derived_rather_than_tracked(self) -> None:
        manifest = self.tree["contentManifest"]
        self.assertFalse(manifest["isATrackedSourceRow"])
        self.assertEqual(manifest["mode"], "0444")
        self.assertEqual(manifest["hardLinkCountMustBe"], 1)
        paths = {row["logicalPath"] for row in self.plan["trackedFiles"]}
        self.assertNotIn(manifest["guestPath"], paths)
        self.assertTrue(manifest["whyItIsNotATrackedSourceRow"].strip())

    def test_the_nested_assembly_is_driven_by_the_runtime_lock(self) -> None:
        driver = self.tree["drivenBy"]
        path = REPO_ROOT / driver["path"]
        self.assertEqual(digest_of(path), driver["sha256"])
        self.assertEqual(path.stat().st_size, driver["sizeBytes"])
        self.assertIn("runtime-rootfs-source-lock", driver["path"])
        lock = load(path)
        self.assertEqual(driver["artifactCount"], len(lock["artifacts"]))
        self.assertEqual(driver["closureRootCount"], len(lock["closureRoots"]))
        self.assertTrue(driver["whyNotTheBootLock"].strip())

    def test_the_runtime_closure_is_contained_in_the_boot_closure(self) -> None:
        proof = self.tree["subsetProof"]
        self.assertTrue(proof["runtimeIsContainedInBoot"])
        self.assertEqual(proof["runtimeArtifactsAbsentFromBoot"], 0)
        source = REPO_ROOT / proof["source"]["path"]
        self.assertEqual(digest_of(source), proof["source"]["sha256"])
        sealed = load(source)["subsetProof"]
        self.assertTrue(sealed["runtimeIsContainedInBoot"])
        self.assertEqual(sealed["runtimeArtifactsAbsentFromBoot"], 0)
        self.assertEqual(sealed["runtimeArtifactCount"], self.tree["drivenBy"]["artifactCount"])
        self.assertEqual(
            sealed["runtimeClosureRootCount"], self.tree["drivenBy"]["closureRootCount"]
        )


class MaskingTests(Fixture):
    def test_nothing_is_mounted_over_the_prefix_in_the_audited_image(self) -> None:
        audit = load(MOUNT_POINTS_PATH)
        tops = {row["path"] for row in load(MOUNT_POINTS_PATH)["requiredRootDirectories"]}
        self.assertNotIn("var", tops)
        targets = [row["where"] for row in audit["audit"]["systemd"]["mountTable"]]
        targets += [row["where"] for row in audit["audit"]["mountUnits"]]
        prefix = self.plan["nestedTrees"][0]["guestPrefix"]
        for target in targets:
            self.assertFalse(
                prefix == target or prefix.startswith(target.rstrip("/") + "/"),
                f"{target} would mask the nested tree",
            )
        self.assertFalse(audit["audit"]["fstab"]["presentInImage"])

    def test_the_read_only_reasoning_is_pinned_to_that_audit(self) -> None:
        reasoning = self.plan["nestedTrees"][0]["whyTheReadOnlyCheckPasses"]
        self.assertTrue(reasoning["rootDiskReadOnly"])
        source = REPO_ROOT / reasoning["source"]["path"]
        self.assertEqual(digest_of(source), reasoning["source"]["sha256"])


class DiscrepancyTests(Fixture):
    def test_the_stale_clause_is_quoted_rather_than_edited(self) -> None:
        found = self.plan["discrepancyFound"]
        self.assertFalse(found["earlierRecordEdited"])
        self.assertFalse(found["wouldHaveBeenAHardStop"])
        path = REPO_ROOT / found["earlierRecord"]["path"]
        self.assertEqual(digest_of(path), found["earlierRecord"]["sha256"])

    def test_the_clause_the_record_disagrees_with_is_really_there(self) -> None:
        contract = load(GUEST_INIT_PATH)
        writable = {row["path"] for row in contract["filesystemLayout"]["writableMounts"]}
        self.assertIn("/var/lib/boole", writable)


class GapTests(Fixture):
    def test_all_three_gaps_are_addressed_with_the_closure_plan_wording(self) -> None:
        closure = load(CLOSURE_PLAN_PATH)
        original = {row["id"]: row for row in closure["gapsToClose"]["gaps"]}
        addressed = {row["id"]: row for row in self.plan["gapsAddressed"]}
        self.assertEqual(set(addressed), set(original))
        for identifier, row in addressed.items():
            self.assertEqual(row["what"], original[identifier]["what"])
            self.assertEqual(row["refusesAtStage"], original[identifier]["refusesAtStage"])
            self.assertEqual(row["kindOfChange"], original[identifier]["kindOfChange"])

    def test_no_gap_is_reported_closed_by_a_plan_that_built_nothing(self) -> None:
        for row in self.plan["gapsAddressed"]:
            self.assertEqual(row["closedByThisPlan"], "declared")
            self.assertTrue(row["stillRequires"])


class NamedFileTests(Fixture):
    """The generator, the two builders, and the staging table inside one of them.

    These are historical claims about where step one left things.  The current
    process policy keeps those exact recorded identities, but it does not require
    today's implementation files to remain byte-identical to that old starting
    point.  Current source behavior is covered by its current producer and
    preflight gates instead.
    """

    def test_the_three_historical_starting_files_keep_their_recorded_identities(self) -> None:
        named = self.plan["whichFilesThisPlanNames"]
        rows = [value for key, value in named.items() if isinstance(value, dict)]
        observed = {
            row["path"]: (row["sha256"], row["sizeBytes"])
            for row in rows
        }
        self.assertEqual(
            observed,
            {
                "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py": (
                    "a5dd54198878473c162ec306fbccd6edac8b22f036d9cf84d244b5f010f96d87",
                    37_435,
                ),
                "scripts/native_shadow_rootfs_builder.py": (
                    "aa25701a8a29cfb0059c911a5df8dcc2f09c8b4c61b4ff46adfc0ef446cdf689",
                    108_296,
                ),
                "scripts/native_shadow_boot_rootfs_source_lock_arm64_v1.py": (
                    "02cc8917c19a7f07810645cde70cf388e7a9ed7dd1b0814028fbcf9ae407577a",
                    25_470,
                ),
            },
        )

    def test_the_builder_staging_table_still_names_four(self) -> None:
        """The count the successor says step four has to change."""
        source = (REPO_ROOT / self.plan["whichFilesThisPlanNames"]["builderBoot"]["path"]).read_text(
            encoding="utf-8"
        )
        namespace: dict = {}
        marker = "BOOT_AUTHORITY_FILES"
        self.assertIn(marker, source)
        start = source.index(marker)
        end = source.index("\n}\n", start) + len("\n}\n")
        exec(compile(source[start:end], "staging-table", "exec"), namespace)
        table = namespace[marker]
        self.assertEqual(len(table), 4)

        staged_guest_paths = {guest for _, guest in table.values()}
        for row in self.plan["trackedFiles"]:
            if row["logicalPath"].startswith("/etc/") and row["logicalPath"] != "/etc/machine-id":
                self.assertNotIn(row["logicalPath"], staged_guest_paths, row["logicalPath"])

        staged_sources = {source for source, _ in table.values()}
        for row in self.plan["changesFromPredecessor"]["supersessions"]:
            self.assertIn(row["oldSourcePath"], staged_sources)
            self.assertNotIn(row["newSourcePath"], staged_sources)


class BudgetTests(Fixture):
    def test_both_budgets_are_bounds_rather_than_measurements(self) -> None:
        budgets = self.plan["theBudgets"]
        self.assertTrue(budgets["mustBeRemeasuredImmediatelyBeforeAssembly"])
        for name in ("bytes", "entries"):
            self.assertFalse(budgets[name]["isAMeasurementOfTheAssembledTree"], name)
            source = REPO_ROOT / budgets[name]["source"]["path"]
            self.assertEqual(digest_of(source), budgets[name]["source"]["sha256"], name)

    def test_each_bound_sits_under_its_limit(self) -> None:
        budgets = self.plan["theBudgets"]
        self.assertGreater(budgets["bytes"]["headroomBytes"], 0)
        self.assertLess(
            budgets["entries"]["boundedTotalEntries"], budgets["entries"]["maxEntries"]
        )


class LimitTests(Fixture):
    def test_it_says_plainly_what_it_does_not_establish(self) -> None:
        limits = self.plan["whatThisDoesNotEstablish"]
        self.assertGreaterEqual(len(limits), 7)
        joined = " ".join(limits)
        for needle in ("no lock was generated", "staging table", "serving is reachable"):
            self.assertIn(needle, joined)

    def test_the_boundaries_forbid_the_things_this_step_must_not_do(self) -> None:
        joined = " ".join(self.plan["boundaries"])
        for needle in (
            "no image was produced",
            "launcher seal is unmoved",
            "public mining",
            "wallet seed",
        ):
            self.assertIn(needle, joined)


if __name__ == "__main__":
    unittest.main()
