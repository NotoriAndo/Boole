#!/usr/bin/env bash
# Production-generation wrapper for the launcher-v2 successor image.
set -euo pipefail

# Root-visible commands must never be selected by a caller-controlled PATH.
# The fixed Ubuntu runner contract supplies every required host utility below.
readonly PATH="/usr/sbin:/usr/bin:/sbin:/bin"
export PATH
unset BASH_ENV ENV CDPATH PYTHONPATH PYTHONHOME LD_PRELOAD LD_LIBRARY_PATH \
  GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY \
  GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_INDEX_FILE

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
PRODUCER="$ROOT/scripts/native_shadow_successor_produce_phase_arm64_v5.py"
readonly v5_workflow_path=".github/workflows/native-shadow-successor-produce-arm64-v5.yml"
readonly exact_event_name="workflow_dispatch"
readonly exact_dispatch_ref="refs/heads/main"
readonly exact_workflow_ref="NotoriAndo/Boole/.github/workflows/native-shadow-successor-produce-arm64-v5.yml@refs/heads/main"
readonly authority_relative="native/containment/native-shadow-mac3-successor-production-authority-arm64-v7.json"
readonly claim_ref_prefix="refs/tags/boole-native-shadow-mac3-successor-production-a7-"
readonly recovery_record_name="RECOVERY-IDENTITY.json"
readonly recovery_cleanup_checkpoint_name="RECOVERY-CLEANUP-VERIFIED.json"
readonly root_anchor_prefix="/var/lib/boole/native-shadow-successor-v5/anchors"
readonly root_export_prefix="/var/lib/boole/native-shadow-successor-v5/exports"

export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

# The v5 production authority seals a 2 GiB / 200,000-entry assembled-tree
# ceiling.  Scratch can simultaneously hold the verified source archive, its
# extracted tree and the writer's deterministic working representation, so the
# tmpfs ceiling is exactly three times those sealed limits.  The production
# preflight tree is removed before production, so a fourth copy is forbidden.
readonly sealed_staging_max_total_bytes=2147483648
readonly sealed_staging_max_entries=200000
readonly staging_tmpfs_size_bytes=$((sealed_staging_max_total_bytes * 3))
readonly staging_tmpfs_inodes=$((sealed_staging_max_entries * 3))
# The transient unit can charge the entire bounded tmpfs plus one complete
# sealed tree of process/tool overhead.  R3 must exercise these exact finite
# limits before any later production authority can bind this generation.
readonly staging_unit_memory_max_bytes=$((staging_tmpfs_size_bytes + sealed_staging_max_total_bytes))
readonly staging_unit_tasks_max=128
# Three units run in series inside the 90-minute production job.  Each inner
# unit has a twenty-minute ceiling, while the
# outer supervisor caps their aggregate at fifty minutes.  That tighter
# aggregate ceiling leaves forty minutes for acquisition, bootstrap and
# evidence collection.  Historical
# closed-local production took about six minutes end to end, but R3 must still
# prove these provisional finite ceilings before any later authority can bind
# them.
readonly staging_unit_runtime_max_seconds=1200
readonly transient_unit_stop_timeout_seconds=20
readonly transient_unit_gc_observations=11
readonly transient_unit_gc_interval_seconds=1
readonly transient_unit_gc_query_timeout_seconds=2
readonly transient_unit_gc_query_kill_after_seconds=2
readonly sealed_cleanup_deadline_seconds=10
readonly systemd_run_client_timeout_seconds=$((staging_unit_runtime_max_seconds + transient_unit_stop_timeout_seconds + sealed_cleanup_deadline_seconds))
readonly systemd_control_timeout_seconds=$((transient_unit_stop_timeout_seconds + sealed_cleanup_deadline_seconds))
readonly systemd_control_kill_after_seconds=$sealed_cleanup_deadline_seconds

die() {
  printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' "$*" >&2
  exit 1
}

