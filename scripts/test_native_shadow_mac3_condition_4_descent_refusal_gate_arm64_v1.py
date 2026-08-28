"""The descent half of corrected condition 4, pinned as a source contract.

Two clauses of the corrected fourth condition say that the submitted answer and
its checker start only after the launcher has descended to the sealed
unprivileged identity, and that a failed descent, a root-state execution or a
chance of regaining privilege is refused. Both clauses named one source file and
nothing else: the function they point at has a single call site and no test.

It cannot be given a behavioural test without editing launcher source, and the
launcher build seal freezes that source -- the produced ELF digest is sealed, the
sealed result pins the build authority, and the build authority pins the digest
of every launcher source file. So the contract is fixed here instead, at the
source level, by reading the sealed file and requiring every refusal to be
present, to be reached by the decision that makes it a refusal, to sit before the
exec, and to have no execution path around it.

A gate that only reads for text can rot into a gate that reads for nothing, so
each condition is a function over source text and this file carries a table of
weakened variants of the sealed source. Every variant deletes one condition or
inverts one order, and every variant must be caught by the condition it weakens.
The variants live in memory only; the sealed file is never written.

This is a static source contract, not a behavioural test. It fails when a refusal
is deleted, weakened, reordered or bypassed. It cannot say how the parser answers
an adversarial kernel status; that stays NOT-MEASURED and is recorded as such.
"""

import hashlib
import json
import pathlib
import unittest

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
RECORD_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json"
)
PREDECESSOR_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json"
)
CONTAINMENT_PATH = (
    REPOSITORY_ROOT
    / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
)


def digest_of(path):
    payload = path.read_bytes()
    return hashlib.sha256(payload).hexdigest(), len(payload)


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


class Violated(Exception):
    """One condition of the source contract does not hold."""


def function_body(source, name):
    """The text of one free function, from its signature to its closing brace.

    The name may be followed by a lifetime parameter rather than the argument
    list, so the signature is matched up to the name and no further.
    """
    marker = f"\nfn {name}"
    start = source.find(marker)
    if start == -1:
        raise Violated(f"{name} is gone")
    start += 1
    if source[start + len(marker) - 1] not in "(<":
        raise Violated(f"{name} is not a free function here")
    end = source.find("\n}\n", start)
    if end == -1:
        raise Violated(f"{name} does not close")
    return source[start:end]


def production_region(source):
    """Everything above the test module, which is the only shipped code."""
    boundary = source.find("\n#[cfg(test)]\nmod tests {")
    if boundary == -1:
        raise Violated("the test module boundary is gone")
    return source[:boundary]


def require(condition, message):
    if not condition:
        raise Violated(message)


def require_ordered(body, decision, message, label):
    """A refusal counts only when its deciding test comes before its message."""
    at_decision = body.find(decision)
    require(at_decision != -1, f"{label}: the decision is gone")
    at_message = body.find(message)
    require(at_message != -1, f"{label}: the refusal message is gone")
    require(at_decision < at_message, f"{label}: the refusal precedes its decision")


CONDITIONS = {}


def condition(identifier):
    def register(check):
        CONDITIONS[identifier] = check
        return check

    return register


@condition("sealed-source-digest-unchanged")
def _sealed_source_digest_unchanged(source, record):
    payload = source.encode("utf-8")
    digest = hashlib.sha256(payload).hexdigest()
    pinned = record["theGate"]["reads"]
    require(digest == pinned["sha256"], "the launcher source is not the sealed source")
    require(len(payload) == pinned["sizeBytes"], "the launcher source changed size")


@condition("uid-zero-refusal")
def _uid_zero_refusal(source, record):
    body = function_body(source, "verify_dropped_privileges")
    require(
        'require_status_ids(&status, "Uid", uid)?' in body,
        "the kernel-reported Uid is no longer checked",
    )
    require_ordered(
        function_body(source, "require_status_ids"),
        "ids != [expected; 4]",
        "identity mismatch",
        "uid",
    )


@condition("gid-zero-refusal")
def _gid_zero_refusal(source, record):
    body = function_body(source, "verify_dropped_privileges")
    require(
        'require_status_ids(&status, "Gid", gid)?' in body,
        "the kernel-reported Gid is no longer checked",
    )
    require_ordered(
        function_body(source, "require_status_ids"),
        "ids != [expected; 4]",
        "identity mismatch",
        "gid",
    )


