"""The MAC.3 guest runtime input set, checked against the tree it describes.

Two habits are enforced here, and they are the two that a later wave under time
pressure would be tempted to drop.

The first is that a record naming a file must agree with that file. Every digest
in the record is recomputed from the path it names, so a record that drifts from
the tree fails rather than reads well. That includes the record's own list of
files it promises to have left alone: those are checked byte-for-byte against
what is on disk, which is what makes "append-only" a property rather than a
claim.

The second is that an input set is not a result. These files change what a
future image would contain. They change nothing about the image that already
booted. So the record is required to say, in its own fields, that no image was
built and that nothing serves -- and the one gap these inputs cannot close is
required to still be recorded as open, in both this record and the contract it
is bound to.

The expected values below are written out literally rather than read back from
the record. A record that agrees with itself proves nothing.
"""

import hashlib
import json
import pathlib
import unittest

REPO = pathlib.Path(__file__).resolve().parents[1]
RECORD = REPO / "native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json"
CONTRACT = REPO / "native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json"

NODE = "boole-node"
CHECKER = "boole-native-checker"
NODE_ID = 990
CHECKER_ID = 991
REQUIRED_HOME = "/nonexistent"
ALLOWED_SHELLS = ("/usr/sbin/nologin", "/bin/false")

INPUT_PATHS = (
    "native/etc/passwd",
    "native/etc/group",
    "native/etc/shadow",
    "native/etc/gshadow",
    "native/etc/nsswitch.conf",
    "native/systemd/boole-native-shadow-launcher-v2.service",
    "native/tmpfiles.d/boole-native-shadow-v2.conf",
)

SUPERSEDED = {
    "native/systemd/boole-native-shadow-launcher-v2.service": (
        "native/systemd/boole-native-shadow-launcher.service"
    ),
    "native/tmpfiles.d/boole-native-shadow-v2.conf": (
        "native/tmpfiles.d/boole-native-shadow.conf"
    ),
}

UNCLOSED_GAP = "/var/lib/boole/native-shadow/runtime-rootfs"


def document():
    return json.loads(RECORD.read_text(encoding="utf-8"))


def contract():
    return json.loads(CONTRACT.read_text(encoding="utf-8"))


def digest(relative):
    return hashlib.sha256((REPO / relative).read_bytes()).hexdigest()


def inputs():
    return {row["path"]: row for row in document()["inputs"]}


def passwd():
    rows = {}
    for line in (REPO / "native/etc/passwd").read_text(encoding="utf-8").splitlines():
        if not line:
            continue
        fields = line.split(":")
        rows[fields[0]] = {
            "uid": int(fields[2]),
            "gid": int(fields[3]),
            "home": fields[5],
            "shell": fields[6],
        }
    return rows


def group():
    rows = {}
    for line in (REPO / "native/etc/group").read_text(encoding="utf-8").splitlines():
        if not line:
            continue
        name, _, gid, members = line.split(":")
        rows[name] = {"gid": int(gid), "members": [m for m in members.split(",") if m]}
    return rows


class TheRecordAgreesWithTheTreeTests(unittest.TestCase):
    """A record that names a file and its digest must be right about both."""

    def test_the_record_exists_and_parses(self) -> None:
        self.assertTrue(RECORD.exists(), RECORD)
        self.assertEqual(
            document()["schema"], "boole.native-shadow.mac3-guest-runtime-inputs.arm64.v1"
        )

    def test_every_input_named_is_a_file_that_exists(self) -> None:
        for path in INPUT_PATHS:
            with self.subTest(path=path):
                self.assertTrue((REPO / path).is_file(), path)

    def test_the_input_set_is_exactly_the_seven_files(self) -> None:
        self.assertEqual(sorted(inputs()), sorted(INPUT_PATHS))

    def test_every_input_digest_is_recomputed_rather_than_trusted(self) -> None:
        for path, row in inputs().items():
            with self.subTest(path=path):
                self.assertEqual(row["sha256"], digest(path))
                self.assertEqual(row["sizeBytes"], (REPO / path).stat().st_size)

    def test_every_file_promised_untouched_is_still_at_its_recorded_digest(self) -> None:
        rows = document()["appendOnly"]["recordsLeftByteUnchanged"]
        self.assertGreaterEqual(len(rows), 8)
        for row in rows:
            with self.subTest(path=row["path"]):
                self.assertEqual(row["sha256"], digest(row["path"]))
                self.assertEqual(row["sizeBytes"], (REPO / row["path"]).stat().st_size)

    def test_the_contract_it_is_bound_to_is_the_one_on_disk(self) -> None:
        bound = document()["boundContract"]
        self.assertEqual(bound["path"], str(CONTRACT.relative_to(REPO)))
        self.assertEqual(bound["sha256"], digest(bound["path"]))