# Root may execute only a Git snapshot copied by the trusted workflow bootstrap
# into the fixed root-owned anchor.  In particular, root never executes bytes
# from the runner-owned Actions checkout.  Non-root verification modes remain
# usable from an ordinary development checkout.
require_root_execution_anchor() {
  local anchored_head=""
  local candidate=""
  local expected_root=""
  local permission_mode=""
  local protected_file=""
  case $mode in
    produce|cleanup-only)
      [[ $expected_tag_object_sha =~ ^[0-9a-f]{40}$ ]] \
        || die "root production lacks the tag identity"
      [[ $replica_ordinal =~ ^[12]$ ]] \
        || die "root production lacks the replica identity"
      expected_root="$root_anchor_prefix/${expected_tag_object_sha}-r${replica_ordinal}/repo"
      ;;
    rehearsal|preflight)
      [[ $ROOT =~ ^/var/lib/boole/native-shadow-successor-v5/anchors/[0-9a-f]{40}-(rehearsal|preflight)/repo$ ]] \
        || die "root effect-free execution is outside its fixed anchor"
      ;;
    *) die "this root mode is not permitted" ;;
  esac
  if [[ -n $expected_root ]]; then
    [[ $ROOT == "$expected_root" ]] \
      || die "root production is outside the claim-bound anchor"
  fi
  for candidate in \
    /var /var/lib /var/lib/boole \
    /var/lib/boole/native-shadow-successor-v5 \
    "$root_anchor_prefix" "$(dirname -- "$ROOT")" "$ROOT"; do
    [[ -d $candidate && ! -L $candidate ]] \
      || die "a production anchor ancestor is not a real directory"
    [[ $(readlink -f -- "$candidate") == "$candidate" ]] \
      || die "a production anchor ancestor is not canonical"
    [[ $(stat -c %u:%g -- "$candidate") == "0:0" ]] \
      || die "a production anchor ancestor is not owned by root"
    permission_mode="$(stat -c %a -- "$candidate")"
    [[ $permission_mode =~ ^[0-7]{3,4}$ ]] \
      || die "a production anchor ancestor mode is invalid"
    [[ $((8#$permission_mode & 8#22)) -eq 0 ]] \
      || die "a production anchor ancestor is group/world writable"
  done
  [[ -d $ROOT/.git && ! -L $ROOT/.git ]] \
    || die "the production anchor lacks a real Git directory"
  for candidate in "$ROOT/.git"; do
    [[ $(stat -c %u:%g -- "$candidate") == "0:0" ]] \
      || die "the anchored Git directory is not owned by root"
    permission_mode="$(stat -c %a -- "$candidate")"
    [[ $((8#$permission_mode & 8#22)) -eq 0 ]] \
      || die "the anchored Git directory is group/world writable"
  done
  for protected_file in \
    "$ROOT/scripts/native-shadow-successor-produce-arm64-v5.sh" \
    "$ROOT/scripts/native_shadow_successor_produce_phase_arm64_v5.py"; do
    [[ -f $protected_file && ! -L $protected_file ]] \
      || die "an anchored production program is not a regular file"
    [[ $(stat -c %u:%g -- "$protected_file") == "0:0" ]] \
      || die "an anchored production program is not owned by root"
    [[ $(stat -c %h -- "$protected_file") == "1" ]] \
      || die "an anchored production program has multiple hard links"
    permission_mode="$(stat -c %a -- "$protected_file")"
    [[ $((8#$permission_mode & 8#22)) -eq 0 ]] \
      || die "an anchored production program is group/world writable"
  done
  if [[ $mode == "rehearsal" || $mode == "preflight" ]]; then
    anchored_head="$(
      /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
        GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
        GIT_CONFIG_NOSYSTEM=1 GIT_OPTIONAL_LOCKS=0 \
        /usr/bin/git --no-replace-objects -c "safe.directory=$ROOT" \
          -C "$ROOT" rev-parse --verify "HEAD^{commit}"
    )" || die "the effect-free anchor HEAD cannot be read"
    [[ $ROOT == "$root_anchor_prefix/${anchored_head}-${mode}/repo" ]] \
      || die "the effect-free anchor identity differs from HEAD"
  fi
}

# P4 freezes these v5 bytes and the exact main-branch dispatch fence.  Verify
# it and all of its live predecessors with a stdlib-only process before
# importing the v5 producer.  R3/F7/A7 are deliberately not required by this
# first gate: R3 is what the rehearsal of these exact bytes will create
# evidence for.
verify_preregistered_bindings() {
  python3 -I -S -c '
import hashlib
import json
import os
import pathlib
import stat
import sys

root = pathlib.Path(sys.argv[1]).resolve()
p4_relative = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-main-branch-"
    "dispatch-fence-correction-arm64-v1.json"
)

def regular(relative, expected_size=None, expected_digest=None):
    pure = pathlib.PurePosixPath(relative)
    if pure.is_absolute() or ".." in pure.parts or pure.as_posix() != relative:
        raise SystemExit("unsafe production-generation binding: " + relative)
    path = root.joinpath(*pure.parts)
    info = path.lstat()
    if path.is_symlink() or not stat.S_ISREG(info.st_mode):
        raise SystemExit("production-generation binding is not regular: " + relative)
    path.resolve().relative_to(root)
    raw = path.read_bytes()
    if expected_size is not None and len(raw) != expected_size:
        raise SystemExit("production-generation binding size differs: " + relative)
    if expected_digest is not None and hashlib.sha256(raw).hexdigest() != expected_digest:
        raise SystemExit("production-generation binding digest differs: " + relative)
    return raw

raw = regular(
    p4_relative,
    13335,
    "63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b",
)
document = json.loads(raw.decode("utf-8"))
if raw != (json.dumps(document, indent=2, sort_keys=True) + "\n").encode():
    raise SystemExit("P4 is not canonical JSON")
if document.get("schema") != (
    "boole.native-shadow.mac3.launcher-v2-successor-main-branch-dispatch-"
    "fence-correction.arm64.v1"
):
    raise SystemExit("P4 schema differs")
if document.get("status") != (
    "A6-WITHHELD-PENDING-MAIN-ONLY-SUCCESSOR-GENERATION"
):
    raise SystemExit("P4 status differs")
authorisations = document.get("authorisations")
if not isinstance(authorisations, dict) or any(value not in (False, 0) for value in authorisations.values()):
    raise SystemExit("P4 grants an authority")
runs = document.get("runs")
if not isinstance(runs, dict) or any(type(value) is not int or value != 0 for value in runs.values()):
    raise SystemExit("P4 run accounting is not zero")
if document.get("boundaries") != {
    "activationAllowed": False,
    "bootableClaim": False,
    "servingClaim": False,
}:
    raise SystemExit("P4 boundary differs")
predecessors = document.get("predecessors")
if not isinstance(predecessors, list) or len(predecessors) != 6:
    raise SystemExit("P4 predecessor set differs")
for row in predecessors:
    if not isinstance(row, dict) or set(row) != {"path", "sha256", "sizeBytes"}:
        raise SystemExit("P4 predecessor identity differs")
    regular(row["path"], row["sizeBytes"], row["sha256"])
unused = document.get("unusedReservations")
if not isinstance(unused, list) or len(unused) != 2:
    raise SystemExit("P4 withdrawn reservations differ")
for row in unused:
    if set(row) != {"authorityEverGranted", "path", "requiredAbsent", "reuseForbidden"}:
        raise SystemExit("P4 withdrawn reservation shape differs")
    if row.get("authorityEverGranted") is not False or row.get("requiredAbsent") is not True or row.get("reuseForbidden") is not True:
        raise SystemExit("P4 withdrawn reservation policy differs")
    if os.path.lexists(root / row["path"]):
        raise SystemExit("withdrawn reservation exists: " + row["path"])
correction = document.get("correction", {})
if correction.get("exactEventName") != "workflow_dispatch":
    raise SystemExit("P4 event fence differs")
if correction.get("exactDispatchRef") != "refs/heads/main":
    raise SystemExit("P4 main-ref fence differs")
if correction.get("exactWorkflowRef") != (
    "NotoriAndo/Boole/.github/workflows/native-shadow-successor-produce-"
    "arm64-v5.yml@refs/heads/main"
):
    raise SystemExit("P4 workflow-ref fence differs")
future = document.get("futureBindingRequirement", {})
if future.get("fieldName") != "mainBranchDispatchFenceCorrection":
    raise SystemExit("P4 future binding field differs")
if future.get("fieldKeys") != ["path", "sha256", "sizeBytes"]:
    raise SystemExit("P4 future binding identity differs")
if future.get("correctionPath") != p4_relative:
    raise SystemExit("P4 future binding path differs")
if future.get("requiredRecords") != ["R3", "F7", "A7", "RESULT-V7"]:
    raise SystemExit("P4 future record order differs")
if future.get("exactKeysOnly") is not True or future.get(
    "directBindingRequired"
) is not True or future.get("transitiveBindingAccepted") is not False:
    raise SystemExit("P4 future direct-binding policy differs")
successor = document.get("successorGeneration", {})
expected_files = [
    "scripts/native_shadow_successor_produce_phase_arm64_v5.py",
    "scripts/test_native_shadow_successor_produce_phase_arm64_v5.py",
    "scripts/native-shadow-successor-produce-arm64-v5.sh",
    ".github/workflows/native-shadow-successor-produce-arm64-v5.yml",
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v5.py",
]
if successor.get("futureFiles") != expected_files:
    raise SystemExit("P4 successor file set differs")
' "$ROOT"
}

mode=""
cas=""
launcher=""
outputs=""
result=""
github_run_id=""
github_run_attempt=""
event_name=""
dispatch_ref=""
workflow_ref=""
workflow_path=""
head_sha=""
head_authority_sha256=""
claim_ref=""
expected_tag_object_sha=""
replica_ordinal=""
strategy_job_index=""
strategy_job_total=""
github_job=""
artifact_name=""
left_bundle=""
right_bundle=""
collectability_armed="no"
outputs_parent_identity=""
staging_mount_identity=""
expected_mount_source=""
rehearsal_unit=""
preflight_unit=""
production_unit=""
qualification_unit=""
production_supervisor_unit=""
cleanup_supervisor_unit=""
production_scratch=""
recovery_lock_path=""
recovery_lock_fd=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --verify-bindings-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="verify-bindings"
      shift
      ;;
    --rehearsal-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="rehearsal"
      shift
      ;;
    --verify-production-authority-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="production-check"
      shift
      ;;
    --dispatch-claim-message)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="dispatch-claim-message"
      shift
      ;;
    --verify-dispatch-claim)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="dispatch-claim-verify"
      shift
      ;;
    --compare-provenanced-replicas)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="compare-provenanced-replicas"
      shift
      ;;
    --preflight-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="preflight"
      shift
      ;;
    --production)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="produce"
      shift
      ;;
    --cleanup-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="cleanup-only"
      shift
      ;;
    --cas) [[ $# -ge 2 ]] || die "--cas needs a path"; cas=$2; shift 2 ;;
    --launcher) [[ $# -ge 2 ]] || die "--launcher needs a path"; launcher=$2; shift 2 ;;
    --outputs) [[ $# -ge 2 ]] || die "--outputs needs a path"; outputs=$2; shift 2 ;;
    --result) [[ $# -ge 2 ]] || die "--result needs a path"; result=$2; shift 2 ;;
    --github-run-id) [[ $# -ge 2 ]] || die "--github-run-id needs a value"; github_run_id=$2; shift 2 ;;
    --github-run-attempt) [[ $# -ge 2 ]] || die "--github-run-attempt needs a value"; github_run_attempt=$2; shift 2 ;;
    --event-name) [[ $# -ge 2 ]] || die "--event-name needs a value"; event_name=$2; shift 2 ;;
    --dispatch-ref) [[ $# -ge 2 ]] || die "--dispatch-ref needs a value"; dispatch_ref=$2; shift 2 ;;
    --workflow-ref) [[ $# -ge 2 ]] || die "--workflow-ref needs a value"; workflow_ref=$2; shift 2 ;;
    --workflow-path) [[ $# -ge 2 ]] || die "--workflow-path needs a value"; workflow_path=$2; shift 2 ;;
    --head-sha) [[ $# -ge 2 ]] || die "--head-sha needs a value"; head_sha=$2; shift 2 ;;
    --head-authority-sha256) [[ $# -ge 2 ]] || die "--head-authority-sha256 needs a value"; head_authority_sha256=$2; shift 2 ;;
    --claim-ref) [[ $# -ge 2 ]] || die "--claim-ref needs a value"; claim_ref=$2; shift 2 ;;
    --tag-object-sha) [[ $# -ge 2 ]] || die "--tag-object-sha needs a value"; expected_tag_object_sha=$2; shift 2 ;;
    --replica-ordinal) [[ $# -ge 2 ]] || die "--replica-ordinal needs a value"; replica_ordinal=$2; shift 2 ;;
    --strategy-job-index) [[ $# -ge 2 ]] || die "--strategy-job-index needs a value"; strategy_job_index=$2; shift 2 ;;
    --strategy-job-total) [[ $# -ge 2 ]] || die "--strategy-job-total needs a value"; strategy_job_total=$2; shift 2 ;;
    --github-job) [[ $# -ge 2 ]] || die "--github-job needs a value"; github_job=$2; shift 2 ;;
    --artifact-name) [[ $# -ge 2 ]] || die "--artifact-name needs a value"; artifact_name=$2; shift 2 ;;
    --left-bundle) [[ $# -ge 2 ]] || die "--left-bundle needs a path"; left_bundle=$2; shift 2 ;;
    --right-bundle) [[ $# -ge 2 ]] || die "--right-bundle needs a path"; right_bundle=$2; shift 2 ;;
    *) die "unexpected argument: $1" ;;
  esac
done

[[ -n $mode ]] || die "one mode is required"

if [[ ( $mode == "verify-bindings" || $mode == "production-check" ) \
      && ( -n $cas || -n $launcher || -n $outputs || -n $result ) ]]; then
  die "the verification-only mode accepts no other input"
fi
if [[ ( $mode == "rehearsal" || $mode == "preflight" ) && -n $outputs ]]; then
  die "the effect-free mode accepts no --outputs"
fi
if [[ $mode != "production-check" \
      && $mode != "dispatch-claim-message" && $mode != "dispatch-claim-verify" \
      && $mode != "compare-provenanced-replicas" \
      && $mode != "preflight" && $mode != "produce" \
      && $mode != "cleanup-only" \
      && ( -n $github_run_id || -n $github_run_attempt \
           || -n $event_name || -n $dispatch_ref || -n $workflow_ref \
           || -n $workflow_path || -n $head_sha \
           || -n $head_authority_sha256 || -n $claim_ref \
           || -n $expected_tag_object_sha ) ]]; then
  die "only a dispatch-claim mode accepts GitHub or claim input"
fi
if [[ ( $mode == "dispatch-claim-message" || $mode == "dispatch-claim-verify" ) \
      && ( -n $cas || -n $launcher || -n $outputs || -n $result ) ]]; then
  die "the dispatch-claim mode accepts no image or output input"
fi
if [[ $mode == "dispatch-claim-message" && -n $claim_ref ]]; then
  die "the claim-message mode derives its ref and accepts no --claim-ref"
fi
if [[ $mode == "produce" ]]; then
  for value_name in github_run_id github_run_attempt event_name dispatch_ref \
    workflow_ref workflow_path head_sha head_authority_sha256 claim_ref expected_tag_object_sha \
    replica_ordinal strategy_job_index strategy_job_total github_job \
    artifact_name; do
    [[ -n ${!value_name} ]] || die "production lacks $value_name"
  done
fi
if [[ $mode == "production-check" || $mode == "dispatch-claim-message" \
      || $mode == "dispatch-claim-verify" \
      || $mode == "compare-provenanced-replicas" \
      || $mode == "preflight" ]]; then
  for value_name in github_run_id github_run_attempt event_name dispatch_ref \
    workflow_ref workflow_path head_sha head_authority_sha256; do
    [[ -n ${!value_name} ]] || die "$mode lacks $value_name"
  done
fi
if [[ $mode == "cleanup-only" ]]; then
  for value_name in github_run_id github_run_attempt event_name dispatch_ref \
    workflow_ref workflow_path head_sha head_authority_sha256 claim_ref expected_tag_object_sha \
    replica_ordinal outputs; do
    [[ -n ${!value_name} ]] || die "cleanup-only lacks $value_name"
  done
  if [[ -n $cas || -n $launcher || -n $result \
        || -n $strategy_job_index || -n $strategy_job_total \
        || -n $github_job || -n $artifact_name ]]; then
    die "cleanup-only accepts no generation input"
  fi
fi
if [[ $mode == "compare-provenanced-replicas" \
      && ( -n $cas || -n $launcher || -n $outputs || -n $result \
           || -n $replica_ordinal || -n $strategy_job_index \
           || -n $strategy_job_total || -n $github_job \
           || -n $artifact_name ) ]]; then
  die "the provenanced comparison accepts no production output input"
fi
if [[ $mode != "compare-provenanced-replicas" \
      && ( -n $left_bundle || -n $right_bundle ) ]]; then
  die "only the provenanced comparison accepts bundle input"
fi
if [[ $mode != "produce" && $mode != "cleanup-only" \
      && ( -n $replica_ordinal || -n $strategy_job_index \
           || -n $strategy_job_total || -n $github_job \
           || -n $artifact_name ) ]]; then
  die "only production accepts replica metadata"
fi

# The complete contracts are filled below.  Keeping the mode split explicit is
# deliberate: rehearsal can never inherit an output argument, and production
# cannot silently fall back to the authority-zero generation.
case $mode in
  verify-bindings|production-check|dispatch-claim-message|dispatch-claim-verify|compare-provenanced-replicas|rehearsal|preflight|produce|cleanup-only) ;;
  *) die "unsupported mode" ;;
esac

# Reject every non-main dispatch context before root-anchor inspection,
# dependency discovery, scratch creation or any output/claim/image effect.
case $mode in
  production-check|dispatch-claim-message|dispatch-claim-verify|compare-provenanced-replicas|preflight|produce|cleanup-only)
    [[ $event_name == "$exact_event_name" ]] || die "the event name is not the fixed production event"
    [[ $dispatch_ref == "$exact_dispatch_ref" ]] || die "the dispatch ref is not exact main"
    [[ $workflow_path == "$v5_workflow_path" ]] || die "the workflow path is not the fixed v5 path"
    [[ $workflow_ref == "$exact_workflow_ref" ]] || die "the workflow ref is not exact main"
    [[ $github_run_attempt == "1" ]] || die "the production dispatch is not first-attempt only"
    [[ $github_run_id =~ ^[1-9][0-9]*$ ]] || die "the GitHub run ID differs"
    [[ $head_sha =~ ^[0-9a-f]{40}$ ]] || die "the dispatch head SHA differs"
    [[ $head_authority_sha256 =~ ^[0-9a-f]{64}$ ]] || die "the HEAD authority digest differs"
    ;;
esac

dispatch_context_argv=()
case $mode in
  production-check|dispatch-claim-message|dispatch-claim-verify|compare-provenanced-replicas|preflight|produce|cleanup-only)
    dispatch_context_argv=(
      --event-name "$event_name"
      --dispatch-ref "$dispatch_ref"
      --workflow-ref "$workflow_ref"
      --workflow-path "$workflow_path"
      --github-run-id "$github_run_id"
      --github-run-attempt "$github_run_attempt"
      --head-sha "$head_sha"
      --head-authority-sha256 "$head_authority_sha256"
    )
    ;;
esac

if [[ ${EUID} -eq 0 ]]; then
  require_root_execution_anchor
fi
verify_preregistered_bindings

if [[ $mode == "verify-bindings" ]]; then
  printf 'native-shadow launcher-v2 successor v5: bindings verified\n' >&2
  exit 0
fi

git_repo() {
  /usr/bin/env -i \
    PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
    GIT_CONFIG_NOSYSTEM=1 GIT_OPTIONAL_LOCKS=0 \
    /usr/bin/git --no-replace-objects -c "safe.directory=$ROOT" \
      -C "$ROOT" "$@"
}

prepare_dispatch_context() {
  local live_head_sha=""
  local head_authority_line=""
  local live_head_authority_sha256=""
  local attempt_id=""

  for value_name in github_run_id github_run_attempt event_name dispatch_ref \
    workflow_ref workflow_path head_sha head_authority_sha256; do
    [[ -n ${!value_name} ]] || die "the dispatch claim lacks $value_name"
  done
  [[ $event_name == "$exact_event_name" ]] \
    || die "the live event name differs"
  [[ $dispatch_ref == "$exact_dispatch_ref" ]] \
    || die "the live dispatch ref differs"
  [[ $workflow_path == "$v5_workflow_path" ]] \
    || die "the live workflow path differs"
  [[ $workflow_ref == "$exact_workflow_ref" ]] \
    || die "the live workflow ref differs"
  [[ $github_run_attempt == "1" ]] \
    || die "the dispatch claim is only valid for github.run_attempt 1"
  [[ $head_authority_sha256 =~ ^[0-9a-f]{64}$ ]] \
    || die "the supplied HEAD authority digest differs"

  [[ -x /usr/bin/git ]] || die "missing command: git"
  command -v sha256sum >/dev/null || die "missing command: sha256sum"
  live_head_sha="$(git_repo rev-parse --verify "HEAD^{commit}")" \
    || die "the checkout HEAD cannot be resolved"
  [[ $live_head_sha == "$head_sha" ]] || die "the checkout HEAD differs"
  head_authority_line="$(git_repo cat-file blob "$head_sha:$authority_relative" | sha256sum)" \
    || die "the HEAD authority blob cannot be hashed"
  live_head_authority_sha256="${head_authority_line%% *}"
  [[ $head_authority_line == "$live_head_authority_sha256  -" ]] \
    || die "the HEAD authority digest output differs"
  [[ $live_head_authority_sha256 == "$head_authority_sha256" ]] \
    || die "the supplied HEAD authority digest differs from HEAD"

  python3 -I -S "$PRODUCER" production-check --repository-root "$ROOT" \
    --event-name "$event_name" \
    --dispatch-ref "$dispatch_ref" \
    --workflow-ref "$workflow_ref" \
    --workflow-path "$workflow_path" \
    --github-run-id "$github_run_id" \
    --github-run-attempt "$github_run_attempt" \
    --head-sha "$head_sha" \
    --head-authority-sha256 "$head_authority_sha256" \
    >/dev/null
  attempt_id="$(git_repo cat-file blob "$head_sha:$authority_relative" | python3 -I -S -c '
import json
import sys
raw = sys.stdin.buffer.read(1048577)
if len(raw) > 1048576:
    raise SystemExit("authority exceeds the metadata byte limit")
document = json.loads(raw.decode("utf-8"))
if raw != (json.dumps(document, indent=2, sort_keys=True) + "\n").encode():
    raise SystemExit("authority is not canonical JSON")
grant = document.get("grant")
if not isinstance(grant, dict):
    raise SystemExit("authority grant differs")
attempt_id = grant.get("attemptId")
if not isinstance(attempt_id, str):
    raise SystemExit("authority attempt ID differs")
sys.stdout.write(attempt_id)
')" || die "the HEAD authority attempt ID cannot be read"
  claim_ref="$claim_ref_prefix$attempt_id"
  git_repo check-ref-format "$claim_ref" >/dev/null \
    || die "the fixed dispatch claim is not a valid Git ref"
}

resolve_dispatch_claim() {
  local provided_claim_ref=$claim_ref
  local resolved_tag_object_sha=""
  local resolved_target_sha=""
  prepare_dispatch_context
  [[ $provided_claim_ref == "$claim_ref" ]] \
    || die "the supplied dispatch claim ref differs"
  [[ $expected_tag_object_sha =~ ^[0-9a-f]{40}$ ]] \
    || die "the guard tag object SHA differs"
  ref_object_sha="$(git_repo rev-parse --verify "$claim_ref")" \
    || die "the dispatch claim ref cannot be resolved"
  resolved_tag_object_sha="$(git_repo rev-parse --verify "$claim_ref^{tag}")" \
    || die "the dispatch claim ref is not an annotated tag"
  [[ $resolved_tag_object_sha == "$expected_tag_object_sha" ]] \
    || die "the live dispatch claim differs from the guard tag object"
  tag_object_sha=$resolved_tag_object_sha
  [[ $(git_repo cat-file -t "$tag_object_sha") == "tag" ]] \
    || die "the dispatch claim object is not an annotated tag"
  resolved_target_sha="$(git_repo rev-parse --verify "$claim_ref^{}")" \
    || die "the dispatch claim target cannot be resolved"
  [[ $resolved_target_sha == "$head_sha" ]] \
    || die "the dispatch claim target differs from HEAD"
}

recheck_dispatch_claim_ref() {
  [[ $(git_repo rev-parse --verify "$claim_ref") == "$expected_tag_object_sha" ]] \
    || die "the dispatch claim ref changed during verification"
  [[ $(git_repo rev-parse --verify "$claim_ref^{}") == "$head_sha" ]] \
    || die "the dispatch claim target changed during verification"
}

verify_live_dispatch_claim() {
  resolve_dispatch_claim
  git_repo cat-file tag "$tag_object_sha" | \
    python3 -I -S "$PRODUCER" dispatch-claim-verify \
      --repository-root "$ROOT" \
      --claim-ref "$claim_ref" \
      --ref-object-sha "$ref_object_sha" \
      --tag-object-sha "$expected_tag_object_sha" \
      --github-run-id "$github_run_id" \
      --github-run-attempt "$github_run_attempt" \
      --event-name "$event_name" \
      --dispatch-ref "$dispatch_ref" \
      --workflow-ref "$workflow_ref" \
      --workflow-path "$workflow_path" \
      --head-sha "$head_sha" \
      --head-authority-sha256 "$head_authority_sha256" \
      >/dev/null
  recheck_dispatch_claim_ref
}

snapshot_and_verify_dispatch_claim() {
  local destination=$1
  resolve_dispatch_claim
  [[ ! -e $destination && ! -L $destination ]] \
    || die "the dispatch tag snapshot already exists"
  (umask 077; set -o noclobber; git_repo cat-file tag "$tag_object_sha" >"$destination") \
    || die "the dispatch tag snapshot could not be written"
  [[ -f $destination && ! -L $destination ]] \
    || die "the dispatch tag snapshot is not a regular file"
  [[ $(wc -c <"$destination") -le 16384 ]] \
    || die "the dispatch tag snapshot exceeds its byte limit"
  python3 -I -S "$PRODUCER" dispatch-claim-verify \
    --repository-root "$ROOT" \
    --claim-ref "$claim_ref" \
    --ref-object-sha "$ref_object_sha" \
    --tag-object-sha "$expected_tag_object_sha" \
    --github-run-id "$github_run_id" \
    --github-run-attempt "$github_run_attempt" \
    --event-name "$event_name" \
    --dispatch-ref "$dispatch_ref" \
    --workflow-ref "$workflow_ref" \
    --workflow-path "$workflow_path" \
    --head-sha "$head_sha" \
    --head-authority-sha256 "$head_authority_sha256" \
    <"$destination" >/dev/null
  recheck_dispatch_claim_ref
}

if [[ $mode == "dispatch-claim-message" ]]; then
  prepare_dispatch_context
  if git_repo show-ref --verify --quiet "$claim_ref"; then
    die "the fixed dispatch claim already exists"
  else
    claim_lookup_status=$?
    [[ $claim_lookup_status -eq 1 ]] \
      || die "the fixed dispatch claim could not be queried"
  fi
  exec python3 -I -S "$PRODUCER" dispatch-claim-message \
    --repository-root "$ROOT" \
    --github-run-id "$github_run_id" \
    --github-run-attempt "$github_run_attempt" \
    --event-name "$event_name" \
    --dispatch-ref "$dispatch_ref" \
    --workflow-ref "$workflow_ref" \
    --workflow-path "$workflow_path" \
    --head-sha "$head_sha" \
    --head-authority-sha256 "$head_authority_sha256"
fi

if [[ $mode == "dispatch-claim-verify" ]]; then
  resolve_dispatch_claim
  git_repo cat-file tag "$tag_object_sha" | \
    python3 -I -S "$PRODUCER" dispatch-claim-verify \
      --repository-root "$ROOT" \
      --claim-ref "$claim_ref" \
      --ref-object-sha "$ref_object_sha" \
      --tag-object-sha "$tag_object_sha" \
      --github-run-id "$github_run_id" \
      --github-run-attempt "$github_run_attempt" \
      --event-name "$event_name" \
      --dispatch-ref "$dispatch_ref" \
      --workflow-ref "$workflow_ref" \
      --workflow-path "$workflow_path" \
      --head-sha "$head_sha" \
      --head-authority-sha256 "$head_authority_sha256"
  recheck_dispatch_claim_ref
  exit 0
fi

if [[ $mode == "compare-provenanced-replicas" ]]; then
  [[ -n $left_bundle ]] || die "--left-bundle is required"
  [[ -n $right_bundle ]] || die "--right-bundle is required"
  resolve_dispatch_claim
  git_repo cat-file tag "$tag_object_sha" | \
    python3 -I -S "$PRODUCER" compare-provenanced-replicas \
      --repository-root "$ROOT" \
      --left-bundle "$left_bundle" \
      --right-bundle "$right_bundle" \
      --claim-ref "$claim_ref" \
      --ref-object-sha "$ref_object_sha" \
      --tag-object-sha "$tag_object_sha" \
      --github-run-id "$github_run_id" \
      --github-run-attempt "$github_run_attempt" \
      --event-name "$event_name" \
      --dispatch-ref "$dispatch_ref" \
      --workflow-ref "$workflow_ref" \
      --workflow-path "$workflow_path" \
      --head-sha "$head_sha" \
      --head-authority-sha256 "$head_authority_sha256"
  recheck_dispatch_claim_ref
  exit 0
fi

if [[ $mode == "production-check" ]]; then
  prepare_dispatch_context
  for required in \
    native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v3.json \
    native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v7.json \
    native/containment/native-shadow-mac3-successor-production-authority-arm64-v7.json; do
    [[ -f $ROOT/$required && ! -L $ROOT/$required ]] \
      || die "required binding is absent: $required"
  done
  exec python3 -I -S "$PRODUCER" production-check --repository-root "$ROOT" \
    --event-name "$event_name" \
    --dispatch-ref "$dispatch_ref" \
    --workflow-ref "$workflow_ref" \
    --workflow-path "$workflow_path" \
    --github-run-id "$github_run_id" \
    --github-run-attempt "$github_run_attempt" \
    --head-sha "$head_sha" \
    --head-authority-sha256 "$head_authority_sha256"
fi

require_host() {
  for command_name in chmod dirname env find flock gpgv install journalctl ln mkdir mktemp \
    mount python3 readlink rm sha256sum sleep stat systemctl systemd-run \
    timeout umount uname wc zstd; do
    command -v "$command_name" >/dev/null || die "missing command: $command_name"
  done
  [[ $(uname -s) == "Linux" ]] || die "the v5 producer requires Linux"
  [[ $(uname -m) == "aarch64" || $(uname -m) == "arm64" ]] \
    || die "the v5 producer requires native arm64"
}

require_cleanup_host() {
  for command_name in dirname env flock install journalctl python3 readlink rm \
    sha256sum sleep stat systemctl timeout umount uname; do
    command -v "$command_name" >/dev/null || die "missing cleanup command: $command_name"
  done
  [[ $(uname -s) == "Linux" ]] || die "the v5 cleanup requires Linux"
  [[ $(uname -m) == "aarch64" || $(uname -m) == "arm64" ]] \
    || die "the v5 cleanup requires native arm64"
}

read_tmpfs_mount_state() {
  local target=$1
  local expected_source=$2
  /usr/bin/python3 -I -S - \
    "$target" "$expected_source" \
    "$staging_tmpfs_size_bytes" "$staging_tmpfs_inodes" <<'PY'
import json
import os
import pathlib
import re
import sys

target = pathlib.Path(sys.argv[1])
expected_source = sys.argv[2]
expected_size_bytes = int(sys.argv[3])
expected_inodes = int(sys.argv[4])
if not target.is_absolute():
    raise SystemExit("the tmpfs target is not absolute")
canonical = os.path.realpath(target)
if canonical != os.fspath(target):
    raise SystemExit("the tmpfs target is not already canonical")

def decode_mount_field(value):
    for encoded, decoded in (("\\040", " "), ("\\011", "\t"), ("\\012", "\n"), ("\\134", "\\")):
        value = value.replace(encoded, decoded)
    return value

with open("/proc/self/mountinfo", "rb", buffering=0) as stream:
    raw = stream.read(8 * 1024 * 1024 + 1)
if len(raw) > 8 * 1024 * 1024:
    raise SystemExit("mountinfo exceeds the fixed byte limit")
try:
    lines = raw.decode("utf-8").splitlines()
except UnicodeDecodeError as exc:
    raise SystemExit("mountinfo is not UTF-8") from exc

matches = []
for line in lines:
    fields = line.split(" ")
    try:
        separator = fields.index("-")
    except ValueError:
        raise SystemExit("mountinfo line has no separator")
    if separator < 6 or len(fields) < separator + 4:
        raise SystemExit("mountinfo line has the wrong shape")
    mount_point = decode_mount_field(fields[4])
    if mount_point != canonical:
        continue
    matches.append({
        "fileSystemType": fields[separator + 1],
        "majorMinor": fields[2],
        "mountId": fields[0],
        "mountOptions": sorted(fields[5].split(",")),
        "mountPoint": mount_point,
        "parentId": fields[1],
        "root": decode_mount_field(fields[3]),
        "source": decode_mount_field(fields[separator + 2]),
        "superOptions": sorted(fields[separator + 3].split(",")),
    })
if len(matches) == 0:
    sys.stdout.write("absent")
    raise SystemExit(0)
if len(matches) != 1:
    raise SystemExit("the exact tmpfs target is not one mount")
record = matches[0]
if record["fileSystemType"] != "tmpfs":
    raise SystemExit("the staging mount is not tmpfs")
if record["source"] != expected_source:
    raise SystemExit("the staging mount source is not tmpfs")
if record["root"] != "/":
    raise SystemExit("the staging mount root differs")
required_options = {"rw", "nodev", "nosuid"}
observed_options = set(record["mountOptions"]) | set(record["superOptions"])
if not required_options.issubset(observed_options):
    raise SystemExit("the staging tmpfs options differ")
super_options = record["superOptions"]
size_options = [
    option.removeprefix("size=")
    for option in super_options
    if option.startswith("size=")
]
inode_options = [
    option.removeprefix("nr_inodes=")
    for option in super_options
    if option.startswith("nr_inodes=")
]
if len(size_options) != 1:
    raise SystemExit("the staging tmpfs size cap differs")
if len(inode_options) != 1:
    raise SystemExit("the staging tmpfs inode cap differs")
size_match = re.fullmatch(r"([0-9]+)([kKmMgG]?)", size_options[0])
if size_match is None:
    raise SystemExit("the staging tmpfs size cap differs")
size_factor = {
    "": 1,
    "k": 1024,
    "K": 1024,
    "m": 1024 * 1024,
    "M": 1024 * 1024,
    "g": 1024 * 1024 * 1024,
    "G": 1024 * 1024 * 1024,
}[size_match.group(2)]
observed_size_bytes = int(size_match.group(1), 10) * size_factor
if observed_size_bytes != expected_size_bytes:
    raise SystemExit("the staging tmpfs size cap differs")
if not inode_options[0].isdigit():
    raise SystemExit("the staging tmpfs inode cap differs")
observed_inodes = int(inode_options[0], 10)
if observed_inodes != expected_inodes:
    raise SystemExit("the staging tmpfs inode cap differs")
sys.stdout.write(json.dumps(record, sort_keys=True, separators=(",", ":")))
PY
}

capture_tmpfs_mount_identity() {
  local state=""
  state="$(read_tmpfs_mount_state "$1" "$2")" || return 1
  [[ $state != "absent" ]] || return 1
  printf '%s' "$state"
}

require_absent_tmpfs_mount() {
  local state=""
  state="$(read_tmpfs_mount_state "$1" "$2")" || return 1
  [[ $state == "absent" ]]
}

bounded_systemd_control() {
  /usr/bin/timeout \
    --foreground \
    --signal=TERM \
    --kill-after="${systemd_control_kill_after_seconds}s" \
    "${systemd_control_timeout_seconds}s" \
    "$@"
}

bounded_systemd_gc_control() {
  /usr/bin/timeout \
    --foreground \
    --signal=TERM \
    --kill-after="${transient_unit_gc_query_kill_after_seconds}s" \
    "${transient_unit_gc_query_timeout_seconds}s" \
    "$@"
}

wait_for_unit_absence() {
  local unit_name=$1
  local load_state=""
  local attempt=0
  for ((attempt = 0; attempt < transient_unit_gc_observations; attempt++)); do
    load_state="$(bounded_systemd_gc_control systemctl show "$unit_name" --property=LoadState --value)" \
      || return 1
    if [[ $load_state == "not-found" && ! -e "/sys/fs/cgroup/system.slice/$unit_name" ]]; then
      return 0
    fi
    if ((attempt + 1 < transient_unit_gc_observations)); then
      /usr/bin/sleep "$transient_unit_gc_interval_seconds" || return 1
    fi
  done
  return 1
}

stop_and_verify_unit() {
  local unit_name=$1
  local load_state=""
  local main_pid=""
  [[ -n $unit_name ]] || return 0
  if ! load_state="$(bounded_systemd_gc_control systemctl show "$unit_name" --property=LoadState --value)"; then
    wait_for_unit_absence "$unit_name" || return 1
    return 0
  fi
  if [[ $load_state == "not-found" ]]; then
    wait_for_unit_absence "$unit_name"
    return $?
  fi
  if ! bounded_systemd_control systemctl stop "$unit_name"; then
    wait_for_unit_absence "$unit_name" || return 1
    return 0
  fi
  bounded_systemd_control journalctl --sync || return 1
  if ! main_pid="$(bounded_systemd_gc_control systemctl show "$unit_name" --property=MainPID --value)"; then
    wait_for_unit_absence "$unit_name" || return 1
    return 0
  fi
  if [[ $main_pid != "0" ]]; then
    wait_for_unit_absence "$unit_name" || return 1
    return 0
  fi
  if ! bounded_systemd_gc_control systemctl reset-failed "$unit_name" >/dev/null 2>&1; then
    wait_for_unit_absence "$unit_name" || return 1
    return 0
  fi
  wait_for_unit_absence "$unit_name"
}

initialise_production_recovery_identity() {
  [[ $expected_tag_object_sha =~ ^[0-9a-f]{40}$ ]] \
    || die "the recovery identity lacks the annotated tag object"
  [[ $replica_ordinal =~ ^[12]$ ]] \
    || die "the recovery identity has an invalid replica ordinal"
  recovery_stem="boole-nsv5-${expected_tag_object_sha}-r${replica_ordinal}"
  production_scratch="/run/boole/native-shadow-successor-v5/${recovery_stem}"
  recovery_lock_path="/run/boole/native-shadow-successor-v5/${recovery_stem}.lock"
  preflight_unit="${recovery_stem}-preflight.service"
  production_unit="${recovery_stem}-produce.service"
  qualification_unit="${recovery_stem}-qualify.service"
  production_supervisor_unit="${recovery_stem}-supervisor.service"
  cleanup_supervisor_unit="${recovery_stem}-cleanup.service"
}

require_recovery_supervisor_membership() {
  local expected_unit=$1
  local expected_membership="/system.slice/$expected_unit"
  # The wrapper and its direct shell children run in this supervisor cgroup.
  # The three claim-named transient work services deliberately close the
  # recovery-lock descriptor and run in independent bounded cgroups; cleanup
  # explicitly stops and verifies each one before it reads recovery state.
  # The separate cleanup supervisor is checked here before it may take the
  # recovery lock or perform that stop-and-verify sequence.
  [[ $expected_unit == "$production_supervisor_unit" \
        || $expected_unit == "$cleanup_supervisor_unit" ]] \
    || die "the recovery supervisor unit is not claim-bound"
  /usr/bin/python3 -I -S - "$expected_membership" <<'PY'
import os
import stat
import sys

expected_membership = sys.argv[1]
fd = os.open("/proc/self/cgroup", os.O_RDONLY | os.O_NOFOLLOW)
try:
    metadata = os.fstat(fd)
    if not stat.S_ISREG(metadata.st_mode):
        raise SystemExit("the recovery supervisor membership is not regular")
    raw = os.read(fd, 4097)
finally:
    os.close(fd)
if len(raw) > 4096:
    raise SystemExit("the recovery supervisor membership exceeds its byte limit")
try:
    membership = raw.decode("ascii")
except UnicodeDecodeError as exc:
    raise SystemExit("the recovery supervisor membership is not ASCII") from exc
if membership != f"0::{expected_membership}\n":
    raise SystemExit("the recovery supervisor membership differs")
PY
}

require_root_recovery_parent() {
  local candidate=""
  local permission_mode=""
  [[ -d /run && ! -L /run ]] \
    || die "the runtime root is not a real directory"
  [[ $(readlink -f -- /run) == "/run" ]] \
    || die "the runtime root is not canonical"
  [[ $(stat -c %u:%g -- /run) == "0:0" ]] \
    || die "the runtime root is not owned by root"
  permission_mode="$(stat -c %a -- /run)"
  [[ $((8#$permission_mode & 8#22)) -eq 0 ]] \
    || die "the runtime root is group/world writable"

  for candidate in /run/boole /run/boole/native-shadow-successor-v5; do
    if [[ ! -e $candidate && ! -L $candidate ]]; then
      install -d -o root -g root -m 0700 -- "$candidate" \
        || die "a production recovery ancestor could not be created"
    fi
    [[ -d $candidate && ! -L $candidate ]] \
      || die "a production recovery ancestor is not a real directory"
    [[ $(readlink -f -- "$candidate") == "$candidate" ]] \
      || die "a production recovery ancestor is not canonical"
    [[ $(stat -c %u:%g -- "$candidate") == "0:0" ]] \
      || die "a production recovery ancestor is not owned by root"
    permission_mode="$(stat -c %a -- "$candidate")"
    [[ $permission_mode == "700" ]] \
      || die "a production recovery ancestor mode differs"
    [[ $((8#$permission_mode & 8#22)) -eq 0 ]] \
      || die "a production recovery ancestor is group/world writable"
  done
}

acquire_production_recovery_lock() {
  local lock_fd_identity=""
  local lock_path_identity=""
  local lock_parent="/run/boole/native-shadow-successor-v5"
  local lock_leaf="${recovery_stem}.lock"
  [[ $recovery_lock_path == "$lock_parent/$lock_leaf" ]] \
    || die "the production recovery lock path differs"
  /usr/bin/python3 -I -S - "$lock_parent" "$lock_leaf" <<'PY'
import os
import stat
import sys

parent_path, leaf = sys.argv[1:]
if not leaf or "/" in leaf or leaf in (".", ".."):
    raise SystemExit("the production recovery lock leaf differs")
parent_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
parent_fd = os.open(parent_path, parent_flags)
lock_fd = -1
created = False
try:
    try:
        lock_fd = os.open(
            leaf,
            os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
            0o600,
            dir_fd=parent_fd,
        )
        created = True
        os.fchown(lock_fd, 0, 0)
        os.fchmod(lock_fd, 0o600)
    except FileExistsError:
        lock_fd = os.open(
            leaf,
            os.O_RDWR | os.O_NOFOLLOW,
            dir_fd=parent_fd,
        )
    metadata = os.fstat(lock_fd)
    if not stat.S_ISREG(metadata.st_mode):
        raise SystemExit("the production recovery lock is not regular")
    if metadata.st_uid != 0 or metadata.st_gid != 0:
        raise SystemExit("the production recovery lock owner differs")
    if stat.S_IMODE(metadata.st_mode) != 0o600:
        raise SystemExit("the production recovery lock mode differs")
    if metadata.st_nlink != 1:
        raise SystemExit("the production recovery lock link count differs")
    if created:
        os.fsync(lock_fd)
        os.fsync(parent_fd)
finally:
    if lock_fd >= 0:
        os.close(lock_fd)
    os.close(parent_fd)
PY
  exec {recovery_lock_fd}<>"$recovery_lock_path" \
    || die "the production recovery lock could not be opened"
  /usr/bin/flock --exclusive --nonblock "$recovery_lock_fd" \
    || die "the claim replica already has an active producer or cleanup"
  [[ -f $recovery_lock_path && ! -L $recovery_lock_path ]] \
    || die "the locked production recovery claim is not a real file"
  lock_path_identity="$(stat -Lc %d:%i:%u:%g:%a:%h -- "$recovery_lock_path")" \
    || die "the production recovery lock path identity could not be read"
  lock_fd_identity="$(stat -Lc %d:%i:%u:%g:%a:%h -- "/proc/$$/fd/$recovery_lock_fd")" \
    || die "the production recovery lock descriptor identity could not be read"
  [[ $lock_path_identity == "$lock_fd_identity" ]] \
    || die "the production recovery lock path changed after acquisition"
  [[ $lock_fd_identity == *":0:0:600:1" ]] \
    || die "the production recovery lock descriptor metadata differs"
}

require_dedicated_write_parent() {
  local candidate=$1
  local canonical=""
  local first_member=""
  [[ $candidate == /* ]] || die "the write parent must be absolute"
  [[ "/$candidate/" != *"/../"* ]] \
    || die "the write parent must not contain dot-dot"
  [[ -d $candidate && ! -L $candidate ]] \
    || die "the write parent must be an existing real directory"
  canonical="$(readlink -f -- "$candidate")" \
    || die "the write parent cannot be canonicalised"
  [[ $canonical == "$candidate" ]] \
    || die "the write parent must already be canonical"
  case $canonical in
    "/"|"/usr"|"/usr/"*|"/etc"|"/etc/"*|"/boot"|"/boot/"*|"$ROOT"|"$ROOT/"*)
      die "the write parent overlaps a protected tree"
      ;;
  esac
  [[ $(stat -c %a -- "$candidate") == "700" ]] \
    || die "the write parent must have mode 0700"
  [[ $(stat -c %u:%g -- "$candidate") == "0:0" ]] \
    || die "the write parent must be owned by root:root"
  first_member="$(find "$candidate" -mindepth 1 -maxdepth 1 -print -quit)"
  [[ -z $first_member ]] || die "the write parent must be empty and dedicated"
}

publish_collectable_parent() {
  local marker=""
  local parent_device=""
  local parent_inode=""
  [[ $collectability_armed == "yes" ]] || return 0
  marker="$outputs/ATTEMPT-CONSUMED.json"
  [[ ! -e $marker && ! -L $marker ]] && return 0
  if [[ $(stat -c %d:%i -- "$outputs_parent") != "$outputs_parent_identity" ]]; then
    printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
      "the dedicated write parent identity changed" >&2
    return 1
  fi
  parent_device=${outputs_parent_identity%%:*}
  parent_inode=${outputs_parent_identity#*:}
  if ! /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" seal-replica-bundle \
      --repository-root "$ROOT" \
      "${dispatch_context_argv[@]}" \
      --parent "$outputs_parent" \
      --parent-device "$parent_device" \
      --parent-inode "$parent_inode" \
      --successful no; then
    printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
      "the consumed replica bundle could not be sealed for collection" >&2
    return 1
  fi
}

resume_successful_parent() {
  local parent_device=""
  local parent_inode=""
  local resume_job_index=""
  local resume_artifact_name=""
  [[ $collectability_armed == "yes" ]] || return 0
  [[ $replica_ordinal =~ ^[12]$ ]] \
    || { recovery_error "the successful replica ordinal differs"; return 1; }
  if [[ $(stat -c %d:%i -- "$outputs_parent") != "$outputs_parent_identity" ]]; then
    recovery_error "the successful write parent identity changed"
    return 1
  fi
  parent_device=${outputs_parent_identity%%:*}
  parent_inode=${outputs_parent_identity#*:}
  resume_job_index=$((replica_ordinal - 1))
  resume_artifact_name="native-shadow-successor-v5-replica-${replica_ordinal}"
  recheck_dispatch_claim_ref \
    || { recovery_error "the successful dispatch claim changed"; return 1; }
  if ! git_repo cat-file tag "$tag_object_sha" | \
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
      PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
      /usr/bin/python3 -I -S "$PRODUCER" publish-and-seal-replica \
        --repository-root "$ROOT" \
        --parent "$outputs_parent" \
        --parent-device "$parent_device" \
        --parent-inode "$parent_inode" \
        --outputs "$outputs" \
        --result "$outputs_parent/REPLICA-PROVENANCE.json" \
        --replica-ordinal "$replica_ordinal" \
        --strategy-job-index "$resume_job_index" \
        --strategy-job-total 2 \
        --github-job produce \
        --artifact-name "$resume_artifact_name" \
        --claim-ref "$claim_ref" \
        --ref-object-sha "$ref_object_sha" \
        --tag-object-sha "$expected_tag_object_sha" \
        --github-run-id "$github_run_id" \
        --github-run-attempt "$github_run_attempt" \
        --event-name "$event_name" \
        --dispatch-ref "$dispatch_ref" \
        --workflow-ref "$workflow_ref" \
        --workflow-path "$workflow_path" \
        --head-sha "$head_sha" \
        --head-authority-sha256 "$head_authority_sha256"; then
    recovery_error "the successful replica bundle could not resume sealing"
    return 1
  fi
  recheck_dispatch_claim_ref \
    || { recovery_error "the successful dispatch claim changed"; return 1; }
}

require_recovery_output_parent() {
  local candidate=$1
  local canonical=""
  local mode_value=""
  [[ $candidate == /* ]] || die "the recovery output parent must be absolute"
  [[ "/$candidate/" != *"/../"* ]] \
    || die "the recovery output parent must not contain dot-dot"
  [[ -d $candidate && ! -L $candidate ]] \
    || die "the recovery output parent must be a real directory"
  canonical="$(readlink -f -- "$candidate")" \
    || die "the recovery output parent cannot be canonicalised"
  [[ $canonical == "$candidate" ]] \
    || die "the recovery output parent must already be canonical"
  case $canonical in
    "/"|"/usr"|"/usr/"*|"/etc"|"/etc/"*|"/boot"|"/boot/"*|"$ROOT"|"$ROOT/"*)
      die "the recovery output parent overlaps a protected tree"
      ;;
  esac
  [[ $(stat -c %u:%g -- "$candidate") == "0:0" ]] \
    || die "the recovery output parent owner differs"
  mode_value="$(stat -c %a -- "$candidate")"
  [[ $mode_value == "700" || $mode_value == "711" ]] \
    || die "the recovery output parent mode differs"
}

# Production and recovery may publish only below the claim-derived root-owned
# export parent prepared by the trusted workflow bootstrap.  Check every
# existing ancestor because a safe leaf beneath a writable parent is not a
# stable security boundary.
require_claim_bound_export_parent() {
  local candidate=$1
  local expected="/var/lib/boole/native-shadow-successor-v5/exports/$expected_tag_object_sha/replica-$replica_ordinal"
  local ancestor=""
  local mode=""
  [[ $expected_tag_object_sha =~ ^[0-9a-f]{40}$ ]] \
    || die "the export parent lacks the annotated tag identity"
  [[ $replica_ordinal =~ ^[12]$ ]] \
    || die "the export parent lacks the replica identity"
  [[ $candidate == "$expected" ]] \
    || die "the export parent is not claim-bound"
  for ancestor in \
    /var /var/lib /var/lib/boole \
    /var/lib/boole/native-shadow-successor-v5 \
    "$root_export_prefix" \
    "$root_export_prefix/$expected_tag_object_sha" "$candidate"; do
    [[ -d $ancestor && ! -L $ancestor ]] \
      || die "an export ancestor is not a real directory"
    [[ $(readlink -f -- "$ancestor") == "$ancestor" ]] \
      || die "an export ancestor is not canonical"
    [[ $(stat -c %u:%g -- "$ancestor") == "0:0" ]] \
      || die "an export ancestor is not owned by root"
    mode="$(stat -c %a -- "$ancestor")"
    [[ $mode =~ ^[0-7]{3,4}$ ]] || die "an export ancestor mode is invalid"
    [[ $((8#$mode & 8#22)) -eq 0 ]] \
      || die "an export ancestor is group/world writable"
  done
  mode="$(stat -c %a -- "$candidate")"
  [[ $mode == "700" || $mode == "711" ]] \
    || die "the claim-bound export parent mode differs"
}

recovery_error() {
  printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' "$*" >&2
  return 1
}

recover_production_state() {
  local current_mount_identity=""
  local mount_state=""
  local output_recovery_state=""
  local parent_device=""
  local parent_inode=""
  local record_recovery_state=""
  [[ $scratch == "$production_scratch" ]] \
    || { recovery_error "the recovery scratch is not the exact claim-bound path"; return 1; }
  [[ $expected_mount_source == "$recovery_stem" ]] \
    || { recovery_error "the recovery mount source is not claim-bound"; return 1; }

  stop_and_verify_unit "$qualification_unit" \
    || { recovery_error "qualification transient unit cleanup failed"; return 1; }
  stop_and_verify_unit "$production_unit" \
    || { recovery_error "production transient unit cleanup failed"; return 1; }
  stop_and_verify_unit "$preflight_unit" \
    || { recovery_error "preflight transient unit cleanup failed"; return 1; }

  parent_device=${outputs_parent_identity%%:*}
  parent_inode=${outputs_parent_identity#*:}
  output_recovery_state="$(
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
      PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
      /usr/bin/python3 -I -S "$PRODUCER" reconcile-output-state \
        --repository-root "$ROOT" \
        "${dispatch_context_argv[@]}" \
        --parent "$outputs_parent" \
        --parent-device "$parent_device" \
        --parent-inode "$parent_inode"
  )" || { recovery_error "the production output recovery state differs"; return 1; }
  if [[ $output_recovery_state != "consumed" \
        && $output_recovery_state != "sealed" \
        && $output_recovery_state != "success-pending-seal" \
        && $output_recovery_state != "unconsumed" ]]; then
    recovery_error "the production output recovery state is unknown"
    return 1
  fi

  if [[ ! -e $scratch && ! -L $scratch ]]; then
    if [[ $output_recovery_state == "consumed" ]]; then
      publish_collectable_parent \
        || { recovery_error "the already-clean consumed bundle is invalid"; return 1; }
    elif [[ $output_recovery_state == "sealed" ]]; then
      :
    elif [[ $output_recovery_state == "success-pending-seal" ]]; then
      resume_successful_parent \
        || { recovery_error "the already-clean successful bundle could not be sealed"; return 1; }
    elif [[ $output_recovery_state == "unconsumed" ]]; then
      :
    else
      recovery_error "the already-clean production output state differs"
      return 1
    fi
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
      PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
      /usr/bin/python3 -I -S "$PRODUCER" remove-verified-recovery \
        --repository-root "$ROOT" \
        "${dispatch_context_argv[@]}" \
        --scratch "$scratch" \
        --outputs-parent "$outputs_parent" \
        --parent-device "$parent_device" \
        --parent-inode "$parent_inode" \
        --recovery-stem "$recovery_stem" \
      >/dev/null \
      || { recovery_error "the production recovery tombstone remains"; return 1; }
    printf 'native-shadow launcher-v2 successor v5: cleanup already-clean\n' >&2
    return 0
  fi
  [[ -d $scratch && ! -L $scratch ]] \
    || { recovery_error "the production recovery scratch is not a real directory"; return 1; }
  [[ $(stat -c %u:%g -- "$scratch") == "0:0" ]] \
    || { recovery_error "the production recovery scratch owner differs"; return 1; }
  [[ $(stat -c %a -- "$scratch") == "700" ]] \
    || { recovery_error "the production recovery scratch mode differs"; return 1; }
  if [[ ! -e $staging && ! -L $staging ]]; then
    [[ $output_recovery_state == "unconsumed" ]] \
      || { recovery_error "incomplete recovery has consumed output"; return 1; }
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
      PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
      /usr/bin/python3 -I -S "$PRODUCER" discard-incomplete-recovery \
        --repository-root "$ROOT" \
        "${dispatch_context_argv[@]}" \
        --scratch "$scratch" \
        --outputs-parent "$outputs_parent" \
        --parent-device "$parent_device" \
        --parent-inode "$parent_inode" \
        --recovery-stem "$recovery_stem" \
      >/dev/null \
      || { recovery_error "incomplete production recovery discard failed"; return 1; }
    return 0
  fi
  [[ -d $staging && ! -L $staging ]] \
    || { recovery_error "the production recovery staging path differs"; return 1; }

  mount_state="$(read_tmpfs_mount_state "$staging" "$expected_mount_source")" \
    || { recovery_error "the production recovery mount state could not be read"; return 1; }
  if [[ $mount_state != "absent" ]]; then
    current_mount_identity="$mount_state"
    record_recovery_state="$(
      printf '%s' "$current_mount_identity" | \
        /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
        PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
        /usr/bin/python3 -I -S "$PRODUCER" reconcile-recovery-record-publication \
          --repository-root "$ROOT" \
          "${dispatch_context_argv[@]}" \
          --scratch "$scratch" \
          --outputs-parent "$outputs_parent" \
          --parent-device "$parent_device" \
          --parent-inode "$parent_inode" \
          --recovery-stem "$recovery_stem"
    )" || { recovery_error "the production recovery record publication differs"; return 1; }
    if [[ $record_recovery_state == "incomplete-no-record" ]]; then
      [[ $output_recovery_state == "unconsumed" ]] \
        || { recovery_error "incomplete recovery has consumed output"; return 1; }
      umount "$staging" \
        || { recovery_error "incomplete production tmpfs cleanup failed"; return 1; }
      require_absent_tmpfs_mount "$staging" "$expected_mount_source" \
        || { recovery_error "incomplete production mount remains"; return 1; }
      /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
        PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
        /usr/bin/python3 -I -S "$PRODUCER" discard-incomplete-recovery \
          --repository-root "$ROOT" \
          "${dispatch_context_argv[@]}" \
          --scratch "$scratch" \
          --outputs-parent "$outputs_parent" \
          --parent-device "$parent_device" \
          --parent-inode "$parent_inode" \
          --recovery-stem "$recovery_stem" \
        >/dev/null \
        || { recovery_error "incomplete production recovery discard failed"; return 1; }
      return 0
    fi
    [[ $record_recovery_state == "record-ready" ]] \
      || { recovery_error "the production recovery record state is unknown"; return 1; }
    printf '%s' "$current_mount_identity" | \
      /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
      PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
      /usr/bin/python3 -I -S "$PRODUCER" publish-cleanup-checkpoint \
        --repository-root "$ROOT" \
        "${dispatch_context_argv[@]}" \
        --scratch "$scratch" \
        --outputs-parent "$outputs_parent" \
        --parent-device "$parent_device" \
        --parent-inode "$parent_inode" \
        --recovery-stem "$recovery_stem" \
      || { recovery_error "the production cleanup checkpoint could not be published"; return 1; }
    umount "$staging" \
      || { recovery_error "production tmpfs cleanup failed"; return 1; }
  elif [[ ! -e $scratch/$recovery_record_name \
          && ! -L $scratch/$recovery_record_name \
          && ! -e $scratch/$recovery_cleanup_checkpoint_name \
          && ! -L $scratch/$recovery_cleanup_checkpoint_name ]]; then
    [[ $output_recovery_state == "unconsumed" ]] \
      || { recovery_error "incomplete recovery has consumed output"; return 1; }
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
      PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
      /usr/bin/python3 -I -S "$PRODUCER" discard-incomplete-recovery \
        --repository-root "$ROOT" \
        "${dispatch_context_argv[@]}" \
        --scratch "$scratch" \
        --outputs-parent "$outputs_parent" \
        --parent-device "$parent_device" \
        --parent-inode "$parent_inode" \
        --recovery-stem "$recovery_stem" \
      >/dev/null \
      || { recovery_error "incomplete production recovery discard failed"; return 1; }
    return 0
  fi
  require_absent_tmpfs_mount "$staging" "$expected_mount_source" \
    || { recovery_error "the production recovery mount remains after unmount"; return 1; }
  /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" verify-recovery-after-unmount \
      --repository-root "$ROOT" \
      "${dispatch_context_argv[@]}" \
      --scratch "$scratch" \
      --outputs-parent "$outputs_parent" \
      --parent-device "$parent_device" \
      --parent-inode "$parent_inode" \
      --recovery-stem "$recovery_stem" \
    || { recovery_error "the post-unmount production recovery state differs"; return 1; }

  require_absent_tmpfs_mount "$staging" "$expected_mount_source" \
    || { recovery_error "the production recovery mount reappeared before publication"; return 1; }

  if [[ $output_recovery_state == "consumed" ]]; then
    publish_collectable_parent \
      || { recovery_error "the consumed replica bundle could not be sealed"; return 1; }
  elif [[ $output_recovery_state == "sealed" ]]; then
    :
  elif [[ $output_recovery_state == "success-pending-seal" ]]; then
    resume_successful_parent \
      || { recovery_error "the successful replica bundle could not be sealed"; return 1; }
  elif [[ $output_recovery_state == "unconsumed" ]]; then
    :
  else
    recovery_error "the post-unmount production output state differs"
    return 1
  fi
  /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" remove-verified-recovery \
      --repository-root "$ROOT" \
      "${dispatch_context_argv[@]}" \
      --scratch "$scratch" \
      --outputs-parent "$outputs_parent" \
      --parent-device "$parent_device" \
      --parent-inode "$parent_inode" \
      --recovery-stem "$recovery_stem" \
    >/dev/null \
    || { recovery_error "production scratch cleanup failed"; return 1; }
  [[ ! -e $scratch && ! -L $scratch ]] \
    || { recovery_error "production scratch remains after cleanup"; return 1; }
}

require_inputs() {
  local result_parent_policy=$1
  [[ -n $cas ]] || die "--cas is required"
  [[ -n $launcher ]] || die "--launcher is required"
  [[ -n $result ]] || die "--result is required"
  [[ $result == /* ]] || die "--result must be absolute"
  [[ -d $cas && ! -L $cas ]] || die "the verified payload store is absent"
  [[ -f $launcher && ! -L $launcher ]] || die "the launcher-v2 ELF is absent"
  result_parent="$(dirname -- "$result")"
  if [[ $result_parent_policy == "require-result-parent" ]]; then
    [[ -d $result_parent && ! -L $result_parent ]] \
      || die "the result parent must be an existing real directory"
  elif [[ $result_parent_policy != "allow-missing-result-parent" ]]; then
    die "invalid result-parent policy"
  fi
  [[ ! -e $result && ! -L $result ]] || die "the result already exists"
}

isolation_prefix() {
  local unit_name=$1
  printf '%s\n' \
    /usr/bin/timeout \
    --foreground \
    --signal=TERM \
    --kill-after="${sealed_cleanup_deadline_seconds}s" \
    "${systemd_run_client_timeout_seconds}s" \
    systemd-run \
    --unit="$unit_name" \
    --pipe \
    --wait \
    --collect \
    --service-type=exec \
    --property=PrivateNetwork=yes \
    --property=ProtectSystem=strict \
    --property=NoNewPrivileges=yes \
    --property=KillMode=control-group \
    --property="TimeoutStopSec=${transient_unit_stop_timeout_seconds}s" \
    --property=SendSIGKILL=yes \
    --property=Restart=no \
    --property=MemoryAccounting=yes \
    --property="MemoryMax=${staging_unit_memory_max_bytes}" \
    --property=MemorySwapMax=0 \
    --property=TasksAccounting=yes \
    --property="TasksMax=${staging_unit_tasks_max}" \
    --property=CPUAccounting=yes \
    --property="RuntimeMaxSec=${staging_unit_runtime_max_seconds}s" \
    --property=OOMPolicy=kill \
    --property=PrivateDevices=yes \
    --property=PrivateMounts=yes \
    --property=RestrictAddressFamilies=AF_UNIX \
    --property=CapabilityBoundingSet= \
    --property=AmbientCapabilities= \
    '--property=SystemCallFilter=~kill tkill tgkill pidfd_send_signal rt_sigqueueinfo rt_tgsigqueueinfo ptrace process_vm_readv process_vm_writev'
}

qualification_prefix() {
  local unit_name=$1
  printf '%s\n' \
    /usr/bin/timeout \
    --foreground \
    --signal=TERM \
    --kill-after="${sealed_cleanup_deadline_seconds}s" \
    "${systemd_run_client_timeout_seconds}s" \
    systemd-run \
    --unit="$unit_name" \
    --pipe \
    --wait \
    --collect \
    --service-type=exec \
    --property=PrivateNetwork=yes \
    --property=ProtectSystem=strict \
    --property=NoNewPrivileges=yes \
    --property=KillMode=control-group \
    --property="TimeoutStopSec=${transient_unit_stop_timeout_seconds}s" \
    --property=SendSIGKILL=yes \
    --property=Restart=no \
    --property=MemoryAccounting=yes \
    --property="MemoryMax=${staging_unit_memory_max_bytes}" \
    --property=MemorySwapMax=0 \
    --property=TasksAccounting=yes \
    --property="TasksMax=${staging_unit_tasks_max}" \
    --property=CPUAccounting=yes \
    --property="RuntimeMaxSec=${staging_unit_runtime_max_seconds}s" \
    --property=OOMPolicy=kill \
    --property=PrivateMounts=yes \
    --property=RestrictAddressFamilies=AF_UNIX \
    --property=PrivateDevices=no \
    --property=DevicePolicy=closed \
    '--property=DeviceAllow=/dev/loop-control rw' \
    '--property=DeviceAllow=block-loop rw' \
    --property=CapabilityBoundingSet=CAP_SYS_ADMIN \
    --property=AmbientCapabilities= \
    '--property=SystemCallFilter=~kill tkill tgkill pidfd_send_signal rt_sigqueueinfo rt_tgsigqueueinfo ptrace process_vm_readv process_vm_writev'
}

if [[ $mode == "cleanup-only" ]]; then
  verify_live_dispatch_claim
  python3 -I -S "$PRODUCER" production-check --repository-root "$ROOT" \
    --event-name "$event_name" \
    --dispatch-ref "$dispatch_ref" \
    --workflow-ref "$workflow_ref" \
    --workflow-path "$workflow_path" \
    --github-run-id "$github_run_id" \
    --github-run-attempt "$github_run_attempt" \
    --head-sha "$head_sha" \
    --head-authority-sha256 "$head_authority_sha256"
  [[ ${EUID} -eq 0 ]] \
    || die "production cleanup must run as root"
  require_cleanup_host
  [[ $outputs == /* ]] || die "cleanup-only --outputs must be absolute"
  [[ ${outputs##*/} == "outputs" ]] \
    || die "cleanup-only --outputs must use the fixed leaf name"
  outputs_parent="$(dirname -- "$outputs")"
  require_claim_bound_export_parent "$outputs_parent"
  initialise_production_recovery_identity
  require_recovery_supervisor_membership "$cleanup_supervisor_unit"
  stop_and_verify_unit "$production_supervisor_unit" \
    || die "the production supervisor could not be stopped before cleanup"
  require_root_recovery_parent
  acquire_production_recovery_lock
  require_recovery_output_parent "$outputs_parent"
  outputs_parent_identity="$(stat -c %d:%i -- "$outputs_parent")"
  collectability_armed="yes"
  scratch="$production_scratch"
  staging="$scratch/staging"
  expected_mount_source="$recovery_stem"
  recover_production_state
  exit 0
fi

if [[ $mode == "rehearsal" ]]; then
  require_inputs require-result-parent
  require_host
  [[ ${EUID} -eq 0 ]] \
    || die "rehearsal isolation must be installed as root"
  require_dedicated_write_parent "$result_parent"
  rehearsal_parent_identity="$(stat -c %d:%i -- "$result_parent")"
  rehearsal_root="$(mktemp -d /tmp/boole-native-shadow-successor-v5-rehearsal.XXXXXX)"
  expected_scratch_prefix="/tmp/boole-native-shadow-successor-v5-rehearsal."
  rehearsal_unit="boole-nsv5-rehearsal-${rehearsal_root##*.}.service"
  expected_mount_source="${rehearsal_unit%.service}"
  staging="$rehearsal_root/staging"
  rehearsal_scratch="$staging/rehearsal"
  cleanup() {
    local primary_status=$?
    local cleanup_status=0
    local current_mount_identity=""
    local mount_state=""
    trap - EXIT TERM INT HUP
    if [[ $rehearsal_root != "$expected_scratch_prefix"* ]]; then
      printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
        "refusing cleanup outside the private rehearsal prefix" >&2
      cleanup_status=1
    else
      if ! stop_and_verify_unit "$rehearsal_unit"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "rehearsal transient unit cleanup failed" >&2
        cleanup_status=1
      elif [[ -n $staging_mount_identity ]]; then
        if ! current_mount_identity="$(capture_tmpfs_mount_identity "$staging" "$expected_mount_source")"; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal tmpfs identity could not be re-read" >&2
          cleanup_status=1
        elif [[ $current_mount_identity != "$staging_mount_identity" ]]; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal tmpfs identity changed" >&2
          cleanup_status=1
        elif ! umount "$staging"; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal tmpfs cleanup failed" >&2
          cleanup_status=1
        elif ! require_absent_tmpfs_mount "$staging" "$expected_mount_source"; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal tmpfs remains after unmount" >&2
          cleanup_status=1
        fi
      else
        if ! mount_state="$(read_tmpfs_mount_state "$staging" "$expected_mount_source")"; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal tmpfs state could not be read" >&2
          cleanup_status=1
        elif [[ $mount_state != "absent" ]]; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "refusing to unmount an unidentified rehearsal mount" >&2
          cleanup_status=1
        fi
      fi
      if (( cleanup_status == 0 )); then
        if ! require_absent_tmpfs_mount "$staging" "$expected_mount_source"; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal mount absence could not be proved" >&2
          cleanup_status=1
        elif ! rm -rf -- "$rehearsal_root"; then
          printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
            "rehearsal scratch cleanup failed" >&2
          cleanup_status=1
        fi
      fi
    fi
    if (( primary_status != 0 )); then
      if (( cleanup_status != 0 )); then
        printf 'native-shadow launcher-v2 successor v5: cleanup also failed after primary status %s\n' \
          "$primary_status" >&2
      fi
      exit "$primary_status"
    fi
    exit "$cleanup_status"
  }
  trap cleanup EXIT
  trap 'exit 143' TERM
  trap 'exit 130' INT
  trap 'exit 129' HUP
  mkdir -m 0700 "$staging"
  mount -t tmpfs \
    -o "mode=0700,nodev,nosuid,size=${staging_tmpfs_size_bytes},nr_inodes=${staging_tmpfs_inodes}" \
    "$expected_mount_source" "$staging"
  staging_mount_identity="$(capture_tmpfs_mount_identity "$staging" "$expected_mount_source")" \
    || die "the rehearsal tmpfs identity could not be captured"
  mkdir -m 0700 "$rehearsal_scratch"
  rehearsal_argv=()
  while IFS= read -r item; do rehearsal_argv+=("$item"); done < <(isolation_prefix "$rehearsal_unit")
  rehearsal_argv+=(
    "--property=ReadWritePaths=$rehearsal_scratch"
    "--property=ReadWritePaths=$result_parent" --
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" rehearsal
    --repository-root "$ROOT" --cas "$cas" --launcher "$launcher"
    --scratch "$rehearsal_scratch"
    --gpgv "$(readlink -f "$(command -v gpgv)")"
    --zstd "$(readlink -f "$(command -v zstd)")"
    --expected-systemd-unit "$rehearsal_unit"
    --result "$result"
  )
  "${rehearsal_argv[@]}"
  stop_and_verify_unit "$rehearsal_unit" \
    || die "the successful rehearsal transient unit remains"
  [[ -f $result && ! -L $result ]] \
    || die "the rehearsal result is not a regular file"
  [[ $(stat -c %d:%i -- "$result_parent") == "$rehearsal_parent_identity" ]] \
    || die "the rehearsal result parent identity changed"
  [[ $(stat -c %u:%g -- "$result_parent") == "0:0" ]] \
    || die "the rehearsal result parent owner changed"
  [[ $(stat -c %a -- "$result_parent") == "700" ]] \
    || die "the rehearsal result parent mode changed"
  [[ $(stat -c %a -- "$result") == "444" ]] \
    || die "the rehearsal result is not read-only"
  unexpected_rehearsal_sibling="$(find "$result_parent" -mindepth 1 -maxdepth 1 ! -path "$result" -print -quit)"
  [[ -z $unexpected_rehearsal_sibling ]] \
    || die "the rehearsal result parent contains an unexpected sibling"
  chmod 0711 -- "$result_parent" \
    || die "the rehearsal result parent could not be opened for collection"
  exit 0
fi

# Both production preflight and production require the complete fixed chain.
# This runs before host discovery, scratch creation or any output path.
if [[ $mode == "preflight" || $mode == "produce" ]]; then
  if [[ $mode == "produce" ]]; then
    verify_live_dispatch_claim
  else
    prepare_dispatch_context
  fi
  python3 -I -S "$PRODUCER" production-check --repository-root "$ROOT" \
    --event-name "$event_name" \
    --dispatch-ref "$dispatch_ref" \
    --workflow-ref "$workflow_ref" \
    --workflow-path "$workflow_path" \
    --github-run-id "$github_run_id" \
    --github-run-attempt "$github_run_attempt" \
    --head-sha "$head_sha" \
    --head-authority-sha256 "$head_authority_sha256"
  [[ ${EUID} -eq 0 ]] \
    || die "production isolation must be installed as root"
fi

if [[ $mode == "produce" ]]; then
  [[ -n $outputs ]] || die "--outputs is required for --production"
  [[ $outputs == /* ]] || die "--outputs must be absolute"
  [[ ${outputs##*/} == "outputs" ]] \
    || die "--outputs must use the fixed leaf name"
  [[ $result == "$outputs/PRODUCE-RESULT.json" ]] \
    || die "--result must be the fixed qualified result below --outputs"
  [[ ! -e $outputs && ! -L $outputs ]] || die "the production output already exists"
  outputs_parent="$(dirname -- "$outputs")"
  [[ -d $outputs_parent && ! -L $outputs_parent ]] \
    || die "the output parent must be an existing real directory"
  require_claim_bound_export_parent "$outputs_parent"
  require_inputs allow-missing-result-parent
else
  require_inputs require-result-parent
fi

require_host

if [[ $mode == "produce" ]]; then
  require_dedicated_write_parent "$outputs_parent"
  outputs_parent_identity="$(stat -c %d:%i -- "$outputs_parent")"
  collectability_armed="yes"
  initialise_production_recovery_identity
  require_recovery_supervisor_membership "$production_supervisor_unit"
elif [[ $mode == "preflight" ]]; then
  require_dedicated_write_parent "$result_parent"
fi

expected_scratch_prefix="/tmp/boole-native-shadow-successor-v5."
if [[ $mode == "produce" ]]; then
  scratch="$production_scratch"
  require_root_recovery_parent
  acquire_production_recovery_lock
  [[ ! -e $scratch && ! -L $scratch ]] \
    || die "production recovery state already exists; cleanup is required"
  mkdir -m 0700 -- "$scratch"
else
  scratch="$(mktemp -d /tmp/boole-native-shadow-successor-v5.XXXXXX)"
  preflight_unit="boole-nsv5-preflight-${scratch##*.}.service"
fi
if [[ $mode == "produce" ]]; then
  expected_mount_source="$recovery_stem"
else
  expected_mount_source="${preflight_unit%.service}"
fi
staging="$scratch/staging"
staging_preflight="$staging/preflight"
staging_production="$staging/production"
cleanup() {
  local primary_status=$?
  local cleanup_status=0
  local current_mount_identity=""
  local mount_state=""
  trap - EXIT TERM INT HUP
  if [[ $mode == "produce" ]]; then
    if ! recover_production_state; then
      cleanup_status=1
    fi
  elif [[ $scratch != "$expected_scratch_prefix"* ]]; then
    printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
      "refusing cleanup outside the private preflight prefix" >&2
    cleanup_status=1
  else
    if ! stop_and_verify_unit "$preflight_unit"; then
      printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
        "preflight transient unit cleanup failed" >&2
      cleanup_status=1
    fi
    if (( cleanup_status == 0 )) && [[ -n $staging_mount_identity ]]; then
      if ! current_mount_identity="$(capture_tmpfs_mount_identity "$staging" "$expected_mount_source")"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production tmpfs identity could not be re-read" >&2
        cleanup_status=1
      elif [[ $current_mount_identity != "$staging_mount_identity" ]]; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production tmpfs identity changed" >&2
        cleanup_status=1
      elif ! umount "$staging"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production tmpfs cleanup failed" >&2
        cleanup_status=1
      elif ! require_absent_tmpfs_mount "$staging" "$expected_mount_source"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production tmpfs remains after unmount" >&2
        cleanup_status=1
      fi
    elif (( cleanup_status == 0 )); then
      if ! mount_state="$(read_tmpfs_mount_state "$staging" "$expected_mount_source")"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production tmpfs state could not be read" >&2
        cleanup_status=1
      elif [[ $mount_state != "absent" ]]; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "refusing to unmount an unidentified production mount" >&2
        cleanup_status=1
      fi
    fi
    if (( cleanup_status == 0 )); then
      if ! require_absent_tmpfs_mount "$staging" "$expected_mount_source"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production mount absence could not be proved" >&2
        cleanup_status=1
      elif ! rm -rf -- "$scratch"; then
        printf 'native-shadow launcher-v2 successor v5: FAIL: %s\n' \
          "production scratch cleanup failed" >&2
        cleanup_status=1
      fi
    fi
  fi
  if (( primary_status != 0 )); then
    if (( cleanup_status != 0 )); then
      printf 'native-shadow launcher-v2 successor v5: cleanup also failed after primary status %s\n' \
        "$primary_status" >&2
    fi
    exit "$primary_status"
  fi
  exit "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
trap 'exit 129' HUP
mkdir -m 0700 "$staging"
mount -t tmpfs \
  -o "mode=0700,nodev,nosuid,size=${staging_tmpfs_size_bytes},nr_inodes=${staging_tmpfs_inodes}" \
  "$expected_mount_source" "$staging"
staging_mount_identity="$(capture_tmpfs_mount_identity "$staging" "$expected_mount_source")" \
  || die "the production tmpfs identity could not be captured"
if [[ $mode == "produce" ]]; then
  scratch_identity="$(stat -c %d:%i -- "$scratch")" \
    || die "the production scratch identity could not be read"
  staging_identity="$(stat -c %d:%i -- "$staging")" \
    || die "the production staging identity could not be read"
  parent_device=${outputs_parent_identity%%:*}
  parent_inode=${outputs_parent_identity#*:}
  if ! printf '%s' "$staging_mount_identity" | \
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" publish-recovery-record \
      --repository-root "$ROOT" \
      "${dispatch_context_argv[@]}" \
      --scratch "$scratch" \
      --scratch-device "${scratch_identity%%:*}" \
      --scratch-inode "${scratch_identity#*:}" \
      --staging-device "${staging_identity%%:*}" \
      --staging-inode "${staging_identity#*:}" \
      --outputs-parent "$outputs_parent" \
      --parent-device "$parent_device" \
      --parent-inode "$parent_inode" \
      --recovery-stem "$recovery_stem"; then
    die "the production recovery record could not be published"
  fi
fi
mkdir -m 0700 "$staging_preflight" "$staging_production"

gpgv_path="$(readlink -f "$(command -v gpgv)")"
zstd_path="$(readlink -f "$(command -v zstd)")"

if [[ $mode == "preflight" ]]; then
  preflight_argv=()
  while IFS= read -r item; do preflight_argv+=("$item"); done < <(isolation_prefix "$preflight_unit")
  preflight_argv+=(
    "--property=ReadWritePaths=$staging_preflight"
    "--property=ReadWritePaths=$result_parent" --
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" preflight
    "${dispatch_context_argv[@]}"
    --repository-root "$ROOT" --cas "$cas" --launcher "$launcher"
    --scratch "$staging_preflight" --gpgv "$gpgv_path" --zstd "$zstd_path"
    --result "$result"
  )
  "${preflight_argv[@]}"
  stop_and_verify_unit "$preflight_unit" \
    || die "the successful standalone preflight transient unit remains"
  exit 0
fi

if [[ $mode == "produce" ]]; then
  # Re-prove the same environment and assembly with no output surface first.
  preflight_argv=()
  while IFS= read -r item; do preflight_argv+=("$item"); done < <(isolation_prefix "$preflight_unit")
  preflight_argv+=(
    "--property=ReadWritePaths=$staging_preflight" --
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" preflight
    "${dispatch_context_argv[@]}"
    --repository-root "$ROOT" --cas "$cas" --launcher "$launcher"
    --scratch "$staging_preflight" --gpgv "$gpgv_path" --zstd "$zstd_path"
    --result "$staging_preflight/PREFLIGHT-RESULT.json"
  )
  "${preflight_argv[@]}" {recovery_lock_fd}>&-
  stop_and_verify_unit "$preflight_unit" \
    || die "the successful production preflight transient unit remains"
  # A successful preflight may contain a full extracted runtime tree.  Remove
  # it before the one-use production starts so both copies never occupy the
  # bounded tmpfs at the same time.
  rm -rf -- "$staging_preflight"
  [[ ! -e $staging_preflight && ! -L $staging_preflight ]] \
    || die "the successful preflight scratch was not removed"
  first_production_member="$(find "$staging_production" -mindepth 1 -maxdepth 1 -print -quit)"
  [[ -z $first_production_member ]] \
    || die "the production scratch is not exactly empty after preflight"

  # Re-resolve the live ref after preflight and freeze the exact guard-created
  # annotated tag bytes for the core's two no-path-reopen verifications.
  dispatch_tag_snapshot="$staging/DISPATCH-TAG-OBJECT"
  snapshot_and_verify_dispatch_claim "$dispatch_tag_snapshot"

  # The core owns output-directory creation and the durable attempt marker.
  # Only its parent is writable so systemd-run does not force the output to
  # exist before the core crosses its budget line.
  production_argv=()
  while IFS= read -r item; do production_argv+=("$item"); done < <(isolation_prefix "$production_unit")
  production_argv+=(
    "--property=ReadWritePaths=$staging_production"
    "--property=ReadWritePaths=$outputs_parent" --
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" produce
    --repository-root "$ROOT" --cas "$cas" --launcher "$launcher"
    --scratch "$staging_production" --gpgv "$gpgv_path" --zstd "$zstd_path"
    --outputs "$outputs"
    --result "$outputs/PRODUCE-RESULT-PENDING-READBACK-V5.json"
    --claim-ref "$claim_ref"
    --ref-object-sha "$ref_object_sha"
    --tag-object-sha "$expected_tag_object_sha"
    --github-run-id "$github_run_id"
    --github-run-attempt "$github_run_attempt"
    --event-name "$event_name"
    --dispatch-ref "$dispatch_ref"
    --workflow-ref "$workflow_ref"
    --workflow-path "$workflow_path"
    --head-sha "$head_sha"
    --head-authority-sha256 "$head_authority_sha256"
  )
  "${production_argv[@]}" {recovery_lock_fd}>&- < "$dispatch_tag_snapshot"
  stop_and_verify_unit "$production_unit" \
    || die "the successful production transient unit remains"

  # Qualification is a second transient unit.  It keeps the same network,
  # filesystem and no-new-privileges walls while exposing only the loop
  # devices and CAP_SYS_ADMIN needed for a private read-only mount.
  qualification_argv=()
  while IFS= read -r item; do qualification_argv+=("$item"); done < <(qualification_prefix "$qualification_unit")
  qualification_argv+=(
    "--property=ReadWritePaths=$outputs_parent" --
    /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" qualify
    "${dispatch_context_argv[@]}"
    --repository-root "$ROOT"
    --outputs "$outputs"
    --pending "$outputs/PRODUCE-RESULT-PENDING-READBACK-V5.json"
    --result "$result"
  )
  "${qualification_argv[@]}" {recovery_lock_fd}>&-
  stop_and_verify_unit "$qualification_unit" \
    || die "the successful qualification transient unit remains"

  # Publish the logical matrix provenance while the parent is still 0700, then
  # let one descriptor-based core operation revalidate the seven outputs and
  # provenance before it seals outputs 0555 and opens parent traversal last.
  recheck_dispatch_claim_ref
  parent_device=${outputs_parent_identity%%:*}
  parent_inode=${outputs_parent_identity#*:}
  /usr/bin/env -i PATH="$PATH" HOME=/nonexistent LANG=C LC_ALL=C \
    PYTHONDONTWRITEBYTECODE=1 PYTHONHASHSEED=0 \
    /usr/bin/python3 -I -S "$PRODUCER" publish-and-seal-replica \
      --repository-root "$ROOT" \
      --parent "$outputs_parent" \
      --parent-device "$parent_device" \
      --parent-inode "$parent_inode" \
      --outputs "$outputs" \
      --result "$outputs_parent/REPLICA-PROVENANCE.json" \
      --replica-ordinal "$replica_ordinal" \
      --strategy-job-index "$strategy_job_index" \
      --strategy-job-total "$strategy_job_total" \
      --github-job "$github_job" \
      --artifact-name "$artifact_name" \
      --claim-ref "$claim_ref" \
      --ref-object-sha "$ref_object_sha" \
      --tag-object-sha "$expected_tag_object_sha" \
      --github-run-id "$github_run_id" \
      --github-run-attempt "$github_run_attempt" \
      --event-name "$event_name" \
      --dispatch-ref "$dispatch_ref" \
      --workflow-ref "$workflow_ref" \
      --workflow-path "$workflow_path" \
      --head-sha "$head_sha" \
      --head-authority-sha256 "$head_authority_sha256" \
      < "$dispatch_tag_snapshot"
  recheck_dispatch_claim_ref
  collectability_armed="no"
  exit 0
fi

die "unreachable mode"