@condition("syscall-identity-refusal")
def _syscall_identity_refusal(source, record):
    body = function_body(source, "verify_dropped_privileges")
    for slot in ("[uid; 3]", "[gid; 3]"):
        require(slot in body, f"the real/effective/saved comparison lost {slot}")
    require_ordered(
        body,
        "[uid; 3]",
        "checker real/effective/saved identity mismatch",
        "syscall identity",
    )


@condition("supplementary-groups-refusal")
def _supplementary_groups_refusal(source, record):
    require_ordered(
        function_body(source, "verify_dropped_privileges"),
        "libc::getgroups(0, std::ptr::null_mut())",
        "checker retained supplementary groups",
        "supplementary groups",
    )


@condition("capability-sets-refusal")
def _capability_sets_refusal(source, record):
    body = function_body(source, "verify_dropped_privileges")
    required = record["theGate"]["capabilitySetsRequiredEmpty"]
    require(len(required) == 5, "five capability sets are the whole set")
    for field in required:
        require(f'"{field}"' in body, f"{field} is no longer required to be empty")
    require_ordered(
        body,
        "u64::from_str_radix(value, 16)",
        "is not exact empty",
        "capability sets",
    )


@condition("no-new-privs-refusal")
def _no_new_privs_refusal(source, record):
    require_ordered(
        function_body(source, "verify_dropped_privileges"),
        'require_status_field(&status, "NoNewPrivs")? != "1"',
        "checker no_new_privs was not enabled",
        "no_new_privs",
    )


@condition("absent-or-unreadable-field-refuses")
def _absent_or_unreadable_field_refuses(source, record):
    field = function_body(source, "require_status_field")
    require(
        'ok_or_else(|| format!("checker status lacks {field}"))?' in field,
        "an absent status field no longer refuses",
    )
    require_ordered(
        field,
        "values.next().is_some() || value.is_empty()",
        "invalid {field} cardinality",
        "cardinality",
    )
    ids = function_body(source, "require_status_ids")
    require(
        'map_err(|_| format!("checker {field} is malformed"))?' in ids,
        "an unparsable identity value no longer refuses",
    )
    body = function_body(source, "verify_dropped_privileges")
    for guard in (
        "value.len() != 16",
        "is_ascii_hexdigit",
        'map_err(|_| "malformed capability set")?',
    ):
        require(guard in body, f"an unreadable capability set no longer refuses: {guard}")


@condition("every-pinned-refusal-present-and-ordered")
def _every_pinned_refusal_present_and_ordered(source, record):
    for refusal in record["refusalsPinned"]:
        require_ordered(
            function_body(source, refusal["inFunction"]),
            refusal["decision"],
            refusal["message"],
            refusal["id"],
        )


@condition("refusal-count-exact")
def _refusal_count_exact(source, record):
    """A new escape hatch, or a deleted refusal, changes one of these counts."""
    for name, expected in record["theGate"]["refusalCounts"].items():
        found = function_body(source, name).count("return Err(")
        require(found == expected, f"{name} has {found} refusals, not {expected}")


@condition("single-success-path")
def _single_success_path(source, record):
    body = function_body(source, "verify_dropped_privileges")
    found = body.count("Ok(())")
    require(found == 1, f"the descent verification has {found} success paths, not one")


@condition("verification-precedes-execution")
def _verification_precedes_execution(source, record):
    order = record["orderPinned"]
    region = production_region(source)
    positions = []
    for stage in order["stages"]:
        at = region.find(f'"{stage}"')
        require(at != -1, f"the {stage} stage is gone")
        require(
            "setup_stage" in region[max(0, at - 200) : at],
            f"the {stage} stage is no longer run as a setup stage",
        )
        require("?" in region[at : at + 400], f"a failed {stage} no longer propagates")
        positions.append(at)
    require(positions == sorted(positions), "the setup stages are out of order")
    at_exec = region.find(order["execCall"])
    require(at_exec != -1, "the exec call is gone")
    require(positions[-1] < at_exec, "a setup stage now runs after the exec")


@condition("single-answer-execution-path")
def _single_answer_execution_path(source, record):
    path = record["singleAnswerExecutionPath"]
    region = production_region(source)
    for call, expected in (
        (path["execCall"], path["execCallOccurrences"]),
        (path["childCreationCall"], path["childCreationCallOccurrences"]),
    ):
        found = region.count(call)
        require(found == expected, f"{call} occurs {found} times, not {expected}")
    for forbidden in path["forbiddenSpawnApis"]:
        require(forbidden not in region, f"a second execution path appeared: {forbidden}")