class TheAccountDatabaseTests(unittest.TestCase):
    """Each clause of the identity contract, checked against the files themselves.

    The launcher refuses to serve unless all eight hold. Checking them here means
    a wrong number in the passwd file is caught in a second rather than in a boot.
    """

    def test_both_service_accounts_are_present(self) -> None:
        rows = passwd()
        self.assertIn(NODE, rows)
        self.assertIn(CHECKER, rows)

    def test_neither_account_is_root(self) -> None:
        for name in (NODE, CHECKER):
            with self.subTest(name=name):
                self.assertNotEqual(passwd()[name]["uid"], 0)
                self.assertNotEqual(passwd()[name]["gid"], 0)

    def test_both_homes_are_nonexistent(self) -> None:
        for name in (NODE, CHECKER):
            with self.subTest(name=name):
                self.assertEqual(passwd()[name]["home"], REQUIRED_HOME)

    def test_both_shells_are_ones_the_contract_allows(self) -> None:
        self.assertEqual(passwd()[NODE]["shell"], "/usr/sbin/nologin")
        self.assertEqual(passwd()[CHECKER]["shell"], "/bin/false")
        for name in (NODE, CHECKER):
            with self.subTest(name=name):
                self.assertIn(passwd()[name]["shell"], ALLOWED_SHELLS)

    def test_each_account_has_a_same_named_group_at_its_primary_id(self) -> None:
        for name in (NODE, CHECKER):
            with self.subTest(name=name):
                self.assertIn(name, group())
                self.assertEqual(group()[name]["gid"], passwd()[name]["gid"])

    def test_each_primary_group_id_resolves_back_to_exactly_one_group(self) -> None:
        gids = [row["gid"] for row in group().values()]
        for name in (NODE, CHECKER):
            with self.subTest(name=name):
                gid = passwd()[name]["gid"]
                self.assertEqual(gids.count(gid), 1)

    def test_no_group_lists_either_account_as_a_member(self) -> None:
        # The contract asks that the account's full group list be exactly its
        # own primary group. A supplementary membership anywhere in this file
        # would break that, so every member list is required to be empty.
        for name, row in group().items():
            with self.subTest(group=name):
                self.assertEqual(row["members"], [])

    def test_the_two_accounts_share_neither_number(self) -> None:
        self.assertNotEqual(passwd()[NODE]["uid"], passwd()[CHECKER]["uid"])
        self.assertNotEqual(passwd()[NODE]["gid"], passwd()[CHECKER]["gid"])

    def test_the_numbers_are_the_ones_the_record_publishes(self) -> None:
        accounts = document()["accounts"]
        self.assertEqual(accounts[NODE]["uid"], NODE_ID)
        self.assertEqual(accounts[NODE]["gid"], NODE_ID)
        self.assertEqual(accounts[CHECKER]["uid"], CHECKER_ID)
        self.assertEqual(accounts[CHECKER]["gid"], CHECKER_ID)
        for name in (NODE, CHECKER):
            with self.subTest(name=name):
                self.assertEqual(accounts[name]["uid"], passwd()[name]["uid"])
                self.assertEqual(accounts[name]["gid"], passwd()[name]["gid"])

    def test_the_shadow_files_lock_every_entry_and_name_the_same_accounts(self) -> None:
        shadow = {
            line.split(":")[0]: line.split(":")[1]
            for line in (REPO / "native/etc/shadow").read_text(encoding="utf-8").splitlines()
            if line
        }
        self.assertEqual(sorted(shadow), sorted(passwd()))
        for name, secret in shadow.items():
            with self.subTest(name=name):
                self.assertEqual(secret, "*")

    def test_the_group_shadow_file_names_the_same_groups_with_no_members(self) -> None:
        rows = [
            line.split(":")
            for line in (REPO / "native/etc/gshadow").read_text(encoding="utf-8").splitlines()
            if line
        ]
        self.assertEqual(sorted(row[0] for row in rows), sorted(group()))
        for row in rows:
            with self.subTest(group=row[0]):
                self.assertEqual(row[1], "*")
                self.assertEqual(row[3], "")


class TheLookupOrderTests(unittest.TestCase):
    """Files, and only files. The alternatives all want something absent."""

    def test_every_database_resolves_from_files(self) -> None:
        for line in (REPO / "native/etc/nsswitch.conf").read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            with self.subTest(line=line):
                self.assertEqual(line.split(":", 1)[1].split(), ["files"])

    def test_passwd_and_group_are_both_named(self) -> None:
        text = (REPO / "native/etc/nsswitch.conf").read_text(encoding="utf-8")
        self.assertIn("passwd:", text)
        self.assertIn("group:", text)

    def test_no_entry_names_the_systemd_module_or_dns(self) -> None:
        # The image ships no libnss-systemd, and the guest has no network
        # device. Either would name a resolver that cannot answer.
        text = (REPO / "native/etc/nsswitch.conf").read_text(encoding="utf-8")
        self.assertNotIn("systemd", text)
        self.assertNotIn("dns", text)


class TheSuccessorUnitTests(unittest.TestCase):
    """The launcher unit changes by exactly two lines, and they are output lines."""

    def test_the_successor_differs_from_v1_in_output_destinations_only(self) -> None:
        old = (REPO / SUPERSEDED[
            "native/systemd/boole-native-shadow-launcher-v2.service"
        ]).read_text(encoding="utf-8").splitlines()
        new = (
            REPO / "native/systemd/boole-native-shadow-launcher-v2.service"
        ).read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(old), len(new))
        differing = [(a, b) for a, b in zip(old, new) if a != b]
        self.assertEqual(
            differing,
            [
                ("StandardOutput=journal", "StandardOutput=journal+console"),
                ("StandardError=journal", "StandardError=journal+console"),
            ],
        )

    def test_the_console_is_added_and_the_journal_is_kept(self) -> None:
        text = (
            REPO / "native/systemd/boole-native-shadow-launcher-v2.service"
        ).read_text(encoding="utf-8")
        self.assertIn("StandardOutput=journal+console", text)
        self.assertIn("StandardError=journal+console", text)

    def test_the_privilege_lines_are_untouched(self) -> None:
        # If observability were bought by widening what the launcher may hold,
        # that would be a different change wearing this one's name.
        text = (
            REPO / "native/systemd/boole-native-shadow-launcher-v2.service"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN", text
        )
        self.assertIn("AmbientCapabilities=\n", text)
        self.assertIn("NoNewPrivileges=no", text)
        self.assertIn("User=root", text)

    def test_it_is_staged_to_the_same_guest_path_as_v1(self) -> None:
        row = inputs()["native/systemd/boole-native-shadow-launcher-v2.service"]
        self.assertEqual(
            row["guestPath"], "/usr/lib/systemd/system/boole-native-shadow-launcher.service"
        )


class TheSuccessorRuntimeRulesTests(unittest.TestCase):
    """Every remaining rule writes to tmpfs, which is the point of the change."""

    def test_only_run_boole_paths_remain(self) -> None:
        for line in (
            REPO / "native/tmpfiles.d/boole-native-shadow-v2.conf"
        ).read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            with self.subTest(line=line):
                self.assertTrue(line.split()[1].startswith("/run/boole"), line)

    def test_nothing_under_var_lib_boole_is_asked_for(self) -> None:
        text = (
            REPO / "native/tmpfiles.d/boole-native-shadow-v2.conf"
        ).read_text(encoding="utf-8")
        self.assertNotIn("/var/lib/boole", text)

    def test_the_runtime_directory_keeps_its_mode_and_owning_group(self) -> None:
        # 2750 with the node group is what lets the launcher hand the socket to
        # the account it drops to without opening it to anyone else.
        text = (
            REPO / "native/tmpfiles.d/boole-native-shadow-v2.conf"
        ).read_text(encoding="utf-8")
        self.assertIn("d /run/boole/native-shadow 2750 root boole-node -", text)

    def test_the_three_dropped_rules_were_the_ones_that_could_not_succeed(self) -> None:
        old = (REPO / SUPERSEDED[
            "native/tmpfiles.d/boole-native-shadow-v2.conf"
        ]).read_text(encoding="utf-8").splitlines()
        new = (
            REPO / "native/tmpfiles.d/boole-native-shadow-v2.conf"
        ).read_text(encoding="utf-8").splitlines()
        dropped = [line for line in old if line and line not in new]
        self.assertEqual(len(dropped), 3)
        for line in dropped:
            with self.subTest(line=line):
                self.assertTrue(line.split()[1].startswith("/var/lib/boole"), line)

    def test_it_is_staged_to_the_same_guest_path_as_v1(self) -> None:
        row = inputs()["native/tmpfiles.d/boole-native-shadow-v2.conf"]
        self.assertEqual(row["guestPath"], "/usr/lib/tmpfiles.d/boole-native-shadow.conf")