def violations(source, record):
    """Every condition this source text fails, by identifier."""
    failed = {}
    for identifier, check in CONDITIONS.items():
        try:
            check(source, record)
        except Violated as error:
            failed[identifier] = str(error)
        except (ValueError, KeyError, IndexError) as error:
            failed[identifier] = f"{type(error).__name__}: {error}"
    return failed


class Weakening:
    """One deliberately weakened variant of the sealed source, held in memory."""

    def __init__(self, identifier, condition, weakens, edits):
        self.identifier = identifier
        self.condition = condition
        self.weakens = weakens
        self.edits = edits

    def apply(self, source):
        weakened = source
        for old, new in self.edits:
            if weakened.count(old) != 1:
                raise AssertionError(f"{self.identifier}: anchor is not unique: {old!r}")
            weakened = weakened.replace(old, new)
        return weakened


SYSCALL_IDENTITY_REFUSAL = """    if [real_uid, effective_uid, saved_uid] != [uid; 3]
        || [real_gid, effective_gid, saved_gid] != [gid; 3]
    {
        return Err("checker real/effective/saved identity mismatch".to_string());
    }
"""

SUPPLEMENTARY_GROUP_REFUSAL = """    if unsafe { libc::getgroups(0, std::ptr::null_mut()) } != 0 {
        return Err("checker retained supplementary groups".to_string());
    }
"""

NO_NEW_PRIVS_REFUSAL = """    if require_status_field(&status, "NoNewPrivs")? != "1" {
        return Err("checker no_new_privs was not enabled".to_string());
    }
"""

CARDINALITY_REFUSAL = """    if values.next().is_some() || value.is_empty() {
        return Err(format!("checker status has invalid {field} cardinality"));
    }
"""

IDENTITY_REFUSAL = """    if ids != [expected; 4] {
        return Err(format!("checker {field} identity mismatch"));
    }
"""

VERIFY_STAGE = """    setup_stage(
        "verify-privileges",
        verify_dropped_privileges(setup.checker_uid, setup.checker_gid),
    )?;
"""

EXEC_LINE = (
    "    unsafe { libc::execve(CHECKER_PATH.as_ptr(), "
    "argv_ptrs.as_ptr(), env_ptrs.as_ptr()) };\n"
)

WEAKENINGS = (
    Weakening(
        "delete-the-uid-identity-check",
        "uid-zero-refusal",
        "stops asking the kernel whether every Uid slot is the sealed account",
        ((' require_status_ids(&status, "Uid", uid)?;\n', "\n"),),
    ),
    Weakening(
        "delete-the-gid-identity-check",
        "gid-zero-refusal",
        "stops asking the kernel whether every Gid slot is the sealed group",
        ((' require_status_ids(&status, "Gid", gid)?;\n', "\n"),),
    ),
    Weakening(
        "delete-the-syscall-identity-refusal",
        "syscall-identity-refusal",
        "accepts a root real, effective or saved identity",
        ((SYSCALL_IDENTITY_REFUSAL, ""),),
    ),
    Weakening(
        "delete-the-supplementary-group-refusal",
        "supplementary-groups-refusal",
        "accepts a checker that kept its supplementary groups",
        ((SUPPLEMENTARY_GROUP_REFUSAL, ""),),
    ),
    Weakening(
        "drop-one-capability-set-from-the-required-list",
        "capability-sets-refusal",
        "stops requiring the bounding set to be empty",
        (
            (
                '["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"]',
                '["CapInh", "CapPrm", "CapEff", "CapAmb"]',
            ),
        ),
    ),
    Weakening(
        "tolerate-a-non-empty-capability-set",
        "capability-sets-refusal",
        "reads the capability sets and then accepts them whatever they hold",
        (
            (
                '            return Err(format!("checker {field} is not exact empty"));\n',
                "            let _retained = value;\n",
            ),
        ),
    ),
    Weakening(
        "delete-the-no-new-privs-refusal",
        "no-new-privs-refusal",
        "accepts a checker that can still regain privilege through exec",
        ((NO_NEW_PRIVS_REFUSAL, ""),),
    ),
    Weakening(
        "tolerate-a-missing-status-field",
        "absent-or-unreadable-field-refuses",
        "turns an unreadable field into an empty string instead of a refusal",
        (
            (
                '        .ok_or_else(|| format!("checker status lacks {field}"))?;',
                '        .unwrap_or("");',
            ),
        ),
    ),
    Weakening(
        "tolerate-a-duplicated-or-empty-status-field",
        "absent-or-unreadable-field-refuses",
        "accepts the first of two conflicting kernel answers",
        ((CARDINALITY_REFUSAL, ""),),
    ),
    Weakening(
        "tolerate-a-malformed-identity-value",
        "absent-or-unreadable-field-refuses",
        "turns an unparsable identity list into a default instead of a refusal",
        (
            (
                '        .map_err(|_| format!("checker {field} is malformed"))?;',
                "        .unwrap_or_default();",
            ),
        ),
    ),
    Weakening(
        "invert-the-drop-and-verify-stage-order",
        "verification-precedes-execution",
        "verifies the descent before performing it",
        (
            ('        "drop-privileges",\n', '        "@swap@",\n'),
            ('        "verify-privileges",\n', '        "drop-privileges",\n'),
            ('        "@swap@",\n', '        "verify-privileges",\n'),
        ),
    ),
    Weakening(
        "move-the-verification-after-the-exec",
        "verification-precedes-execution",
        "runs the answer first and checks the descent afterwards",
        ((VERIFY_STAGE, ""), (EXEC_LINE, EXEC_LINE + VERIFY_STAGE)),
    ),
    Weakening(
        "add-a-second-answer-execution-path",
        "single-answer-execution-path",
        "adds a second exec that no descent verification guards",
        (
            (
                "    env_ptrs.push(std::ptr::null());\n",
                "    env_ptrs.push(std::ptr::null());\n"
                "    unsafe { libc::execve(FALLBACK_PATH.as_ptr(), "
                "argv_ptrs.as_ptr(), env_ptrs.as_ptr()) };\n",
            ),
        ),
    ),
    Weakening(
        "add-a-forbidden-process-spawn",
        "single-answer-execution-path",
        "reaches the answer through a shell instead of the contained exec",
        (
            (
                "    let mut env_ptrs = env.iter()",
                '    let _ = std::process::Command::new("/bin/sh").status();\n'
                "    let mut env_ptrs = env.iter()",
            ),
        ),
    ),
    Weakening(
        "add-an-early-success-return",
        "single-success-path",
        "returns success before any of the checks run",
        (
            (
                "fn verify_dropped_privileges(uid: u32, gid: u32) -> Result<(), String> {\n",
                "fn verify_dropped_privileges(uid: u32, gid: u32) -> Result<(), String> {\n"
                "    if uid == gid {\n        return Ok(());\n    }\n",
            ),
        ),
    ),
    Weakening(
        "invert-a-refusal-and-its-decision",
        "every-pinned-refusal-present-and-ordered",
        "keeps the refusal text while moving the decision behind it",
        (
            (
                IDENTITY_REFUSAL,
                '    let refusal = format!("checker {field} identity mismatch");\n'
                "    if ids != [expected; 4] {\n"
                "        return Err(refusal);\n"
                "    }\n",
            ),
        ),
    ),
    Weakening(
        "add-an-unaccounted-refusal",
        "refusal-count-exact",
        "adds a refusal the record does not describe",
        (
            (
                "    let mut real_uid = 0;\n",
                "    let mut real_uid = 0;\n"
                "    if real_uid == 1 {\n"
                '        return Err("unaccounted".to_string());\n'
                "    }\n",
            ),
        ),
    ),
)


class SealedSourceTests(unittest.TestCase):
    """The sealed source satisfies every condition of the contract."""

    def setUp(self):
        self.record = load(RECORD_PATH)
        self.source = CONTAINMENT_PATH.read_text(encoding="utf-8")

    def test_the_text_read_is_the_bytes_the_seal_covers(self):
        self.assertEqual(self.source.encode("utf-8"), CONTAINMENT_PATH.read_bytes())

    def test_the_sealed_source_violates_no_condition(self):
        self.assertEqual(violations(self.source, self.record), {})

    def test_the_record_names_exactly_the_conditions_this_gate_enforces(self):
        named = [row["id"] for row in self.record["conditions"]]
        self.assertEqual(sorted(named), sorted(CONDITIONS))
        self.assertEqual(len(named), len(set(named)))
        for row in self.record["conditions"]:
            self.assertTrue(row["requires"].strip(), row["id"])