class NotAResultTests(unittest.TestCase):
    """Inputs exist; an image does not. The record has to say both."""

    def test_no_image_is_claimed(self) -> None:
        self.assertIs(document()["imageProduced"], False)

    def test_no_serving_is_claimed(self) -> None:
        self.assertIs(document()["servingClaim"], False)

    def test_activation_stays_disallowed(self) -> None:
        self.assertIs(document()["activationAllowed"], False)

    def test_the_status_says_frozen_and_not_built(self) -> None:
        self.assertEqual(document()["status"], "MAC3-GUEST-RUNTIME-INPUTS-FROZEN-NOT-BUILT")

    def test_it_carries_no_verdict_field(self) -> None:
        for absent in ("verdict", "passed", "conditionsMet", "result"):
            with self.subTest(field=absent):
                self.assertNotIn(absent, document())


class TheGapThatStaysOpenTests(unittest.TestCase):
    """The runtime rootfs is not an input file, and is not quietly counted."""

    def test_it_is_named_as_not_closed(self) -> None:
        unclosed = [row["gap"] for row in document()["doesNotClose"]]
        self.assertIn(UNCLOSED_GAP, unclosed)

    def test_no_input_claims_to_close_it(self) -> None:
        for path, row in inputs().items():
            with self.subTest(path=path):
                self.assertNotEqual(row.get("closesGap"), UNCLOSED_GAP)

    def test_the_contract_still_records_it_as_a_gap(self) -> None:
        gaps = [row["path"] for row in contract()["gaps"]]
        self.assertIn(UNCLOSED_GAP, gaps)

    def test_the_contract_still_records_the_image_as_not_made(self) -> None:
        # These inputs do not produce anything. If a later change makes one, it
        # has to move this flag deliberately rather than as a side effect.
        self.assertIs(contract()["newImageProduction"]["performed"], False)
        self.assertIs(contract()["servingClaim"], False)

    def test_the_consequence_of_the_open_gap_matches_what_the_contract_sealed(self) -> None:
        sealed = {row["path"]: row["consequence"] for row in contract()["gaps"]}
        for row in document()["doesNotClose"]:
            if row["gap"] in sealed:
                with self.subTest(gap=row["gap"]):
                    self.assertIn("verify_runtime_rootfs_replay", row["consequence"])
                    self.assertIn("verify_runtime_rootfs_replay", sealed[row["gap"]])


class SupersededFilesSurviveTests(unittest.TestCase):
    """The v1 inputs stay in the tree, because sealed records still name them."""

    def test_each_superseded_file_still_exists_unchanged(self) -> None:
        unchanged = {
            row["path"]: row for row in document()["appendOnly"]["recordsLeftByteUnchanged"]
        }
        for successor, original in SUPERSEDED.items():
            with self.subTest(path=original):
                self.assertTrue((REPO / original).is_file(), original)
                self.assertIn(original, unchanged)
                self.assertEqual(unchanged[original]["sha256"], digest(original))

    def test_the_successor_names_what_it_supersedes(self) -> None:
        for successor, original in SUPERSEDED.items():
            with self.subTest(path=successor):
                self.assertEqual(inputs()[successor]["supersedes"], original)

    def test_the_sysusers_file_is_left_alone(self) -> None:
        # It is pinned by four sealed records. The baked account database makes
        # it a no-op at boot rather than a thing to edit.
        self.assertIn(
            "native/sysusers.d/boole-native-shadow.conf",
            {row["path"] for row in document()["appendOnly"]["recordsLeftByteUnchanged"]},
        )


class ThingsStatedRatherThanDiscoveredTests(unittest.TestCase):
    """Awkward facts are recorded in the record, not left for a later reader."""

    def test_the_absent_shells_are_stated(self) -> None:
        facts = " ".join(row["fact"] + " " + row["why"] for row in document()["statedPlainly"])
        self.assertIn("nologin", facts)
        self.assertIn("absent", facts)

    def test_the_deviation_from_the_packaged_default_is_stated(self) -> None:
        facts = [row["fact"] for row in document()["statedPlainly"]]
        self.assertTrue(any("base-passwd" in fact for fact in facts), facts)

    def test_the_boundaries_still_name_what_is_not_being_done(self) -> None:
        joined = " ".join(document()["boundaries"])
        for phrase in ("wallet", "API key", "network device", "public mining"):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, joined)


if __name__ == "__main__":
    unittest.main()