class WeakeningTests(unittest.TestCase):
    """Each weakened variant is caught by the condition it weakens."""

    def setUp(self):
        self.record = load(RECORD_PATH)
        self.source = CONTAINMENT_PATH.read_text(encoding="utf-8")

    def test_every_weakening_is_caught_by_the_condition_it_weakens(self):
        for weakening in WEAKENINGS:
            with self.subTest(weakening.identifier):
                weakened = weakening.apply(self.source)
                self.assertNotEqual(weakened, self.source)
                failed = violations(weakened, self.record)
                self.assertIn(weakening.condition, failed, weakening.identifier)

    def test_every_condition_has_a_weakening_that_exercises_it(self):
        exercised = {weakening.condition for weakening in WEAKENINGS}
        exercised.add("sealed-source-digest-unchanged")
        self.assertEqual(exercised, set(CONDITIONS))

    def test_the_sealed_file_was_never_written(self):
        """The variants exist in memory; the seal must survive this suite."""
        sha256, size = digest_of(CONTAINMENT_PATH)
        pinned = self.record["theGate"]["reads"]
        self.assertEqual(sha256, pinned["sha256"])
        self.assertEqual(size, pinned["sizeBytes"])

    def test_the_record_names_exactly_the_weakenings_this_gate_carries(self):
        named = self.record["weakeningFixtures"]
        self.assertEqual(
            sorted(row["id"] for row in named),
            sorted(weakening.identifier for weakening in WEAKENINGS),
        )
        by_id = {weakening.identifier: weakening for weakening in WEAKENINGS}
        for row in named:
            self.assertEqual(row["condition"], by_id[row["id"]].condition, row["id"])
            self.assertEqual(row["weakens"], by_id[row["id"]].weakens, row["id"])
        self.assertIs(self.record["theGate"]["fixturesAreInTheGateScript"], True)


class RecordShapeTests(unittest.TestCase):
    def setUp(self):
        self.record = load(RECORD_PATH)

    def test_the_record_is_the_arm64_descent_refusal_gate_schema(self):
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3-condition-4-descent-refusal-gate.arm64.v1",
        )
        self.assertEqual(self.record["status"], "STATIC-SOURCE-CONTRACT-GREEN")
        self.assertEqual(self.record["unitLevelDropFailureMatrix"], "NOT-MEASURED")

    def test_the_record_claims_no_outcome(self):
        forbidden = {"verdict", "passed", "servingReached", "booted"}
        seen = set()

        def walk(node):
            if isinstance(node, dict):
                for key, value in node.items():
                    seen.add(key)
                    walk(value)
            elif isinstance(node, list):
                for item in node:
                    walk(item)

        walk(self.record)
        self.assertEqual(forbidden & seen, set())
        self.assertIs(self.record["activationAllowed"], False)
        self.assertIs(self.record["servingClaim"], False)
        self.assertIs(self.record["servingGapsClosed"], False)
        self.assertEqual(self.record["servingGapsRemaining"], 3)

    def test_nothing_was_built_or_booted(self):
        nothing = self.record["nothingWasBuiltOrBooted"]
        for field in (
            "imageProduced",
            "productionDispatched",
            "bootPerformed",
            "builderChanged",
            "unitFileChanged",
            "launcherSourceChanged",
            "launcherResealed",
        ):
            self.assertIs(nothing[field], False, field)


class AppendOnlyTests(unittest.TestCase):
    """The predecessor is succeeded, never edited, and its stamps still hold."""

    def setUp(self):
        self.section = load(RECORD_PATH)["appendOnly"]

    def test_the_predecessor_is_stamped_exactly_as_the_tree_holds_it(self):
        self.assertIs(self.section["predecessorEdited"], False)
        stamp = self.section["predecessor"]
        sha256, size = digest_of(PREDECESSOR_PATH)
        self.assertEqual(stamp["path"], "native/containment/" + PREDECESSOR_PATH.name)
        self.assertEqual(stamp["sha256"], sha256)
        self.assertEqual(stamp["sizeBytes"], size)

    def test_no_stamp_of_the_predecessor_went_stale(self):
        """This record adds enforcement; it must not have moved what was pinned."""
        self.assertIs(self.section["predecessorStampsStillMatchTheTree"], True)
        predecessor = load(PREDECESSOR_PATH)
        checked = 0
        for clause in predecessor["clauseEnforcement"]:
            for stamp in clause["enforcedBy"]:
                sha256, size = digest_of(REPOSITORY_ROOT / stamp["path"])
                self.assertEqual(stamp["sha256"], sha256, stamp["path"])
                self.assertEqual(stamp["sizeBytes"], size, stamp["path"])
                checked += 1
        self.assertGreater(checked, 0)

    def test_the_two_clauses_this_record_serves_are_named(self):
        served = self.section["clausesGivenEnforcement"]
        predecessor = load(PREDECESSOR_PATH)
        known = {clause["clauseId"] for clause in predecessor["clauseEnforcement"]}
        self.assertTrue(set(served) <= known, served)
        self.assertEqual(
            sorted(served),
            [
                "descent-failure-root-execution-or-reacquisition-refuses",
                "submissions-and-checker-start-only-after-descent",
            ],
        )


class WhatWasMissingTests(unittest.TestCase):
    """The gap is stated as it was found, not as it was convenient to find."""

    def setUp(self):
        self.section = load(RECORD_PATH)["whatWasMissing"]
        self.source = CONTAINMENT_PATH.read_text(encoding="utf-8")

    def test_the_named_function_has_the_recorded_number_of_call_sites(self):
        name = self.section["function"]
        calls = self.source.count(f"{name}(") - self.source.count(f"fn {name}(")
        self.assertEqual(self.section["callSites"], calls)
        self.assertEqual(self.section["unitTests"], 0)

    def test_the_module_is_gated_to_linux_which_is_why_it_was_untestable(self):
        gate = self.section["whyItCouldNotBeTestedOnTheDevelopmentMachine"]
        path = REPOSITORY_ROOT / gate["path"]
        sha256, size = digest_of(path)
        self.assertEqual(gate["sha256"], sha256)
        self.assertEqual(gate["sizeBytes"], size)
        self.assertIn(gate["moduleGate"], path.read_text(encoding="utf-8"))
        self.assertIn('cfg(target_os = "linux")', gate["moduleGate"])


class TheLauncherSealTests(unittest.TestCase):
    """Why the stronger test was deferred: every link of the seal is stamped."""

    def setUp(self):
        self.section = load(RECORD_PATH)["whyBehaviouralTestsWereNotAdded"]

    def test_every_link_of_the_seal_chain_matches_the_tree(self):
        for key in ("buildResult", "buildAuthority", "producerAuthority"):
            stamp = self.section["sealChain"][key]
            sha256, size = digest_of(REPOSITORY_ROOT / stamp["path"])
            self.assertEqual(stamp["sha256"], sha256, key)
            self.assertEqual(stamp["sizeBytes"], size, key)

    def test_the_sealed_launcher_digest_is_the_one_the_producer_rebuilds_against(self):
        chain = self.section["sealChain"]
        result = load(REPOSITORY_ROOT / chain["buildResult"]["path"])
        producer = load(REPOSITORY_ROOT / chain["producerAuthority"]["path"])
        self.assertEqual(chain["launcherSha256"], result["launcher"]["sha256"])
        self.assertEqual(chain["launcherSha256"], producer["launcher"]["sha256"])
        self.assertEqual(producer["launcher"]["acquisition"], "rebuild-and-match-seal")
        self.assertEqual(chain["acquisition"], "rebuild-and-match-seal")

    def test_the_authority_pins_every_launcher_source_file(self):
        chain = self.section["sealChain"]
        authority = load(REPOSITORY_ROOT / chain["buildAuthority"]["path"])
        self.assertEqual(chain["sourceFilesPinned"], len(authority["sourceFiles"]))
        pinned = {row["path"] for row in authority["sourceFiles"]}
        self.assertIn(
            "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs",
            pinned,
        )
        self.assertIn("crates/boole-native-shadow-launcher/src/privilege.rs", pinned)

    def test_the_refusal_that_would_have_fired_is_quoted_from_the_build_script(self):
        """The block is automatic, so the sentence that blocks is pinned."""
        script = REPOSITORY_ROOT / self.section["refusalIsAutomatic"]["path"]
        sha256, size = digest_of(script)
        self.assertEqual(self.section["refusalIsAutomatic"]["sha256"], sha256)
        self.assertEqual(self.section["refusalIsAutomatic"]["sizeBytes"], size)
        self.assertIn(
            self.section["refusalIsAutomatic"]["message"],
            script.read_text(encoding="utf-8"),
        )

    def test_the_attempt_is_recorded_and_nothing_of_it_survives_in_the_tree(self):
        attempt = self.section["whatWasTriedAndReverted"]
        self.assertIs(attempt["revertedBeforeCommit"], True)
        self.assertIs(attempt["committed"], False)
        self.assertIs(attempt["couldHaveBeenForced"], False)
        self.assertIs(attempt["wouldHaveAbortedImageProduction"], True)
        for stamp in attempt["launcherSourceRestoredTo"]:
            sha256, size = digest_of(REPOSITORY_ROOT / stamp["path"])
            self.assertEqual(stamp["sha256"], sha256, stamp["path"])
            self.assertEqual(stamp["sizeBytes"], size, stamp["path"])


class EvidenceSeparationTests(unittest.TestCase):
    """A static gate and a real kernel run are different evidence."""

    def setUp(self):
        self.section = load(RECORD_PATH)["evidenceSeparation"]

    def test_the_two_kinds_of_evidence_are_recorded_apart(self):
        self.assertIs(self.section["sameEvidence"], False)
        static = self.section["staticSourceContract"]
        self.assertEqual(static["kind"], "source-text")
        self.assertIs(static["ranOnARealKernel"], False)
        self.assertEqual(static["gate"], pathlib.Path(__file__).name)

    def test_the_real_kernel_evidence_that_exists_is_named_and_still_in_the_tree(self):
        real = self.section["realLinuxKernelEvidence"]
        self.assertGreater(len(real), 0)
        for row in real:
            path = REPOSITORY_ROOT / row["path"]
            self.assertTrue(path.exists(), row["path"])
            self.assertIn(row["provingTest"], path.read_text(encoding="utf-8"), row["path"])
            self.assertTrue(row["covers"].strip(), row["path"])
            self.assertIs(row["coversTheCheckerSideDescentVerification"], False)

    def test_the_descent_verification_itself_has_no_real_kernel_evidence(self):
        gap = self.section["whatHasNoRealKernelEvidence"]
        self.assertEqual(gap["function"], "verify_dropped_privileges")
        self.assertIs(gap["normalPathObservedOnARealKernel"], False)
        self.assertIs(gap["failurePathsFaultInjected"], False)
        self.assertEqual(gap["failurePathsNotMeasured"], 5)
        self.assertTrue(gap["whyNotObserved"].strip())


class LimitsTests(unittest.TestCase):
    """What a source-level gate does not buy, said plainly."""

    def setUp(self):
        self.record = load(RECORD_PATH)

    def test_the_record_declines_the_claims_a_behavioural_test_would_support(self):
        text = " ".join(self.record["whatThisDoesNotEstablish"])
        for phrase in (
            "not a behavioural test",
            "not observed on a booted guest",
            "adversarial",
            "fault-injected",
        ):
            self.assertIn(phrase, text, phrase)
        self.assertGreaterEqual(len(self.record["whatThisDoesNotEstablish"]), 6)

    def test_the_stronger_test_needs_a_successor_chain_not_a_reseal(self):
        deferred = self.record["whenTheStrongerTestBecomesPossible"]
        self.assertIs(deferred["deferredNotAbandoned"], True)
        self.assertIs(deferred["byResealingTheCurrentLauncher"], False)
        self.assertIs(deferred["usableAsEvidenceForTheCurrentImage"], False)
        chain = deferred["launcherV2SuccessorChain"]
        self.assertEqual(
            [step["step"] for step in chain],
            [
                "new-launcher-source-authority",
                "new-launcher-binary",
                "new-boot-image",
                "new-boot-qualification-criteria",
            ],
        )
        self.assertTrue(deferred["whyAResealWouldBeWrong"].strip())


class RegistrationTests(unittest.TestCase):
    def test_the_suite_is_registered_in_self_test(self):
        text = (REPOSITORY_ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, text)

    def test_the_record_is_pinned_in_docs_smoke(self):
        text = (REPOSITORY_ROOT / "scripts/docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(RECORD_PATH.name, text)


if __name__ == "__main__":
    unittest.main()
