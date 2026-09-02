#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'docs-smoke: missing required file: %s\n' "$path" >&2
    return 1
  fi
}

require_text() {
  local path="$1"
  local needle="$2"
  if ! grep -Fq -- "$needle" "$path"; then
    printf 'docs-smoke: missing %q in %s\n' "$needle" "$path" >&2
    return 1
  fi
}

forbid_text() {
  local path="$1"
  local needle="$2"
  if grep -Fq -- "$needle" "$path"; then
    printf 'docs-smoke: forbidden %q in %s\n' "$needle" "$path" >&2
    return 1
  fi
}

require_file README.md
require_file install.sh
require_file docs/install.md
require_file docs/proof-to-block-benchmark.md
require_file docs/local-ollama-benchmark.md
require_file docs/benchmarks/proof-to-block-v0.1-sample.md
require_file fixtures/benchmarks/proof-to-block-v0.1/sample-summary.json
require_file fixtures/benchmarks/proof-to-block-v0.1/sample-leaderboard.md
require_file docs/replay-consensus.md
require_file docs/settlement-report.md
require_file docs/receipt-commitment.md
require_file docs/verified-answer-local-mvp-closeout.md
require_file docs/dev-mock-payment.md
require_file docs/mac-first-hidden-linux-execution-plan-v1.md
require_file docs/development-throughput-and-evidence-policy-v1.md
require_text docs/development-throughput-and-evidence-policy-v1.md "BOOLE-DEVELOPMENT-THROUGHPUT-AND-EVIDENCE-V1"
require_text docs/development-throughput-and-evidence-policy-v1.md "TP8-CURRENT-AUTHORITY-BOUNDARY"
require_text docs/development-throughput-and-evidence-policy-v1.md "TP9-PROCESS-ONLY-CI"

# Historical Mac-first native-checker contract pins. These strings remain in
# the append-only record, but the current curl-first correction below controls
# product form. The no-user-managed-Docker/Linux boundary remains current.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "never require the user to install Docker Desktop"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "user-invisible Linux execution appliance"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.6 — Release gate"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Mac packaging planned"
require_text docs/native-submission-shadow-verification-v1.md "docs/mac-first-hidden-linux-execution-plan-v1.md"

# MAC.0/MAC.1 minimal contract (2026-08-24). Pins the MAC.0 completion record
# with its PR #221 crash/restart merge SHA and CI run, the frozen MAC.1 status,
# the not-yet-implemented Mac production boundary, and the BF.7 HOLD invariant,
# so none of them can silently rot or be silently upgraded to a product claim.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.0 status: COMPLETE"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "6553360a6291c300ad0d19c50238b8b7c9263c68"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "https://github.com/NotoriAndo/Boole/actions/runs/32709400913"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.1-PARTIAL — OPERATOR VALUE REQUIRED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Mac VM, the Mac production checker and every MAC.2+ gate remain NOT implemented"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BF.7=HOLD"
# The evidence-channel position.  Each of these is a claim that a later edit
# must not quietly soften: the mark is durable, the scan was run and did not
# settle the condition, the helper is designed rather than built, and the
# unprivileged half is the operator's to decide rather than mine to relax.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "ONE-USE-MARK  DURABLE / NOT CLAIMED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "SECRET-ABSENCE-SCAN  RUN / NOT SETTLED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "GUEST-EVIDENCE-HELPER  DESIGNED / NOT BUILT"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "UNPRIVILEGED-SUBMISSIONS  REPORTED / NOT DECIDED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CONSOLE-EVIDENCE-PRODUCER  DECIDED / NOT WRITTEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "GUEST-USERLAND  READ FROM THE SEALED SOURCE LOCK"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-FLOW-REHEARSAL  WALKED / NO MACHINE STARTED"
# The historical runtime-path zero belongs to builder v1.  The preserved v4
# image followed producer v2 into builder v3 and carries assembly evidence, but
# runtime launcher verification and boot remain unmeasured.
require_file native/containment/native-shadow-mac3-runtime-path-generation-correction-arm64-v1.json
require_text native/containment/native-shadow-mac3-runtime-path-generation-correction-arm64-v1.json 'CORRECTED-CURRENT-PATHS-PRESENT-RUNTIME-UNMEASURED'
require_text native/containment/native-shadow-mac3-runtime-path-generation-correction-arm64-v1.json '"imageAssemblyEstablished": true'
require_text native/containment/native-shadow-mac3-runtime-path-generation-correction-arm64-v1.json '"launcherRuntimeVerificationMeasured": false'
require_file scripts/test_native_shadow_mac3_runtime_path_generation_correction_arm64_v1.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_mac3_runtime_path_generation_correction_arm64_v1.py'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'RUNTIME-PATH GENERATION  CORRECTED'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER RUNTIME VERIFICATION  NOT MEASURED'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Runtime-path generation correction'
# The old whole-image scan remains exact raw-byte evidence, but a joined path
# is a graph of ext4 directory entries rather than a required contiguous byte
# string.  Keep the condition closed until the raw ranges, graph and sealed
# content table reconcile independently.
RAW_SCAN_CORRECTION=native/containment/native-shadow-mac3-guest-secret-absence-raw-scan-correction-arm64-v1.json
require_file "$RAW_SCAN_CORRECTION"
require_text "$RAW_SCAN_CORRECTION" 'RAW-SCAN-PATH-AND-ORIGIN-INFERENCES-FALSIFIED-CONDITION-NOT-SETTLED'
require_text "$RAW_SCAN_CORRECTION" 'PRESERVED-AS-RAW-BYTE-FACTS'
require_text "$RAW_SCAN_CORRECTION" '"joinedMultiComponentLogicalPathAbsence": "NOT-PROVEN"'
require_text "$RAW_SCAN_CORRECTION" '"hostOriginOrSecretLeakFromAnyRawHit": "NOT-PROVEN"'
require_text "$RAW_SCAN_CORRECTION" '"conditionSettled": false'
require_text "$RAW_SCAN_CORRECTION" '"bootAttemptsUsedByThisRecord": 0'
require_file scripts/test_native_shadow_mac3_guest_secret_raw_scan_correction_arm64_v1.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_mac3_guest_secret_raw_scan_correction_arm64_v1.py'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'Keeping the raw scan as an inventory, not a joined-path proof'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'PRESERVED-DISK PATH/CONTENT RECONCILIATION  NEXT'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'RAW-HIT HOST ORIGIN  NOT PROVEN'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Raw-scan joined-path correction'
# The preserved ext4 image is now reconciled path-by-path, byte-by-byte and
# block-owner-by-block-owner.  That closes the raw inventory methodology gap,
# but the sealed launcher contains its CI producer's build-home path, so the
# strict no-host-path condition remains NOT-SETTLED rather than being waived.
EXT4_SECRET_RECONCILIATION=native/containment/native-shadow-mac3-guest-secret-path-content-reconciliation-arm64-v1.json
require_file "$EXT4_SECRET_RECONCILIATION"
require_text "$EXT4_SECRET_RECONCILIATION" 'LOGICAL-PATH-CONTENT-AND-PHYSICAL-OWNER-RECONCILIATION-PASS-HOST-PATH-CONDITION-NOT-SETTLED'
require_text "$EXT4_SECRET_RECONCILIATION" '"reconciliationPassed":true'
require_text "$EXT4_SECRET_RECONCILIATION" '"conditionSettled":false'
require_text "$EXT4_SECRET_RECONCILIATION" '"rawHits":135'
require_text "$EXT4_SECRET_RECONCILIATION" '"attributedRawHits":135'
require_text "$EXT4_SECRET_RECONCILIATION" 'SEALED-LAUNCHER-BUILD-PROVENANCE-PATH-NOT-SECRET-MATERIAL-BUT-HOST-PATH-CONDITION-BLOCKER'
require_text "$EXT4_SECRET_RECONCILIATION" '"hostPathCriterionMet":false'
require_text "$EXT4_SECRET_RECONCILIATION" '"noHostWalletModelOrNodeSecretMaterialObserved":true'
require_text "$EXT4_SECRET_RECONCILIATION" '"bootAttempted":false'
require_text "$EXT4_SECRET_RECONCILIATION" '"mineableNow":0'
require_file scripts/native_shadow_ext4_readonly_owner_map_arm64_v1.py
require_file scripts/native_shadow_mac3_guest_secret_path_content_reconcile_arm64_v1.py
require_file scripts/test_native_shadow_ext4_readonly_owner_map_arm64_v1.py
require_file scripts/test_native_shadow_mac3_guest_secret_path_content_reconcile_arm64_v1.py
require_text scripts/self-test.sh 'native-shadow-ext4-secret-reconciliation'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'Reconciling every byte without hiding the producer build path'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'PRODUCER BUILD PATH  23 ATTRIBUTED / BLOCKING'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER V2 PATH REMAP  NEXT'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Preserved-disk reconciliation and producer build-path blocker'
require_text docs/native-submission-shadow-verification-v1.md 'Preserved ext4 reconciliation addendum'
require_text docs/native-submission-shadow-verification-v1.md 'conditionSettled=false'
require_text docs/native-submission-shadow-verification-v1.md "is now claimed and closed on"
require_text docs/native-submission-shadow-verification-v1.md "PR #221"
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "CRASH-RESTART-EXACTLY-ONCE-E2E: GREEN"

# MAC.1 closure contract (2026-08-24). Pins the operator-supplied support range
# (minimum macOS 14.0 on Apple Silicon M1 or later, Intel outside the v1 scope),
# the MAC.1 COMPLETE status, the MAC.2-not-started cursor, the not-ready Mac
# production checker boundary, and the mineable_now=0 invariant, so the closed
# contract can neither rot nor be silently upgraded to a Mac product claim.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.1 status: COMPLETE"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "macOS 14.0 (Sonoma)"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Apple Silicon (M1 or later)"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Intel Mac is not supported by v1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "The execution cursor moves to **MAC.2"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2 has NOT been started"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "The Mac production checker is NOT ready"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "mineable_now=0"

# MAC.1 accounting correction and MAC.2 Linux/arm64 closure (2026-08-25).
# Historical MAC.1-COMPLETE/MAC.2-NOT-STARTED pins above remain on purpose;
# these pins require a later append-only current-state correction and the
# executed arm64 authority evidence without granting Mac-product activation.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.1-PARTIAL — DISTRIBUTION MODE, PUBLIC IDENTITY, AND MEASUREMENT PROTOCOL REQUIRED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2 status: **MAC.2-PARTIAL — CLOSED-LOCAL LINUX/ARM64 AUTHORITY PARITY COMPLETE; STAGED VERIFIER AND POST-ADOPTION REVERIFICATION OPEN.**"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "POST-UPDATE-IMAGE-AND-RUNTIME-AUTHORITY-REVERIFICATION"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.3 is BLOCKED / NOT STARTED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.1-PARTIAL — DISTRIBUTION MODE, PUBLIC IDENTITY, AND MEASUREMENT PROTOCOL REQUIRED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-A — architecture authority parity"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-B — authenticated staged-update verification"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-C — post-adoption re-verification"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Measurement results are earned in MAC.2–MAC.5"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "PR #224"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "2a6de07ba6c77355d19a3d342ab718f7358fd76a"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "https://github.com/NotoriAndo/Boole/actions/runs/32766488279"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "d636e56dbf7e32d6054a1d4abfaeb97c6ebdf5119d217fe7740db0513984badd"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "df8be9eb7f3d92335d22b95a7e9423d8baaa2d581a2fd3b3633f60ae63db4e3f"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "666cdd6a6908822b35a3839e905ab03bed2846ce8e49091ccd163b5f59947f36"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "23b9c235a638cf08d38b2082af19d599320c9e5e5fc785bc1e14f51b4667f210"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "2962adef8d1aea9ba1c8466b8e014b71f1ec3c9555ce8b685d58ede6b631fe74"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "bd5cd9fc87e5e47a23e6fa12844ec0c47bdb01ee34090cddff24568c18d7236f"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "b7ef42d084adb8d660d7446092d768546cb555a868d2bbe7a5d6f4f9b1985d09"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "d220d20b7adaa22357929729d2f0666a8c9cbe50ce8031f90539ba1309950c6b"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "79073e541856c9be3bfbf56bf9c4415677679dc994c1342902f631716db7f312"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "62 artifacts / 56 Ubuntu packages / 181,623,999 bytes"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "sha256:dfeafb2918764736bdcd94d0fd121ed8eee2ef88d0a82e1ef28b3e625723bc0d"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "766,556,160 bytes"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "1,285,116 bytes"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '"authorityInputBytes": 181623999'

# MAC.1 operator decisions with no Team ID yet (2026-08-25). These pins keep
# the approved user-facing distribution choices distinct from the still-open
# Apple signing identity and production update trust root. A test key must
# never silently upgrade either missing production identity to COMPLETE.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.1-DECISIONS-FROZEN — TEAM-ID-AND-PRODUCTION-TRUST-ROOT-OPEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "first-run download of a verified Linux/arm64 guest"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Developer ID direct distribution"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "GitHub Releases"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "io.github.NotoriAndo.Boole"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Apple Developer Team ID is not available"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "No production private key is generated or stored by this slice"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-B-CORE may be tested with a non-production KAT key"
require_text docs/native-submission-shadow-verification-v1.md "TEAM-ID-AND-PRODUCTION-TRUST-ROOT-OPEN"
# MAC.2-B offline verifier core/KAT closure (2026-08-25). This is deliberately
# narrower than production update authorization: no production trust root,
# downloader, durable adoption or post-adoption execution is claimed.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-B-CORE/KAT GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "PR #226"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "fb7142d21129852847ff1ab6c19ca3deb9713692"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "production trust root remains absent"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-B production OPEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.3 BLOCKED"
require_text docs/native-submission-shadow-verification-v1.md "MAC.2-B-CORE/KAT GREEN"
require_text docs/native-submission-shadow-verification-v1.md "fb7142d21129852847ff1ab6c19ca3deb9713692"
require_text docs/native-submission-shadow-verification-v1.md "MAC.2-B production OPEN"
require_text docs/native-submission-shadow-verification-v1.md "MAC.3 BLOCKED"
# Curl-first distribution correction (2026-08-25). Historical Boole.app,
# Developer ID and Team-ID records stay in the append-only plan, but they must
# not remain the current product contract or block the next implementation.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL-FIRST-CLI-SERVICE-DISTRIBUTION — CURRENT"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Boole.app distribution decision is SUPERSEDED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Team ID is not a prerequisite for the curl-first path"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Bundle ID are not v1 requirements"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "notarization are optional future distribution hardening"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "curl installer → verified prebuilt macOS arm64 CLI and host controller"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "install-scoped host"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "512 MiB total host-payload cap"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.0  COMPLETE"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.1  OPEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.2-B-CORE/KAT GREEN remains valid"
# CURL.1 contract/verifier closure and boot-format freeze (2026-08-25, section 15). The
# historical "CURL.1  OPEN" cursor above stays in the append-only record; these pins require
# the successor state — frozen release contract, direct-Linux-boot decision, and the explicit
# not-bootable/no-installer/no-production-trust-root boundary — so the closed-local verifier
# result can never silently rot into an installer, release or production claim.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.1 status: **CONTRACT/VERIFIER GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "boole.curl-product-release.v1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "boole-curl-product-release-v1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "https://developer.apple.com/documentation/virtualization/vzlinuxbootloader"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "https://developer.apple.com/documentation/virtualization/vzefibootloader"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "bootFormatVersion=1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "NOT a bootable VM image"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.2  NOT STARTED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "no signed production release and no installer exist"
require_text docs/native-submission-shadow-verification-v1.md "CURL.1 CONTRACT/VERIFIER GREEN"
require_text docs/native-submission-shadow-verification-v1.md "boole.curl-product-release.v1"
require_text docs/native-submission-shadow-verification-v1.md "CURL-FIRST-CLI-SERVICE-DISTRIBUTION — CURRENT"
# CURL.2-CORE installer core closure (2026-08-25, section 16). The historical
# "CURL.2  NOT STARTED" cursor above stays in the append-only record; these pins
# require the successor state — verified atomic local adoption behind a durable
# fail-closed replay floor, with download/transport explicitly still absent — so
# the local installer core can never silently rot into a transport, release or
# production-installation claim.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.2-CORE status: **INSTALLER CORE GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "boole.curl-product-install-state.v1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "installed-release.json"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "fails closed with the on-disk evidence preserved"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.2-CORE  INSTALLER CORE GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.2-TRANSPORT  NOT STARTED"
require_text docs/native-submission-shadow-verification-v1.md "CURL.2-CORE INSTALLER CORE GREEN"
require_text docs/native-submission-shadow-verification-v1.md "boole.curl-product-install-state.v1"
# CURL.2-TRANSPORT closure (2026-08-25, section 17). The historical
# "CURL.2-TRANSPORT  NOT STARTED" cursor above stays in the append-only record;
# these pins require the successor state — a fail-closed download order in which
# transport signals (URL, HTTP status, file names) are never trust grounds and
# downloaded bytes live only in transient staging until the verified installer
# adopts them — so the closed-local loopback evidence can never silently rot
# into a real-release, production-trust-root or public-distribution claim.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.2-TRANSPORT status: **GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "aborts **before any"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "bounded by its signed \`byteLength\`"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "transient download staging directory that is never the"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.2-TRANSPORT  GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "No default trust root ships in the binary"
require_text docs/native-submission-shadow-verification-v1.md "CURL.2-TRANSPORT GREEN"
require_text docs/native-submission-shadow-verification-v1.md "Transport is never trust"
require_text docs/native-submission-shadow-verification-v1.md "Team ID is not a runtime authority"
# CURL.3-PREP freeze (2026-08-25, section 18). The canary acceptance grounds are
# frozen before any canary runs, and the cursor must keep saying the canary has
# NOT run. The load-bearing pin is the clean-machine rule: machine grounds are
# evaluated before any success signal, so a flawless run on this developer Mac
# can never be relabelled a clean-Mac pass, and pinned boot inputs must survive
# the reboot byte-identically rather than being re-fetched.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.3-PREP status: **CONTRACT FROZEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "A successful run on a developer machine can never be recorded as"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "the reboot must reuse byte-identical pins"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.3-PREP  CONTRACT FROZEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.3  NOT STARTED — no clean macOS 14/M1 host"
require_text docs/native-submission-shadow-verification-v1.md "CURL.3-PREP CONTRACT FROZEN — CANARY NOT RUN"
require_text docs/native-submission-shadow-verification-v1.md "a developer machine can never be recorded as a"
# CURL.3 environment deferral + bootable guest contract v2 (2026-08-26,
# section 19). The old clean-Mac canary is neither passed nor waived. It moves
# out of the implementation prerequisite chain only to break the circular
# dependency, and remains mandatory before clean-install/release claims. The
# successor contract must keep v1 exact-ten while binding exact-twelve bootable
# guest inputs under separate schemas/signing domains; real boot bytes and the
# v2 installer consumer remain absent.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "CURL.3 DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Circular prerequisite corrected"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "before MAC.5 clean-install acceptance, MAC.6 release readiness"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "guest-update v1 remains byte/meaning compatible and exact-ten"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "authenticates exactly twelve artifacts in a fixed order"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "current CURL.2 installer and transport still consume"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-ARTIFACT-BUILDER  NEXT"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC.3-CLOSED-LOCAL  UNBLOCKED / NOT STARTED"
require_text docs/native-submission-shadow-verification-v1.md "BOOTABLE GUEST CONTRACT V2 GREEN"
require_text docs/native-submission-shadow-verification-v1.md "Successor guest-update v2 has a separate schema/signing"
require_text docs/native-submission-shadow-verification-v1.md "CURL.2 installer/transport still consume v1"
# BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 (2026-08-26, section 20). This is an
# audit-only GREEN implementation while the real inputs remain fail-closed.
# It must bind the frozen systemd execution policy and must not revive the
# discarded static-PID-1 shortcut. Keep zero outputs/no boot claim separate
# from future builder or VM evidence.
require_file native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json
require_file scripts/native_shadow_boot_artifact_builder_arm64_v1.py
require_file scripts/test_native_shadow_boot_artifact_builder_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_artifact_builder_arm64_v1.py"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 = GREEN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "KERNEL/SYSTEMD-GUEST/IMAGE-BUILDER AUTHORITIES UNDEFINED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "NATIVE-SHADOW-BOOT-ARTIFACT-BUILD-PLAN-ARM64-V1-SCAFFOLD-NOT-ACTIVATABLE"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "REAL-BOOT-ARTIFACTS ="
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BLOCKED_MISSING_INPUTS"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "artifactsWritten=0"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "bootableClaim=false"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-GUEST-INIT-COMPATIBILITY-V1  NEXT"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-INPUT-AUTHORITY-V1  BLOCKED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "boole-native-shadow-launcher.service"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "static one-off PID 1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "permanently audit-only"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "successor plan/schema/tool"
require_text docs/native-submission-shadow-verification-v1.md "BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 GREEN"
require_text docs/native-submission-shadow-verification-v1.md "CURRENT INPUT READINESS BLOCKED_MISSING_INPUTS"
require_text docs/native-submission-shadow-verification-v1.md "REAL BOOT ARTIFACTS NOT PRODUCED"
require_text docs/native-submission-shadow-verification-v1.md "BOOT-GUEST-INIT-COMPATIBILITY-V1"
require_text docs/native-submission-shadow-verification-v1.md "static-PID-1 shortcut contradicted"
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json '"systemdGuestClosure"'
require_text docs/native-submission-shadow-verification-v1.md "DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED"
# BOOT-GUEST-INIT-COMPATIBILITY-V1 freezes the real systemd guest shape
# without reinterpreting the incomplete OCI source lock as a boot disk.
require_file native/containment/native-shadow-guest-init-compatibility-arm64-v1.json
require_file scripts/native_shadow_guest_init_compatibility_arm64_v1.py
require_file scripts/test_native_shadow_guest_init_compatibility_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_guest_init_compatibility_arm64_v1.py"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-GUEST-INIT-COMPATIBILITY-V1 (2026-08-26)"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'signedClosureVerified=false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'runtimeCompatibilityVerified=false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'authorityBoundaryVerified=false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Explicit replay-node binary, service and"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-INPUT-AUTHORITY-V1  NEXT"
require_text docs/native-submission-shadow-verification-v1.md "BOOT-GUEST-INIT-COMPATIBILITY-V1 CONTRACT"
require_text docs/native-submission-shadow-verification-v1.md "Explicit replay-node paths are rejected"
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"guestNodeAuthorityAllowed": false'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"rootDiskReadOnly": true'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"requiredPackageSeed": "systemd"'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"staticPid1Allowed": false'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"sourceShapeStatusIsSignedClosureEvidence": false'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"sourceShapeStatusIsRuntimeCompatibilityEvidence": false'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"sourceShapeStatusIsAuthorityBoundaryEvidence": false'
# BOOT-ROOTFS-DEPENDENCY-CANDIDATE-ARM64-V1 freezes signed-metadata selection
# only. Payload acquisition, image construction, VM boot and activation remain
# absent and CURL.3 remains an unpassed release gate.
require_file native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json
require_file native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json
require_file scripts/native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py
require_file scripts/test_native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "DEPENDENCY-CANDIDATE-FROZEN-NOT-BOOT-AUTHORITY"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "191 packages / 208,936,876 declared payload bytes"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "135 packages / 141,944,114 declared payload bytes"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-PAYLOAD-ACQUISITION/VERIFICATION  NEXT"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "REAL-BOOT-ARTIFACTS  NOT-PRODUCED"
require_text docs/native-submission-shadow-verification-v1.md "FROZEN-NOT-BOOT-AUTHORITY"
require_text docs/native-submission-shadow-verification-v1.md "Signed repository metadata replay is verified; package payload acquisition and verification are"
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"packagePayloadsAcquired": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"packagePayloadsVerified": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"maintainerScriptsExecuted": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"kernelImageExtracted": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"launcherElfPresent": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"imageBuilderAuthorityPresent": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"runtimeCompatibilityVerified": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json '"signedRepositoryMetadataVerified": true'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json '"productionByteProvenanceComplete": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json '"bootArtifactsWritten": 0'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json '"activationAllowed": false'
# BOOT-ROOTFS-PAYLOAD-ACQUISITION-ARM64-V1 promotes only exact package-byte
# acquisition/verification. Source-lock, image, runtime, boot and activation
# authority remain absent; Rust dist artifacts are explicitly out of scope.
require_file native/containment/native-shadow-boot-rootfs-payload-acquisition-plan-arm64-v1.json
require_file native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json
require_file scripts/native_shadow_boot_rootfs_payload_acquire_arm64_v1.py
require_file scripts/test_native_shadow_boot_rootfs_payload_acquire_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_rootfs_payload_acquire_arm64_v1.py"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "PACKAGE-PAYLOADS-ACQUIRED-VERIFIED-NOT-BOOT-AUTHORITY"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "records exactly 186 GETs"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Total network payload was 209,807,900 bytes"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-INPUT-AUTHORITY/SOURCE-LOCK SUCCESSOR  NEXT"
require_text docs/native-submission-shadow-verification-v1.md "43becf01889f8ca5b4fc9acff20b95b12ef78f3736dd13c9081001c5110aac2a"
require_text docs/native-submission-shadow-verification-v1.md "cb4d6bc0f85d2dead1fbae20d9dcebcc3310e734d9a2d1937855997ae22b61ea"
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"packagePayloadsAcquired": true'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"packagePayloadsVerified": true'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"baselineFetched": 51'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"deltaFetched": 134'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"metadataFetched": 1'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"maintainerScriptsExecuted": false'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"runtimeCompatibilityVerified": false'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"productionByteProvenanceComplete": false'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"bootArtifactsWritten": 0'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json '"activationAllowed": false'
# BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1 (2026-08-26, section 24). The successor lock
# seals the 191 verified package rows together with the guest-init deployment
# bytes, and nothing more. The load-bearing pin is the deferred launcher binary:
# the guest placement is bound, but its digest belongs to a build that has not
# run, so the audit reports the gap instead of inventing a digest. A lock that
# ever reads SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED would still be source
# shape only — never runtime compatibility, boot authority or boot success.
require_file native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json
require_file native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json
require_file native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json
require_file native/etc/machine-id
require_file native/sysusers.d/boole-native-shadow.conf
require_file native/tmpfiles.d/boole-native-shadow.conf
require_file scripts/native_shadow_boot_rootfs_source_lock_arm64_v1.py
require_file scripts/test_native_shadow_boot_rootfs_source_lock_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_rootfs_source_lock_arm64_v1.py"
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"status": "BOOT-ROOTFS-SOURCE-LOCK-SEALED-LAUNCHER-BINARY-DEFERRED-NOT-BOOT-AUTHORITY"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"role": "tracked-file:launcher-binary"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"launcherElfPresent": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"imageBuilderAuthorityPresent": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"kernelImageExtracted": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"maintainerScriptsExecuted": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"runtimeCompatibilityVerified": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"guestBootVerified": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"bootArtifactsWritten": 0'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v1.json '"activationAllowed": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json '"digestBoundary": "deferred-to-arm64-launcher-build-authority"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json '"activationAllowed": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1  SEALED — 191 verified package rows"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "a digest cannot be stated for a file that does not exist"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "ARM64 RUST/LAUNCHER/IMAGE-BUILDER INPUT AUTHORITY  NEXT"
require_text docs/native-submission-shadow-verification-v1.md "9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf"
require_text docs/native-submission-shadow-verification-v1.md "sealing a source lock is not a boot claim"
# BOOT-RUSTDIST-ACQUISITION-ARM64-V1 pre-registration. The three ARM64 Rust
# archives are declared — exact URL, size and SHA-256 carried unchanged from the
# merged runtime acquisition plan — *before* a single byte is fetched, so a
# later result cannot quietly widen what was requested. The transport stays
# fail-closed: one exact HTTPS request per artifact, no proxy, no redirect, no
# retry, no Range, no parallelism. Holding archive bytes installs no toolchain,
# builds no launcher and boots nothing; every boundary stays false.
require_file native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json
require_file scripts/native_shadow_boot_rustdist_acquire_arm64_v1.py
require_file scripts/test_native_shadow_boot_rustdist_acquire_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_rustdist_acquire_arm64_v1.py"
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"allowEnvironmentProxy": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"allowRangeRequests": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"allowRedirects": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"allowRetries": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"concurrency": 1'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"minimumTlsVersion": "TLSv1.2"'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"requireCertificateValidation": true'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"requireContentLengthMatch": true'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"requireHostnameValidation": true'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"ci-artifacts.rust-lang.org"'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"launcherElfBuilt": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"toolchainInstalled": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"reproducibleBuildProven": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"activationAllowed": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json '"totalBytes": 112995148'

# The acquisition result records what was actually fetched against that frozen
# request. It carries the plan digest so the pair cannot drift apart, and it
# keeps every downstream boundary false: verified bytes in a content-addressed
# store are not an installed toolchain, not a launcher ELF, not a reproducible
# build and not a boot authority.
require_file native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"planSha256": "8ee39ab4c828c31bdd82bf8da12546d9b6595aeac8e6e9f4da9899eaacf0accc"'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"status": "RUSTDIST-PAYLOADS-ACQUIRED-VERIFIED-NOT-TOOLCHAIN-AUTHORITY"'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"verifiedCount": 3'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"fetchedBytes": 112995148'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"toolchainInstalled": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"launcherElfBuilt": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"reproducibleBuildProven": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"runtimeCompatibilityVerified": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rustdist-acquisition-result-arm64-v1.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "Acquiring verified bytes is not an installed toolchain"

# NATIVE-SHADOW-LAUNCHER-BUILD-ARM64-V1. The guest source lock defers exactly one
# role, the launcher binary, because a digest cannot be stated for a file that does
# not exist. This authority fixes every input that decides those bytes and requires
# two independent builds to agree. Determinism is declared, never manufactured:
# --remap-path-prefix is written into the recipe in the open, and nothing suppresses
# a timestamp. The build toolchain is the workspace channel, NOT the rust-lang-ci
# nightly acquired for the guest checker -- conflating them would misattribute the
# launcher's provenance, so byte provenance stays explicitly unclosed.
require_file native/containment/native-shadow-launcher-build-authority-arm64-v1.json
require_file scripts/native_shadow_launcher_build_arm64_v1.py
require_file scripts/test_native_shadow_launcher_build_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_launcher_build_arm64_v1.py"
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"rustTarget": "aarch64-unknown-linux-gnu"'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"channel": "1.95.0"'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"byteProvenanceClosed": false'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"independentBuildCount": 2'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"artifactMustBeByteIdentical": true'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"forbidTimestampSuppression": true'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"mismatchAction": "report-the-difference-never-force-a-match"'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"sourceTreeOrigin": "git-archive-of-tracked-files-only"'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"path": "scripts/native_shadow_launcher_build_arm64_v1.py"'
require_text .github/workflows/ci.yml 'native-shadow-launcher-build-arm64'
require_text .github/workflows/ci.yml 'python3 scripts/native_shadow_launcher_build_arm64_v1.py --build'
require_text docs/native-submission-shadow-verification-v1.md "A byte-identical pair of builds is not a boot"
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"guestImageBuilt": false'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"launcherDeployedIntoGuest": false'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-launcher-build-authority-arm64-v1.json '"activationAllowed": false'

# The artifact that authority describes, now sealed by the arm64 CI job that is the
# only place the double build can run. Pinning the launcher digest here is what turns
# the build step from "seal whatever comes out" into "re-prove these exact bytes": a
# later run that produces something else fails instead of quietly agreeing with itself.
require_file native/containment/native-shadow-launcher-build-result-arm64-v1.json
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"status": "LAUNCHER-ELF-BUILT-BYTE-IDENTICAL-NOT-BOOT-AUTHORITY"'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"sha256": "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434"'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"sizeBytes": 2006632'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"guestLogicalPath": "/usr/libexec/boole/boole-native-shadow-launcher"'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"authoritySha256": "64f4ea0c6b574e1479e51a78e250da8fac6f3d3522d60cb03dde65b53da594ee"'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"independentBuildCount": 2'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"guestImageBuilt": false'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"launcherDeployedIntoGuest": false'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"runtimeCompatibilityVerified": false'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-launcher-build-result-arm64-v1.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "Two byte-identical builds are a reproducibility result, not a running program"

# The guest image builder input authority. The rootfs stage already owns the step
# that produces an OCI layout; nothing owned the step that turns that layout into a
# kernel, an initrd and an ext4 root disk. This authority fixes that step's inputs.
# Its whole point is that no tool is taken from PATH: each executable is a member of
# an Ubuntu package the source lock already froze by digest, so "which mke2fs" has
# one answer a different build machine cannot change. mkfs.ext4 is a symlink to
# mke2fs, so the role pins mke2fs itself -- pinning the symlink would let an upstream
# rename repoint the tool without moving the digest. The kernel ships gzip-compressed
# and Apple's VZLinuxBootLoader wants a raw arm64 Image, so both digests are recorded
# and the decompression step is declared rather than discovered later.
require_file native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json
require_file scripts/native_shadow_boot_image_builder_authority_arm64_v1.py
require_file scripts/test_native_shadow_boot_image_builder_authority_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_image_builder_authority_arm64_v1.py"
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"format": "initrd-ext4-builder-authority-v1"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"memberPath": "./usr/sbin/mke2fs"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"role": "ext4-image-writer"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"memberPath": "./boot/vmlinuz-6.8.0-31-generic"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"compression": "gzip"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"forbidHostPathLookup": true'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"forbidMaintainerScripts": true'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"forbidLatestVersionSelection": true'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"forbidNetworkDuringBuild": true'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"forbidProductionSigningMaterial": true'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"forbidSymlinkToolPins": true'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"independentBuildCount": 2'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"ownership": "root:root-only"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"machineId": "empty-file-first-boot"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"path": "scripts/native_shadow_boot_image_builder_authority_arm64_v1.py"'
# The sealed scaffold plan named this exact format for its imageBuilderToolchain
# input and left the digest null. The two format strings have to keep agreeing, or
# this authority is answering a slot nothing asked for. The scaffold itself stays
# untouched -- its null digest is filled by a successor plan, never by editing it.
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json '"format": "initrd-ext4-builder-authority-v1"'
require_text docs/native-submission-shadow-verification-v1.md "Pinning the inputs of an image is not an image"
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"guestImageBuilt": false'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"kernelImageExtracted": false'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"rootDiskBuilt": false'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"initrdBuilt": false'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"activationAllowed": false'

# The first real boot artifact: the guest kernel image. The builder authority above
# pinned the kernel as a gzip member inside a frozen Ubuntu package; this step turns
# that pin into bytes on disk and proves the bytes are the pinned ones. Two things
# are worth pinning here. First, the arm64 check reads the magic at offset 0x38 (56)
# where the kernel header defines it -- searching the file for "ARM\x64" would also
# match an x86 image that happens to contain those bytes, so the offset is the test.
# Second, the extraction runs twice in independent temp directories and the digests
# must agree; decompression has no freedom to differ, so that second run rules out
# state leaking between runs rather than proving compiler reproducibility.
# CI cannot re-prove this result the way it re-proves the launcher build: the package
# bytes live in the gitignored content store, so the runner has never seen them.
require_file native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json
require_file scripts/native_shadow_boot_kernel_extract_arm64_v1.py
require_file scripts/test_native_shadow_boot_kernel_extract_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_kernel_extract_arm64_v1.py"
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"status": "KERNEL-IMAGE-EXTRACTED-REPRODUCIBLY-NOT-BOOT-AUTHORITY"'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"sha256": "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"sizeBytes": 57860488'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"architecture": "aarch64"'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"magicOffset": 56'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"name": "guest-kernel"'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"sha256": "f67ad535a1b19295985d0266394d1c3a5620178a3ba61aca22cda1b6c1e27a2a"'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"independentExtractionCount": 2'
# The document binds itself to the image builder authority that pinned the kernel.
# If that authority is ever superseded, this digest stops matching and the successor
# has to say so out loud instead of inheriting the claim silently.
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"authoritySha256": "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e"'
# Exactly one boundary flips true. A kernel is not an image, and the remaining six
# stay false until something actually produces and runs one.
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"kernelImageExtracted": true'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"guestImageBuilt": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"initrdBuilt": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"rootDiskBuilt": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"launcherDeployedIntoGuest": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"runtimeCompatibilityVerified": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "A kernel image is a file the boot loader can read, not a system that has booted"

# The systemd guest closure audit -- the third and last null slot in the plan
# scaffold. It answers one question from files alone: would PID 1 be real systemd,
# and would systemd start the launcher? Both halves are chains of file facts, and
# the pins below hold each link. The init symlink is the interesting one: Ubuntu
# 24.04 is usr-merged, so systemd-sysv ships /usr/sbin/init rather than /sbin/init,
# and its target is RELATIVE. Resolving that against the link's own directory is
# part of the audit -- reading the target string would accept a link that lands
# anywhere. A target climbing above the root is refused rather than clamped.
require_file native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json
require_file scripts/native_shadow_boot_systemd_closure_arm64_v1.py
require_file scripts/test_native_shadow_boot_systemd_closure_arm64_v1.py
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_systemd_closure_arm64_v1.py"
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"status": "SYSTEMD-GUEST-CLOSURE-AUDITED-NOT-BOOT-AUTHORITY"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"closureFormat": "systemd-rootfs-closure-authority-v1"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json '"format": "systemd-rootfs-closure-authority-v1"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"initLinkPath": "/usr/sbin/init"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"initLinkTarget": "../lib/systemd/systemd"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"initLinkResolvesTo": "/usr/lib/systemd/systemd"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"initLinkProvidedBy": "systemd-sysv"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"pid1Path": "/usr/lib/systemd/systemd"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"pid1ProvidedBy": "systemd"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"pid1Sha256": "ab970cc6f829555cad7e6891823b9c82b02f277b8fae081b7072b05e94f23f90"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"pid1Machine": "aarch64"'
# systemd being installed is not the same as systemd being PID 1. systemd-sysv is
# the package that makes the init symlink exist, so its absence would turn the
# claim back into an assumption.
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"name": "systemd-sysv"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"version": "255.4-1ubuntu8"'
# Enablement has to agree with what the unit itself asks for. A unit symlinked
# into the wrong target's wants directory is present and never starts.
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"wantedBy": "multi-user.target"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"execStart": "/usr/libexec/boole/boole-native-shadow-launcher"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"enablementTarget": "/usr/lib/systemd/system/boole-native-shadow-launcher.service"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"machineIdEmpty": true'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"replayNodeReferences": []'
# The two evidence tiers stay separate and each says which it is. Averaging them
# into one boolean would let the package half borrow the lock half's credibility.
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"reproducibleInCi": true'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"reproducibleInCi": false'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"systemdGuestClosureAudited": true'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"guestBootVerified": false'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"guestImageBuilt": false'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"rootDiskBuilt": false'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"bootAuthority": false'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "An audited closure is a set of file facts, not a system that has started"

# 2026-08-26n -- successor boot artifact build plan (arm64 v2).
# The audit-only v1 preflight refuses any plan whose three authority slots carry
# a digest and says so in its own words: use a successor plan/schema/tool. This
# is that successor, so the v1 scaffold must stay exactly as it is.
require_file scripts/native_shadow_boot_artifact_plan_arm64_v2.py
require_file scripts/test_native_shadow_boot_artifact_plan_arm64_v2.py
require_file native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_artifact_plan_arm64_v2.py"
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"schema": "boole.native-shadow.boot-artifact-build-plan.arm64.v2"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"release": "NATIVE-SHADOW-BOOT-ARTIFACT-BUILD-PLAN-ARM64-V2-RESOLVED-NOT-ACTIVATABLE"'
# The scaffold keeps its own v1 schema and its null slots. If either changes the
# v1 preflight starts rejecting it.
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json '"schema": "boole.native-shadow.boot-artifact-build-plan.arm64.v1"'
require_text scripts/native_shadow_boot_artifact_plan_arm64_v2.py 'use a successor plan/schema/tool'
# Three resolved slots. Two pin an authority document, one pins raw image bytes.
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"format": "initrd-ext4-builder-authority-v1"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"sha256": "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"sizeBytes": 4714'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"format": "linux-arm64-image"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"sha256": "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"sizeBytes": 57860488'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"format": "systemd-rootfs-closure-authority-v1"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"sha256": "9bcc1819fa406ef0479b8c200231d08a66023477d57a1cd3ade6637968ea8501"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"sizeBytes": 2529'
# The two documents do not agree on what to call the field that declares their
# format. A reader that tried one name and fell back to the other would accept
# either document in either slot, so the key is pinned per slot.
require_text scripts/native_shadow_boot_artifact_plan_arm64_v2.py '"imageBuilderToolchain": "format"'
require_text scripts/native_shadow_boot_artifact_plan_arm64_v2.py '"systemdGuestClosure": "closureFormat"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"format": "initrd-ext4-builder-authority-v1"'
require_text native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json '"closureFormat": "systemd-rootfs-closure-authority-v1"'
# Resolving inputs is not building and not booting.
require_text scripts/native_shadow_boot_artifact_plan_arm64_v2.py '"BOOT-INPUT-AUTHORITIES-RESOLVED-NOT-BOOT-AUTHORITY"'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "A resolved input is a pinned digest, not a built image"

# 2026-08-26o -- frozen producer authority for the arm64 CI image build (v2).
# The v1 builder authority left sourceDateEpoch null and could not say where the
# build would run. This successor states the rest and is frozen BEFORE anything
# is produced; the v1 document keeps its sealed digest.
require_file scripts/native_shadow_boot_image_producer_authority_arm64_v2.py
require_file scripts/test_native_shadow_boot_image_producer_authority_arm64_v2.py
require_file native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json
require_text scripts/self-test.sh "scripts/test_native_shadow_boot_image_producer_authority_arm64_v2.py"
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"schema": "boole.native-shadow.boot-image-producer-authority.arm64.v2"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"release": "NATIVE-SHADOW-BOOT-IMAGE-PRODUCER-AUTHORITY-ARM64-V2"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"format": "initrd-ext4-producer-authority-v2"'
# The sealed v1 authority is pinned, not copied. Its tool digests must not be
# restated here -- two copies of one fact can drift invisibly.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"sha256": "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e"'
forbid_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json "763be3ec03774647799b1186d30b4b524e6e73dd27be01cbe0be4b6043f62cb1"
forbid_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json "2c0bf348d91f9b3bd6eec6666b9897b9f733c430e6baa8066bd70b645b2ca023"
# The two slots v1 deliberately left open.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"sourceDateEpoch": 0'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"sourceDateEpoch": null'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"hostToolPinning": "record-at-build-time"'
# No network during the produce phase is enforced by the kernel, not promised.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"PrivateNetwork=yes"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"RestrictAddressFamilies=AF_UNIX"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"runner": "ubuntu-24.04-arm"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"separateJobs": true'
# A determinism mismatch is a hard stop, never a knob turned down.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"id": "independent-builds-differ"'
# Maintainer scripts in the frozen packages are normal (262 of them); the abort
# is one reaching the assembled tree. Wording it as the consumed set would stop
# every run that ever starts.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"id": "maintainer-script-copied-into-tree"'
forbid_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json "consumed set"
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"id": "package-path-collision"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"relaxKnobAllowed": false'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"mismatchAction": "report-the-difference-never-force-a-match"'
# The launcher is rebuilt and matched against the seal, never received as a handoff.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"acquisition": "rebuild-and-match-seal"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"sha256": "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434"'
# Images stay out of git and out of releases.
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"commitImagesToGit": false'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"uploadToRelease": false'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"keep": "ci-artifact-and-sha256-manifest"'
# Freezing a contract is not a build.
require_text scripts/native_shadow_boot_image_producer_authority_arm64_v2.py '"IMAGE-PRODUCER-AUTHORITY-FROZEN-NOTHING-PRODUCED"'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "A frozen contract is a promise about a build, not the build"

# The initrd writer holds the shape v1 froze: newc, uncompressed, canonical
# mtime, root-only. Inode numbers are archive positions -- a host inode would
# differ between the two independent jobs and fail the byte comparison for a
# reason that has nothing to do with the image.
require_file scripts/native_shadow_boot_initrd_arm64_v1.py
require_file scripts/test_native_shadow_boot_initrd_arm64_v1.py
require_text scripts/native_shadow_boot_initrd_arm64_v1.py 'MAGIC = b"070701"'
require_text scripts/native_shadow_boot_initrd_arm64_v1.py 'COMPRESSION = "none"'
require_text scripts/native_shadow_boot_initrd_arm64_v1.py "CANONICAL_MTIME = 0"
require_text scripts/native_shadow_boot_initrd_arm64_v1.py "BOOTABLE_CLAIM = False"
require_text scripts/native_shadow_boot_initrd_arm64_v1.py "ACTIVATION_ALLOWED = False"
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"initrdCompression": "none"'
require_text native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json '"fileOrder": "sorted-by-logical-path-bytes"'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_initrd_arm64_v1.py

# The root disk plan pins the knobs the writer it now runs actually reads, and
# the two time knobs are not interchangeable. What was pinned here was true of
# the frozen 1.47.0 writer, which has no SOURCE_DATE_EPOCH at all: back then
# E2FSPROGS_FAKE_TIME was the only knob there was. The selected build reads
# SOURCE_DATE_EPOCH first and arms the flag mke2fs branches on, and keeps
# E2FSPROGS_FAKE_TIME as a fallback that sets the time and leaves that flag
# clear -- so the superseded name is pinned as superseded rather than dropped.
# Setting it would look correct and rebuild the sealed failure.
require_file scripts/native_shadow_boot_root_disk_arm64_v1.py
require_file scripts/test_native_shadow_boot_root_disk_arm64_v1.py
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'WRITER_TIME_ENV = "SOURCE_DATE_EPOCH"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'SUPERSEDED_WRITER_TIME_ENV = "E2FSPROGS_FAKE_TIME"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'STAGING_FILESYSTEM = "tmpfs"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'EXT4_UUID = "00000000-0000-4000-8000-000000000001"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'EXT4_HASH_SEED = "00000000-0000-4000-8000-000000000002"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py "BOOTABLE_CLAIM = False"
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py "ACTIVATION_ALLOWED = False"
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py '"onMismatch": "abort-never-relax"'
forbid_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'SOURCE_DATE_EPOCH": "0"'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_root_disk_arm64_v1.py

# Two replicas built that disk from identical inputs and disagreed. The hard
# stop is the sealed failure; the successor is the bar the fix must clear,
# written before the fix exists. Both must keep running -- a record whose tests
# are not wired into CI is a record nothing is checking.
require_file native/containment/native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json
require_file native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json
require_text scripts/self-test.sh scripts/test_native_shadow_boot_root_disk_determinism_arm64_v1.py
require_text scripts/self-test.sh scripts/test_native_shadow_boot_root_disk_determinism_successor_arm64_v1.py

# The successor may not soften what it inherits: byte identity stays the
# criterion, the filesystem check stays read-only with one accepted exit code,
# and a mismatch stays a stop rather than another roll of the dice.
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"criterion": "byte identity, unchanged from the predecessor"'
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"criterionRelaxationForbidden": true'
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"acceptedExitCodes"'
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"onMismatch": "HARD STOP; report; do not produce a third image"'
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"successorValue": "1"'
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"activationAllowed": false'

# The fix itself. Zero is the library's unset sentinel, so the writer is handed
# a fixed non-zero time; the staged inputs keep their own epoch, which is a
# different thing and stays zero. The checker is forced and read-only, and none
# of the repair flags may reappear in its argv.
require_text scripts/self-test.sh scripts/test_native_shadow_boot_root_disk_determinism_fix_arm64_v1.py
require_file scripts/native_shadow_boot_root_disk_time_audit_arm64_v1.py
forbid_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'EXT4_WRITER_TIME = "0"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'EXT4_WRITER_TIME = "1"'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'E2FSCK_ARGV_OPTIONS = ("-f", "-n")'
require_text scripts/native_shadow_boot_root_disk_arm64_v1.py 'E2FSCK_ACCEPTED_EXIT_CODES = (0,)'
require_text scripts/native_shadow_boot_root_disk_execute_arm64_v1.py 'def assert_loader_evidence('
require_text scripts/native_shadow_boot_root_disk_execute_arm64_v1.py 'def assert_writer_time('
require_text scripts/native_shadow_boot_produce_phase_arm64_v1.py '"rootDiskEvidence": root_disk_evidence(disk_result)'

# Handing the writer a non-zero time is only half of it: the frozen writer
# cannot honour it at all, because it overwrites each staged file's i_ctime
# from a field userspace cannot set. So a different writer was chosen, and the
# two records that make that choice evidence are pinned here. The first was
# written while no deb had been fetched, which is the only reason its rule is a
# rule; the second applied that rule by reading the binaries rather than running
# them, and had to fail a control to have decided anything.
require_file native/containment/native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json
require_file native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json
require_text native/containment/native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json '"debsFetchedSoFar": 0'
require_text native/containment/native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json '"staticOnly": true'
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"establishedByRunningTheBinary": false'
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"verdict": "FIXED"'
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"verdict": "DEFECT"'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_e2fsprogs_candidate_preregistration_arm64_v1.py
require_text scripts/self-test.sh scripts/test_native_shadow_boot_e2fsprogs_selection_plucky_arm64_v1.py

# The writer is an addition and never a substitution. The 191 packages the guest
# is built from do not move, and the image inspector and the read-only checker
# stay on the build that did not write the image.
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"replacesAGuestPackage": false'
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"replacedByTheSelection": false'
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"count": 191'
require_text native/containment/native-shadow-boot-e2fsprogs-selection-plucky-arm64-v1.json '"deleted": false'

# The two scripts that put that writer on the runner: one fetches the sealed
# pair beside the frozen closure, the other unpacks it into a tree of its own.
# They are the newest things in the production run that reach the network, and
# they decide which bytes write the image.
require_file scripts/native_shadow_boot_writer_set_acquire_arm64_v1.py
require_file scripts/native_shadow_boot_writer_tree_arm64_v1.py
require_text scripts/self-test.sh scripts/test_native_shadow_boot_writer_set_acquire_arm64_v1.py
require_text scripts/self-test.sh scripts/test_native_shadow_boot_writer_tree_arm64_v1.py

# The sealed record names one open cause and forbids dispatching against it. It
# is not edited to agree with what the two records above later found -- it was
# written when the frozen writer was the only writer, and against that writer
# the cause is open and stays open. What clears it is a derivation from those
# records, keyed on the cause by name, so a second cause or a renamed one has no
# clearance and still refuses.
require_text scripts/native_shadow_boot_produce_phase_arm64_v1.py 'STAGED_CTIME_BLOCKER = "staged-inode-ctime-is-not-fs-now"'
require_text scripts/native_shadow_boot_produce_phase_arm64_v1.py 'BLOCKER_CLEARANCES = {STAGED_CTIME_BLOCKER: assert_staged_ctime_cause_removed}'
require_text native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json '"staged-inode-ctime-is-not-fs-now"'
require_text native/containment/native-shadow-boot-e2fsprogs-candidate-preregistration-arm64-v1.json '"unblocksOnlyOnAPassingStaticRead": true'

# The result of that one production pair. It is pinned by the digest the two
# replicas converged on rather than by the file's own name, so a later edit that
# keeps the name and changes the answer does not pass. The two boundaries below
# are pinned for the same reason a green result needs them most: two identical
# images say the writer is deterministic and say nothing about whether the guest
# boots.
require_file native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json
require_text native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json '"sha256": "9834036f7738f3848fff23e5c3d1be85cd1f288f7ca43d2094b815eca2b378cc"'
require_text native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json '"runId": "33045285925"'
require_text native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json '"activationAllowed": false'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_root_disk_determinism_green_arm64_v1.py

# The MAC.3 boot qualification, frozen before the one attempt it allows. The
# three pins below are the ones a run that went badly would be tempted to move:
# the count of allowed attempts, the read-only attachment of the sealed image,
# and the statement that nothing here is a claim that the guest boots. Pinning
# them in the gate means changing them fails on every push rather than quietly
# on the day it matters.
require_file native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json '"runsAllowed": 1'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json '"rootDiskAttachedReadOnly": true'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v1.json '"activationAllowed": false'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v1.py

# The successor qualification, frozen before the one attempt it opens against
# the rebuilt image. The first attempt failed and its allowance is spent; the
# pins below are the ones a successor would be tempted to soften into a second
# try at the same attempt -- its own single allowance, the count already spent
# on the first, and the statement that reopening it is not what this is.
require_file native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"runsAllowed": 1'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"runsPerformed": 0'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"resetsTheSpentAttempt": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"reusesTheSpentAttempt": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"rootDiskAttachedReadOnly": true'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"bootableClaim": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json '"activationAllowed": false'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v2.py

# The host that performs it. It is a development-Mac program that CI cannot run,
# so what the gate holds is its contract: no network device, no shared
# directory, the image opened read-only, and an ad-hoc signature carrying the
# one entitlement virtualization needs. A Team ID or a Developer ID certificate
# appearing here would be a release identity on a closed-local run.
require_file native/mac3/boole-mac3-closed-local-boot.swift
require_file native/mac3/boole-mac3-closed-local-boot.entitlements
require_text native/mac3/boole-mac3-closed-local-boot.swift 'configuration.networkDevices = []'
require_text native/mac3/boole-mac3-closed-local-boot.swift 'configuration.directorySharingDevices = []'
require_text native/mac3/boole-mac3-closed-local-boot.swift 'readOnly: true'
require_text native/mac3/boole-mac3-closed-local-boot.entitlements 'com.apple.security.virtualization'
forbid_text native/mac3/boole-mac3-closed-local-boot.entitlements 'com.apple.developer.team-identifier'
require_file scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'CODESIGN_IDENTITY = "-"'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'RUNS_ALLOWED = 1'

# The result of that one attempt. It did not pass, and the verdict is pinned
# here in the same words the record uses, so softening it later fails the gate
# rather than passing quietly. The attempt is also pinned as spent: the driver
# reads this record before it will start anything, which is why a wiped scratch
# directory cannot buy a second run.
require_file native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json '"verdict": "FAIL"'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json '"runsPerformed": 1'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json '"rerunPermitted": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json '"guest-systemd-is-pid-1"'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'assert_no_run_has_been_spent(sealed_result_path(attempt))'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'return SEALED_RESULT_PATH'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_closed_local_boot_result_arm64_v1.py

# The successor attempt is selected, never assumed, and selecting one is not the
# same as reopening the other. Each attempt seals to the path its own record
# names, so the receipt that records the first failure is neither overwritten
# nor read as the second attempt's; the conditions are compared against the
# first attempt's file before a machine is built, so a reworded bar fails here
# rather than passing as a successor; and the closed-machine properties are
# refused up front as well as read back off the host afterwards.
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-2'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'def assert_attempt_identity'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'def assert_conditions_are_not_relaxed'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'def assert_isolation_is_closed'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'assert_conditions_are_not_relaxed(record)'
require_text scripts/native_shadow_mac3_closed_local_boot_arm64_v1.py 'assert_isolation_is_closed(record)'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_closed_local_boot_successor_driver_arm64_v2.py

# The successor attempt ran once and passed. A pass is easier to inflate than a
# failure, so what is pinned here is mostly its size: the attempt is spent and
# not rerunnable, the launcher is recorded as started and explicitly not as
# serving, where it refused is recorded as unobserved rather than guessed, and
# every boundary a boot does not move stays false. CURL.3 stays not passed and
# activation stays disallowed in the same words the frozen record used.
require_file native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"verdict": "PASS"'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"runsPerformed": 1'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"rerunPermitted": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"launcherServing": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"cleanMacEvidence": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"productRelease": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"whereItRefused": "not observable from this run"'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json '"activationAllowed": false'
require_text native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json 'DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_closed_local_boot_result_arm64_v2.py

# The MAC.3 guest runtime contract, frozen before anything answers it. What is
# pinned here is the half that a later wave would be tempted to soften: the
# record reads as unrun, it claims no serving, and the one condition the sealed
# containment contradicts stays held rather than reworded into one the current
# design happens to satisfy. A new image is named as required and not as made.
require_file native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json 'MAC3-GUEST-RUNTIME-CONTRACT-FROZEN-NOT-RUN'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"frozenBefore": "any guest runtime run"'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"servingClaim": false'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"condition": "the launcher runs under an unprivileged account"'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"state": "awaiting an operator decision"'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"relaxed": false'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"readingApplied": false'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"performed": false'
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"activationAllowed": false'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_guest_runtime_contract_arm64_v1.py

# The input set that would close two of the contract's three gaps. Pinned here
# is the part that separates an input from a result: no image was built, nothing
# serves, and the gap these files cannot close is still named as open rather
# than quietly dropped once two of three looked done. The two v1 files these
# supersede stay in the tree at their sealed digests, because four records name
# them; the successors go to the same guest paths instead of editing them.
require_file native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json
require_text native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json 'MAC3-GUEST-RUNTIME-INPUTS-FROZEN-NOT-BUILT'
require_text native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json '"imageProduced": false'
require_text native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json '"servingClaim": false'
require_text native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json '"gap": "/var/lib/boole/native-shadow/runtime-rootfs"'
require_text native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json '"activationAllowed": false'
require_file native/etc/passwd
require_file native/etc/group
require_file native/etc/nsswitch.conf
require_file native/systemd/boole-native-shadow-launcher-v2.service
require_file native/tmpfiles.d/boole-native-shadow-v2.conf
require_file native/systemd/boole-native-shadow-launcher.service
require_file native/tmpfiles.d/boole-native-shadow.conf
require_text native/systemd/boole-native-shadow-launcher-v2.service 'StandardOutput=journal+console'
require_text native/systemd/boole-native-shadow-launcher-v2.service 'CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_guest_runtime_inputs_arm64_v1.py

# What a successor image would have to satisfy, frozen before one exists. Pinned
# here is the part that keeps it a test rather than a description: one run
# allowed, none spent, an empty result path, and loosening a condition listed as
# a reason to stop rather than as a step. The survey of what staging the inputs
# would cost is pinned too, because knowing the size of a change in advance is
# what stops it from being discovered halfway through.
require_file native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json 'MAC3-SUCCESSOR-IMAGE-PRODUCTION-CRITERIA-PRE-FROZEN-NOT-RUN'
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json '"runsAllowed": 1'
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json '"runsPerformed": 0'
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json 'criteria-would-have-to-be-loosened'
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json '"activationAllowed": false'
require_text native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json 'successorChainForStaging'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_successor_image_production_criteria_arm64_v1.py

# How far the booting guest is from a guest the launcher would serve in, measured
# rather than recalled. Pinned here is what keeps the measurement honest about
# its own two kinds of claim: the builder's zero mentions of the paths the
# launcher requires, the fact that the large object is derived on every pull
# request rather than fetched, and the earlier reading that got this backwards
# being kept in the record instead of quietly dropped.
require_file native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json 'MAC3-RUNTIME-SERVING-GAP-MEASURED-NOT-CLOSED'
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json '"mentionsOfRequiredPaths": 0'
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json '"newExternalAcquisitionRequired": false'
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json '"bootPerformed": false'
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json '"productionDispatched": false'
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json 'correctionOfAnEarlierReading'
require_text native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json 'observedOnDeveloperMachine'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_runtime_serving_gap_measurement_arm64_v1.py

# What closing the serving gap would take, all of it, before any of it is built.
# Pinned here is what stops the plan from drifting into an implementation report
# or a quiet resolution of an open question: three gaps rather than the one that
# was measured, the byte headroom stated as an upper bound rather than a result,
# the held condition carried over unrelaxed, and nothing built, staged or run.
require_file native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json 'MAC3-SERVING-GAP-CLOSURE-PLANNED-NOT-IMPLEMENTED'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json '"count": 3'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json '"builderChanged": false'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json '"lockSuccessorProduced": false'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json '"walkedInThisSession": false'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json '"anyWritablePathCoversAFixedPath": false'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json 'whatThisDoesNotEstablish'
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json 'heldConditionUnchanged'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_serving_gap_closure_plan_arm64_v1.py

# The corrected fourth MAC.3 condition, pre-registered before an image is built
# against it. Pinned here is what keeps it a correction rather than a relaxation:
# it stays a pre-registration and not a result, the sealed contract is not edited
# so the held state before the decision remains readable, the launcher half is an
# equality check on exactly four capabilities, and the submissions half still runs
# unprivileged. The three serving gaps stay open and nothing was built.
require_file native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json 'MAC3-CONDITION-4-CORRECTED-PRE-REGISTERED-NOT-IMPLEMENTED'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"isARelaxation": false'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"allowsSubmissionsToRunAsRoot": false'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"originalRecordEdited": false'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"comparison": "exact-equality"'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"failClosedBeforeExec": true'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"servingGapsRemaining": 3'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json '"imageProduced": false'
require_text native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json 'whatThisDoesNotEstablish'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_condition_4_correction_arm64_v1.py
# The sealed contract keeps saying held; the correction succeeds it, never edits it.
require_text native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json '"readingApplied": false'

# The entry-count half of the nesting budget. The closure plan answered the byte
# half and said the entry half could not be had because no entry count was pinned
# anywhere; one was, and this record carries the bound that follows from it plus
# the correction of that sentence. Pinned here is what keeps it a bound rather
# than a measurement: it stays an upper bound, it keeps the assembly input and
# the produced output apart as different numbers, the pre-assembly check is still
# required, and the closure plan is succeeded rather than edited.
require_file native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json 'MAC3-NESTED-RUNTIME-ENTRY-BUDGET-BOUNDED-NOT-MEASURED'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"limitAppliesTo": "assembly-input"'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"countedNumberDescribes": "produced-output"'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"sameNumber": false'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"preAssemblyCheckStillRequired": true'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"builderRefusesRatherThanTruncates": true'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"runtimeIsContainedInBoot": true'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"earlierRecordEdited": false'
require_text native/containment/native-shadow-mac3-nested-runtime-entry-budget-arm64-v1.json '"treeAssembled": false'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_nested_runtime_entry_budget_arm64_v1.py
# The closure plan keeps the sentence that was too strong, so both halves stay readable.
require_text native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json "neither tree's entry count is pinned anywhere in the repository"

# The descent half of corrected condition 4: the two clauses that named a source
# file and nothing else now have a source-level contract behind them. The two
# labels have to stay together -- the contract is green, and the unit-level drop
# failure matrix is still not measured -- so a later reader cannot take the green
# half alone. The record also has to keep saying that the stronger test was
# written and reverted rather than quietly dropped, that the launcher seal is
# what deferred it, and that re-sealing the current launcher is not the way back.
require_file native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"status": "STATIC-SOURCE-CONTRACT-GREEN"'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"unitLevelDropFailureMatrix": "NOT-MEASURED"'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"acquisition": "rebuild-and-match-seal"'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"revertedBeforeCommit": true'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"couldHaveBeenForced": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"wouldHaveAbortedImageProduction": true'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"launcherSourceChanged": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"launcherResealed": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"predecessorEdited": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"deferredNotAbandoned": true'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"byResealingTheCurrentLauncher": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"usableAsEvidenceForTheCurrentImage": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"fixturesAreInTheGateScript": true'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"sameEvidence": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"normalPathObservedOnARealKernel": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json '"failurePathsFaultInjected": false'
require_text native/containment/native-shadow-mac3-condition-4-descent-refusal-gate-arm64-v1.json 'not a behavioural test'
require_text scripts/self-test.sh scripts/test_native_shadow_mac3_condition_4_descent_refusal_gate_arm64_v1.py

# The first of the four ordered steps that close the three serving gaps. It
# names files and nothing more, so the pins here are mostly about what it did
# NOT do: no lock, no tree, no builder change, no image, no production, no boot.
# The counts are pinned because the whole point of the step is which files the
# next three steps operate on, and the manifest's not-a-tracked-row decision is
# pinned because it refines what the closure plan asked for rather than
# following it silently.
require_file native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"status": "BOOT-ROOTFS-SOURCE-LOCK-PLAN-SUCCESSOR-FROZEN-LOCK-NOT-GENERATED"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"lockGenerated": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"treeAssembled": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"builderChanged": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"imageProduced": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"productionDispatched": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"bootPerformed": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"activationAllowed": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"trackedFileCountBefore": 10'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"trackedFileCountAfter": 15'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"addedTrackedSources": 5'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"supersededTrackedSources": 2'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"predecessorLeftInTree": true'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"leftByteUnchanged": true'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"state": "declared-not-assembled"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"requiresBuilderChange": true'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"isATrackedSourceRow": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"earlierRecordEdited": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"wouldHaveBeenAHardStop": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"isAMeasurementOfTheAssembledTree": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json '"mustBeRemeasuredImmediatelyBeforeAssembly": true'
require_file scripts/test_native_shadow_boot_rootfs_source_lock_plan_arm64_v2.py
require_text scripts/self-test.sh scripts/test_native_shadow_boot_rootfs_source_lock_plan_arm64_v2.py

# The second of the four ordered steps: the tool that builds a successor lock out
# of the files the first step named, and refuses one that is wrong. It seals
# nothing, so the refusal wording that hands sealing to the third step is pinned
# here -- if a later step wants to seal, it supersedes this pin on the record
# rather than quietly dropping it. The predecessor's grounds are imported and
# run rather than reworded, so the import is pinned too: restating a ground is
# how it gets weakened. The frozen guest-init contract pins the digest of both
# superseded sources and therefore refuses the successor outright; the shadow
# lock is how that refusal is answered instead of routed around, so the shadow
# and the verdict it has to reproduce are pinned as the load-bearing part.
require_file scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'PLAN_SCHEMA = "boole.native-shadow.boot-rootfs-source-lock-plan.arm64.v2"'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'RESULT_SCHEMA = "boole.native-shadow.boot-rootfs-source-lock-result.arm64.v2"'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'LOCK_RELEASE = "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V2-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'BOOT-ROOTFS-SOURCE-LOCK-SUCCESSOR-SEALED-LAUNCHER-BINARY-DEFERRED-NOT-BOOT-AUTHORITY'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'PLAN_SHA256 = "da4e7af1dd3cb1db9e263363210c1aec30b7f1bd60ddf87c73fa3921bc018777"'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'LOCK_SCHEMA = predecessor.LOCK_SCHEMA'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'return predecessor.build_source_lock(shim)'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'predecessor._verify_package_closure(source_lock)'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'predecessor._verify_authority_bindings(source_lock)'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'def build_shadow_lock('
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'the frozen guest-init contract refused the unmoved part of the successor'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py "the frozen contract's verdict on the unmoved part differs from the predecessor's"
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py 'the successor documents are not sealed yet. Sealing them is the third step of the '
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py '"nestedRuntimeTreeAssembled": False'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py '"bootableClaim": False'
require_text scripts/native_shadow_boot_rootfs_source_lock_arm64_v2.py '"activationAllowed": False'
require_file scripts/test_native_shadow_boot_rootfs_source_lock_arm64_v2.py
require_text scripts/self-test.sh scripts/test_native_shadow_boot_rootfs_source_lock_arm64_v2.py

# The third of the four ordered steps: the two successor documents, sealed. The
# second step's gate required them to be absent, so the digests are pinned here
# and in the step-three gate rather than left to whichever run wrote them last.
# The generator is pinned at the digest it had when it ran, because this step ran
# it and did not edit it; the sealed result document records that same digest, so
# a later edit to the tool moves both and fails rather than reinterpreting bytes
# that are already sealed. The launcher-unit and tmpfiles digests are pinned on
# both sides -- the sealed lock carries the successor value, and the frozen
# contract still pins the predecessor value and therefore still refuses the sealed
# lock. That refusal is the point, so it is a pin and not an accident.
require_file native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json
require_file native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json
require_text native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json '"release": "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V2-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json '"activationAllowed": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json '"sha256": "4c31bce411c9999b8e877977ce8787d0716a977316ae0a7677240b987181bd55"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json '"sha256": "730ae451fd1c70d41e9a865004040bca03db8cda29dd458cf6bb4d8e75f23b10"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"status": "BOOT-ROOTFS-SOURCE-LOCK-SUCCESSOR-SEALED-LAUNCHER-BINARY-DEFERRED-NOT-BOOT-AUTHORITY"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"sourceLockSha256": "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"generatorSha256": "8218db5cba96440a78bb7cc88edec54f0edb1110684150d1964378f681369b9d"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"status": "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"nestedRuntimeTreeAssembled": false'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"state": "declared-not-assembled"'
require_text native/containment/native-shadow-boot-rootfs-source-lock-result-arm64-v2.json '"bootableClaim": false'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"sha256": "126f0d88e24ecc53879aba02ad910d516980b14473ea30ac4ed14e1cd120e0d8"'
require_text native/containment/native-shadow-guest-init-compatibility-arm64-v1.json '"sha256": "ad9676f2836b097b48e7955c07c165100b2257010bfdb6b4099396fc68f0d721"'
require_file scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py 'SEALED_LOCK_SHA256 = "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f"'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py 'SEALED_RESULT_SHA256 = "0542978a6c49287b27c46a836ae3c1aa548d61e4e065b345ebccbb8d8821dedd"'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py 'GENERATOR_SHA256 = "8218db5cba96440a78bb7cc88edec54f0edb1110684150d1964378f681369b9d"'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py 'def test_the_frozen_contract_still_refuses_the_sealed_lock_itself'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py 'def test_the_fourth_step_widened_the_table_without_editing_this_one'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_arm64_v2.py 'Superseded on 2026-08-28 by the third step'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py

# The fourth of the four ordered steps: the builder's staging table, widened by
# projection rather than by edit. The predecessor keeps its bytes and its four
# boot rows, so the lock it was written for still validates against it, and its
# digest is pinned inside the successor -- an edit there fails here instead of
# being projected onward. The successor runs the same builder a second time with
# nine boot rows, fifteen tracked files in total.
#
# The release gate moved too, and had to. It accepts exactly one release string,
# so the sealed successor lock was refused there before the widened table was
# ever reached; widening which lock is accepted is not accepting both, so the
# predecessor release is refused by the successor exactly as the successor
# release is refused by the predecessor.
#
# The nested runtime tree is declared at the content-manifest digest the launcher
# compiles against and deliberately not merged into a build: the sealed plan
# requires the assembled totals to be measured rather than bounded, and that
# measurement is taken immediately before assembly. The gate asserts the
# not-merged state so it cannot be mistaken for done.
require_file scripts/native_shadow_rootfs_builder_boot_arm64_v2.py
require_file scripts/native_shadow_rootfs_portable_boot_arm64_v2.py
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py 'BOOT_V1_SHA256 = "a5dd54198878473c162ec306fbccd6edac8b22f036d9cf84d244b5f010f96d87"'
require_text scripts/native_shadow_rootfs_portable_boot_arm64_v2.py '"4598e73f9389f41d739edb59660b69b99376a7be1788af24406a58b64d6e0a62"'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py 'BOOTABLE_CLAIM = False'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py 'ACTIVATION_ALLOWED = False'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py 'NESTED_RUNTIME_TREE_ASSEMBLED = False'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"native/etc/passwd",'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"native/etc/shadow",'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"native/systemd/boole-native-shadow-launcher-v2.service",'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"native/tmpfiles.d/boole-native-shadow-v2.conf",'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"guestPrefix": "/var/lib/boole/native-shadow/runtime-rootfs"'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa"'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"contentManifestSizeBytes": 1285116'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v2.py '"layerSizeBytesIsAMeasuredTotal": False'
require_text scripts/native_shadow_rootfs_portable_boot_arm64_v2.py 'BOOTABLE_CLAIM = False'
require_text scripts/native_shadow_rootfs_portable_boot_arm64_v2.py 'ACTIVATION_ALLOWED = False'
require_file scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py 'def test_the_predecessor_builder_table_is_left_at_ten'
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py 'def test_the_predecessor_builder_refuses_the_successor_lock'
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py 'def test_the_successor_builder_passes_every_source_shape_check'
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py 'def test_an_unsorted_closure_is_refused_with_the_predecessors_words'
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py 'def test_this_projection_still_does_not_merge_the_nested_tree'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py '2026-08-28 addendum: the fourth step ran'
require_text scripts/test_native_shadow_boot_rootfs_source_lock_sealed_arm64_v2.py 'def test_the_widened_table_lives_in_the_successor_projection'
require_text scripts/self-test.sh scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py

# The fifth step: the nested runtime tree is merged, and the assembled tree is
# measured rather than added up. The two numbers the fourth step left standing
# -- 13454 boot entries and 4217 nested entries -- do not sum to the answer:
# assembling them derives three parent directories neither table listed, so the
# real total is 17674. That is exactly why the sealed plan required a
# measurement of a real assembly instead of arithmetic.
#
# The merge lives in a successor projection rather than in the fourth step's
# module, because editing that module would falsify assertions the fourth step
# sealed. It is threaded into the frozen builder's own assembler, at the site
# the boot projection already reserved: after the mount-point merge and before
# parent derivation, so the frozen _merge refuses collisions in its own words,
# the derived parents are derived rather than guessed, and the limit checks at
# the end of that function see the combined table. Both the measurement and any
# future production call that one function -- the gate proves it is one object,
# not two that agree.
#
# The measurement itself is a read-only walk of a tree written to disk, checked
# against the builder's own totals key by key. Neither number comes from du or
# from an archive size. The sealed launcher is an aarch64 Linux binary that
# cannot exist on the measuring host, so it is not in the walked tree; rather
# than omit it, the record adds its sealed size and two entries and re-applies
# all three limits to that larger figure.
#
# Passing the limits is a statement that image production's preconditions are
# met. It is not a claim that an image was produced, that it serves, or that it
# boots.
require_file scripts/native_shadow_rootfs_builder_boot_arm64_v3.py
require_file scripts/native_shadow_boot_staging_measure_arm64_v1.py
require_file scripts/test_native_shadow_boot_staging_measure_arm64_v1.py
require_file native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v3.py 'BOOT_V2_SHA256 = "82b96d5a1ab465a710725d580ef58ddb3e1bd4f1db2a11b7e6ccb85fb6acf655"'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v3.py '_merge(entries, nested_tree, "nested runtime tree")'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v3.py 'BOOTABLE_CLAIM = False'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v3.py 'ACTIVATION_ALLOWED = False'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v3.py 'IMAGE_PRODUCED_CLAIM = False'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py 'IMAGE_PRODUCED_CLAIM = False'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py 'SERVING_CLAIM = False'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py 'BOOT_CLAIM = False'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py 'MEASUREMENT_SCHEMA = "boole.native-shadow.boot-staging-tree-measurement.arm64.v1"'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py '"mke2fs",'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py '"mkinitramfs",'
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py '"qemu-img",'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"authorityStatus": "MEASURED-NOT-PRODUCED"'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"entries": 17674'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"payloadBytes": 1771449867'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"largestFileBytes": 160096808'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"pathManifestSha256": "a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736"'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"pathCollisions": 0'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"duplicatePaths": 0'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"symlinkEscapes": 0'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"caseFoldedSiblings": 20'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"imageProductionPreconditionsMet": true'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"imageProduced": false'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"servingClaim": false'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"bootClaim": false'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"payloadBytesIsAMeasuredTotal": true'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"includedInTheMeasuredTree": false'
require_text native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json '"entries": 17676'
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py '2026-08-28 addendum: the fifth step merged the nested tree'
require_text scripts/test_native_shadow_rootfs_builder_boot_arm64_v2.py 'def test_the_merge_lives_in_the_successor_projection'
require_text scripts/test_native_shadow_boot_staging_measure_arm64_v1.py 'def test_both_entry_points_call_the_same_assembler'
require_text scripts/test_native_shadow_boot_staging_measure_arm64_v1.py 'def test_a_forbidden_tool_is_refused_before_it_is_run'
require_text scripts/test_native_shadow_boot_staging_measure_arm64_v1.py 'def test_nothing_is_truncated_or_excluded_to_fit'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_staging_measure_arm64_v1.py
# The largest file is a property of the tree, not of the order it was read in.
# Two files carry the sealed largest size, so "the largest file" needs a second
# question answered: among the regular files of greatest size, the path whose
# canonical bytes sort first -- the rule that produced the sealed value, now
# written where the walk reads it too.
require_text scripts/native_shadow_boot_staging_measure_arm64_v1.py 'def largest_regular_file'
require_text scripts/test_native_shadow_boot_staging_measure_arm64_v1.py 'def test_the_table_and_the_walk_choose_the_same_path_under_a_tie'

# Step six: the successor production path, pre-registered before it exists.
#
# The predecessor criteria record named the predecessor workflow as its producer,
# and it was right to at the time -- it listed three requirements as not done and
# that workflow was the only one there was. All three are closed now, but the
# record cannot be corrected in place: a producer changed after the fact would
# describe the run instead of committing to it. So the correction lives in a
# successor authority that supersedes it on its own terms and leaves its bytes
# alone, which is why the predecessor's digest is pinned here beside the new one.
#
# What the pins are for. The successor path has to be impossible to confuse with
# the predecessor in the two directions that would waste the one allowed attempt:
# the predecessor lock reaching the successor builder, and the successor lock
# reaching the predecessor builder. Neither may fall back to the other, so the
# separate result path, the separate attempt identifier and the separate workflow
# are all named here rather than left to whoever wires it. The budget boundary is
# pinned as prose because it is the rule that decides whether a failure cost the
# attempt: before an output file exists it did not, after one exists it did.
#
# The numbers are the ones the sealed measurement already reached, repeated here
# so that a preflight which quietly disagrees with the measurement fails against
# a record written before it ran rather than against a memory of one.
require_file native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"status": "SUCCESSOR-PRODUCTION-PRE-REGISTERED-NOT-WIRED-NOT-RUN"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V2-ATTEMPT-1"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"producedBy": ".github/workflows/native-shadow-successor-produce-arm64.yml"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json 'native-shadow-mac3-successor-image-production-result-arm64-v2.json'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json 'native-shadow-mac3-successor-preflight-result-arm64-v1.json'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"sha256": "417d2497072519031506664553a0d9b478c53a7bf7983f431332f69bbecec4b8"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"leftByteUnchanged": true'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"materializationFunction": "materialize_staging_tree"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"nestedTreeAssembler": "nested_runtime_tree"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '_assemble_entries, in the fifth projection'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"runsAllowed": 1'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"runsPerformed": 0'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"replicas": 2'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"productionsPerReplica": 1'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"invocation": "e2fsck -f -n"'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json 'the attempt is consumed whatever happens next'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json 'the preflight creates no output directory by construction'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"entries": 17674'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"entries": 17676'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"payloadBytes": 1773456499'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"expectedLauncherSizeBytes": 2006632'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"bootableClaim": false'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"servingClaim": false'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"imageProducedClaim": false'
require_text native/containment/native-shadow-mac3-successor-production-authority-arm64-v2.json '"activationAllowed": false'

# The preflight result the arm64 runner actually wrote, sealed at the path the
# authority named before it existed. Repeatable runs are only evidence if the
# bytes kept are the bytes produced, so the gate re-derives every claim in it
# from the sealed files beside it. It measured; it produced nothing.
require_file native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json
require_text native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json '"outputsCreated": false'
require_text native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json '"imageProducedClaim": false'
require_text native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json '"largestFilePath": "opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly"'
require_text native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json '"pathManifestSha256": "a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736"'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class SealedPreflightResultTests'

# The first production attempt failed inside the isolation the preflight had
# never entered, before any output file existed. The record of it, the
# correction that answers every indirect caller at once, and the preflight that
# now runs where the production runs.
require_file native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json '"outputFilesCreated": 0'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json '"spentVerdict": "OPERATOR-DECISION-PENDING"'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json '"isolationRelaxed": false'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json '"sealedAuthorityRule"'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v1.json '"undefinedMiddle"'
require_file native/containment/native-shadow-mac3-successor-image-production-budget-ruling-arm64-v1.json
require_text native/containment/native-shadow-mac3-successor-image-production-budget-ruling-arm64-v1.json '"productionBudgetConsumed": 0'
require_text native/containment/native-shadow-mac3-successor-image-production-budget-ruling-arm64-v1.json '"attemptsRemaining": 1'
require_text native/containment/native-shadow-mac3-successor-image-production-budget-ruling-arm64-v1.json '"markerName": "ATTEMPT-CONSUMED.json"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'CONSUMED_MARKER_NAME = "ATTEMPT-CONSUMED.json"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def write_consumed_marker'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def attempt_consumed(*, marker_written: bool) -> bool:'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'dir=str(outputs), prefix=".attempt-consumed-partial.", delete=False'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'successor-attempt-consumed-'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ConsumedMarkerTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class OperatorBudgetRulingTests'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def pin_temporary_directory'
require_text scripts/native-shadow-successor-produce-arm64.sh '--preflight-only'
require_text scripts/native-shadow-successor-produce-arm64.sh 'preflight_scratch="$scratch/preflight"'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class SpentAttemptHardStopTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ProvenIsolationBeforeTheBudgetLineTests'

# 2026-08-28 -- the second dispatch, which wrote the marker, built all three
# files, passed the content check and then died assembling the document that
# reports what it built. The record is added beside the first rather than
# replacing it, and the three defects that run exposed are repaired here: the
# assembly is a function a free test runs, the marker lands readable to the
# account that uploads it, and what a failed replica produced is kept under a
# name and a document that disown it.
require_file native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"status": "HARD-STOP-ATTEMPT-CONSUMED-NO-IMAGE-PRESERVED"'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"productionBudgetConsumed": 1'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"preservedArtifacts": 0'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"imageProducedClaim": false'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"newProductionOpportunitiesGranted": 1'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"bootOpportunitiesUsed": 0'
require_text native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v2.json '"leftByteUnchanged": true'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'COLLECTABLE_FILE_MODE = 0o444'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'os.chmod(str(partial), COLLECTABLE_FILE_MODE)'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'UNQUALIFIED_MARKER_NAME = "UNQUALIFIED-DIAGNOSTIC.json"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def write_unqualified_diagnostic'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def make_outputs_readable'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def consumed_attempt(outputs):'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def production_result('
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'with consumed_attempt(outputs):'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'successor-unqualified-diagnostic-'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'Keep what a failed replica left, under a name that disowns it'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ConsumedAttemptHardStopTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ResultDocumentAssemblyTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class MarkerIsReadableToWhoeverCollectsItTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class FailureAfterTheMarkerKeepsWhatItProducedTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class KeptEvidenceSurvivesAFailedReplicaTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class OneShotSectionRehearsedOnFakeFilesTests'

# 2026-08-28 -- the third authority. The production workflow was dispatched
# twice under the second one -- one dispatch refused before it could spend
# anything, one spent -- and this carries exactly one more, sealed before it
# runs. It changes
# the attempt identifier, the result path and the budget boundary -- which now
# names the marker instead of the output directory, because that is where the
# code had already drawn it -- and carries everything else over unchanged. The
# nine inherited hard stops are quoted word for word and four are added beside
# them; adding is the only edit that list takes.
#
# The producer is sealed in its own record rather than in the authority: the
# module carries the authority's digest, so an authority carrying the module's
# would leave neither file with an order it could be written in.
V3_AUTHORITY=native/containment/native-shadow-mac3-successor-production-authority-arm64-v3.json
V3_FINGERPRINT=native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v3.json
require_file "$V3_AUTHORITY"
require_text "$V3_AUTHORITY" '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V3-ATTEMPT-1"'
require_text "$V3_AUTHORITY" '"status": "SUCCESSOR-PRODUCTION-RE-AUTHORISED-AFTER-REPAIR-NOT-RUN"'
require_text "$V3_AUTHORITY" '"runsAllowed": 1'
require_text "$V3_AUTHORITY" '"runsPerformed": 0'
require_text "$V3_AUTHORITY" '"priorProductionDispatches": 2'
require_text "$V3_AUTHORITY" '"priorProductionDispatchesUnspent": 1'
require_text "$V3_AUTHORITY" '"priorProductionAttemptsSpent": 1'
require_text "$V3_AUTHORITY" '"productionAttemptsGrantedHere": 1'
require_text "$V3_AUTHORITY" '"bootAttemptsUsed": 0'
require_text "$V3_AUTHORITY" '"priorImage": "created, lost, not adoptable"'
# The totals above are the count of the rows below them. The wording was
# corrected with them, on the operator's ruling, before anything ran under this
# authority; the correction is recorded in the document rather than applied to
# it quietly.
require_text "$V3_AUTHORITY" 'two prior dispatches, one unspent and one spent'
require_text "$V3_AUTHORITY" '"accountingCorrection"'
require_text "$V3_AUTHORITY" '"anythingRanUnderTheUncorrectedBytes": false'
forbid_text "$V3_AUTHORITY" 'two spent attempts'
require_text "$V3_AUTHORITY" 'native-shadow-mac3-successor-image-production-result-arm64-v3.json'
require_text "$V3_AUTHORITY" 'A refusal raised before ATTEMPT-CONSUMED.json exists'
require_text "$V3_AUTHORITY" '"declaredAdditions"'
require_text "$V3_AUTHORITY" '"inherited"'
require_text "$V3_AUTHORITY" '"leftByteUnchanged": true'
require_text "$V3_AUTHORITY" '"bootableClaim": false'
require_text "$V3_AUTHORITY" '"servingClaim": false'
require_text "$V3_AUTHORITY" '"imageProducedClaim": false'
require_text "$V3_AUTHORITY" '"activationAllowed": false'
require_file "$V3_FINGERPRINT"
require_text "$V3_FINGERPRINT" '"status": "PRODUCER-SEALED-NOT-RUN"'
require_text "$V3_FINGERPRINT" '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V3-ATTEMPT-1"'
require_text "$V3_FINGERPRINT" 'scripts/native_shadow_successor_produce_phase_arm64_v2.py'
require_text "$V3_FINGERPRINT" 'scripts/native-shadow-successor-produce-arm64.sh'
require_text "$V3_FINGERPRINT" '.github/workflows/native-shadow-successor-produce-arm64.yml'
require_text "$V3_FINGERPRINT" '"imageProducedClaim": false'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ThirdAuthorityTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ProducerFingerprintTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'SPENT_AUTHORITY_PATH'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_the_summary_counts_the_rows_it_summarises'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_the_correction_is_recorded_rather_than_made_quietly'

# 2026-08-28 -- the successor production path, wired to the authority above.
#
# The authority named a workflow, a result path and a set of inputs before any
# of them existed. These pins are the other half: the module, the wrapper and
# the workflow that actually consume them, held to the same shape the authority
# pre-registered rather than to whatever they drift into.
#
# The three gaps the wave exists to close are each pinned by the check that
# closes them, not by a comment saying they are closed: the five account files,
# the v2 launcher unit whose output reaches the console the host already
# collects, and the nested runtime tree with its content manifest.
require_file scripts/native_shadow_successor_produce_phase_arm64_v2.py
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'RELEASE = "NATIVE-SHADOW-SUCCESSOR-PRODUCE-PHASE-ARM64-V2"'
require_text scripts/self-test.sh scripts/test_native_shadow_successor_produce_phase_arm64_v2.py
# The predecessor phase is reached for its lock-independent image helpers and
# for nothing that decides which lock is built. Its own name for the first lock
# is what marks the functions this path may not call.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'HISTORICAL_LOCK_CONSTANT = "BOOT_SOURCE_LOCK_PATH"'
# Whether the preflight could have produced an image is answered from this
# module's own call graph, not from a promise about it.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def assert_preflight_creates_no_outputs'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'the preflight can reach the production entry point'
# Production and measurement share one assembler object, which is what stops the
# two from agreeing about a tree neither of them built the same way.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def assert_shared_assembler'
# The staged entry carries its bytes and no digest, so the checks hash what will
# be written. Reading a claimed digest would compare None against a seal.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def _staged_bytes'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'is staged without its bytes'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'LAUNCHER_UNIT_SOURCE = "native/systemd/boole-native-shadow-launcher-v2.service"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"4c31bce411c9999b8e877977ce8787d0716a977316ae0a7677240b987181bd55"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"StandardOutput": "journal+console"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"StandardError": "journal+console"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"CAP_SYS_ADMIN"'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"bootableClaim": BOOTABLE_CLAIM'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"servingClaim": SERVING_CLAIM'
# `WantedBy=` inside the unit is a request; systemd acts on the wants link. A
# tree with the unit and without the link holds a launcher that is installed and
# never started, which is indistinguishable from a working image until it boots.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'LAUNCHER_UNIT_ENABLEMENT_GUEST_PATH'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '/etc/systemd/system/multi-user.target.wants/boole-native-shadow-launcher.service'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def assert_launcher_enabled'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'the unit is installed and never started'
# The three gaps are read back off the tree the writer produced, not off the
# table it was handed. Ownership is deliberately not among them: a preflight that
# is not root cannot reproduce it, so a uid read there is whoever ran it.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def gap_evidence'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'the written staging tree has no'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"gapEvidence": gaps'
# What the sealed result has to carry for a later reader to trace it back to the
# exact text that produced it.
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'def provenance'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'PROVENANCE_MODULES'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py '"provenance": provenance('

# The wrapper. The staging tree is built on a tmpfs this file mounts and the
# phase runs inside the transient unit the sealed producer authority prints, so
# two replicas that agree agree about the image and not about their runners.
# The unit is not spelled here: a second copy of a frozen list is a thing that
# can weaken without anyone noticing.
require_file scripts/native-shadow-successor-produce-arm64.sh
require_text scripts/native-shadow-successor-produce-arm64.sh 'mount -t tmpfs -o mode=0755,nodev,nosuid tmpfs "$staging"'
require_text scripts/native-shadow-successor-produce-arm64.sh 'isolation-argv'
require_text scripts/native-shadow-successor-produce-arm64.sh 'a successor result is already here and is not replaced'

# The read-back consumer, which is the successor's own. Reading a successor
# image back through the predecessor's consumer compares it against the
# predecessor's source lock, and that is what spent the third attempt.
require_text scripts/native-shadow-successor-produce-arm64.sh 'native_shadow_successor_root_disk_readback_arm64_v2.py'
require_text scripts/native-shadow-successor-produce-arm64.sh '$outputs/SUCCESSOR-ROOT-DISK-READBACK.json'
forbid_text scripts/native-shadow-successor-produce-arm64.sh 'native_shadow_boot_root_disk_readback_arm64_v1.py'
require_file scripts/native_shadow_successor_root_disk_readback_arm64_v2.py
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v2.py 'SOURCE_LOCK_PATH = phase.SOURCE_LOCK_PATH'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v2.py 'def assert_no_lock_fallback'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v2.py 'RESULT_NAME = "SUCCESSOR-ROOT-DISK-READBACK.json"'
require_file scripts/test_native_shadow_successor_root_disk_readback_arm64_v2.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_root_disk_readback_arm64_v2.py'

# The predecessor keeps its own consumer and its own lock, byte for byte: it was
# never wrong, and correcting the successor does not get to disturb it.
require_text scripts/native-shadow-boot-produce-arm64.sh 'native_shadow_boot_root_disk_readback_arm64_v1.py'
require_text scripts/native_shadow_boot_root_disk_readback_arm64_v1.py 'phase.BOOT_SOURCE_LOCK_PATH'

# The workflow the authority named. Two modes: one repeatable and producing
# nothing, one the single dispatch. The preflight mode's claim is checked
# against the filesystem afterwards rather than against the code that ran.
require_file .github/workflows/native-shadow-successor-produce-arm64.yml
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'options: [preflight, produce]'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'Require this run to have produced nothing'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'sudo ./scripts/native-shadow-successor-produce-arm64.sh'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'replica: [1, 2]'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'Require the two independent runs to agree byte for byte'
# Both modes fill the store the same way. The first run of the preflight found
# this missing from it: the package acquirer refuses a store without the three
# distribution archives, so the no-output mode stopped before it assembled
# anything -- on the free side of the budget line, which is where a wiring gap
# is supposed to land.
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'Acquire the frozen Rust distribution and re-prove its sealed record'
require_text .github/workflows/native-shadow-successor-produce-arm64.yml 'the production would never have had'
# The two prose records of the same wiring, held so that the code can change only
# alongside the account of it. The local assembly is pinned as what it is -- the
# wrong operating system and architecture -- because that is the sentence a later
# reader would otherwise be tempted to drop.
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "Successor production wiring addendum"
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "assert_preflight_creates_no_outputs"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "successor production path = WIRED-NOT-RUN"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "The tests were richer than reality"
# The addendum that found what the first pass did not ask: enablement is a
# symlink, and a result that raised no exception is not evidence.
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "Enablement and evidence addendum"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "the wants symlink is now required, not inferred"
# The first dispatch, and the asymmetry it refused on. Held in prose because the
# run identifier is the only place the refusal itself survives -- the workflow log
# expires, the sealed result was never written, and the fix on its own reads like
# a step somebody happened to add.
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "Preflight dispatch addendum"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "successor preflight = DISPATCHED-ONCE / REFUSED-BEFORE-ASSEMBLY / RE-RUNNABLE"

# The five directories the kernel filesystems are mounted on. The one MAC.3 boot
# froze because none of them is in the image, and the list is five rather than
# the three the console named because it comes from the guest's own systemd --
# the mount table decoded out of the libsystemd-shared the image ships, plus
# every .mount unit in it and the absence of /etc/fstab. Pinned here so the
# audit cannot quietly shrink back to what one transcript happened to show.
require_file native/containment/native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json
require_text native/containment/native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json '"mountTableEntryCount": 22'
require_text native/containment/native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json '"presentInImage": false'
require_text native/containment/native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json '"mode": "1777"'
require_text native/containment/native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rootfs-runtime-mount-points-arm64-v1.json '"successorImageProduced": false'
require_file scripts/native_shadow_boot_rootfs_mount_point_audit_arm64_v1.py
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py 'runtime_mount_point_entries()'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_rootfs_runtime_mount_points_arm64_v1.py

# Every check the verification stage had passed on the image that froze, because
# none of them asked whether PID 1 could get past its first act. The seventh
# does, and it reads the same audit record the builder writes from.
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"runtime-mount-points-present"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py 'mount_points.required_root_directories()'

# The verification stage is separate from the producer on purpose: a producer
# that checks its own output can only confirm that it did what it did. debugfs
# keeps the inspector role v1 sealed for it -- read-only, never `-w`.
require_file scripts/native_shadow_boot_image_verify_arm64_v1.py
require_file scripts/test_native_shadow_boot_image_verify_arm64_v1.py
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py 'KERNEL_MAGIC = b"ARM\x64"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"kernel-is-arm64"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"pid1-is-systemd"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"launcher-digest-matches-seal"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"launcher-service-is-enabled"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"replay-node-absent"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"modes-owners-and-paths-match-the-lock"'
require_text scripts/native_shadow_boot_image_verify_arm64_v1.py '"guestBootVerified": False'
require_text scripts/self-test.sh scripts/test_native_shadow_boot_image_verify_arm64_v1.py

# The boot projection widens the frozen builder's tables and corrects two
# dependency-reading defects. It must keep reading the exact builder bytes the
# sealed boot lock pins, and record its own bytes separately -- the widening is
# not covered by that pin and must not pretend to be. Both dependency changes
# stay guarded: `:native` is still refused, `:any` needs a Multi-Arch: allowed
# provider, and the closure must hold exactly one concrete architecture.
require_file scripts/native_shadow_rootfs_builder_boot_arm64_v1.py
require_file scripts/test_native_shadow_rootfs_builder_boot_arm64_v1.py
require_file scripts/native_shadow_rootfs_portable_boot_arm64_v1.py
require_file scripts/test_native_shadow_rootfs_portable_boot_arm64_v1.py
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py 'ARM64_BUILDER_SHA256 = "180e893e9643c6fab110016119679b96a5ddf56785cd398b51c8cf8352615ef4"'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py "BOOT_PROJECTION_SHA256 = hashlib.sha256("
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py 'MULTI_ARCH_ANY_REQUIREMENT = "allowed"'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py "def assert_single_architecture"
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py "def normalized_runtime_lock"
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py "BOOTABLE_CLAIM = False"
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py "ACTIVATION_ALLOWED = False"
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py 'if match.group("qualifier") not in (None, ":any"):'
require_text scripts/native_shadow_rootfs_builder_boot_arm64_v1.py 'if match.group("qualifier") == ":any" and candidate.get("multiArch") != "allowed":'
require_text scripts/self-test.sh scripts/test_native_shadow_rootfs_builder_boot_arm64_v1.py
require_text scripts/self-test.sh scripts/test_native_shadow_rootfs_portable_boot_arm64_v1.py

# The produce phase must be unable to reach the network, and two independent
# jobs that disagree must stop rather than be reconciled. The properties are
# read out of the sealed authority rather than restated here, so the gate below
# checks the refusals that deriving alone would not give: a weakened network
# property, and a read-write hole wide enough to undo ProtectSystem=strict.
require_file scripts/native_shadow_boot_image_produce_arm64_v1.py
require_file scripts/test_native_shadow_boot_image_produce_arm64_v1.py
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py 'NETWORK_PROPERTY_REQUIRED_VALUE = "yes"'
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py 'MISMATCH_ACTION = "report-the-difference-never-force-a-match"'
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py 'ABORT_BUILDS_DIFFER = "independent-builds-differ"'
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py 'ABORT_OUTPUT_MISSING = "output-missing-or-empty"'
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py "read-write path would undo ProtectSystem=strict"
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py "BOOTABLE_CLAIM = False"
require_text scripts/native_shadow_boot_image_produce_arm64_v1.py "GUEST_IMAGE_BUILT = False"
require_text scripts/self-test.sh scripts/test_native_shadow_boot_image_produce_arm64_v1.py
require_text docs/native-submission-shadow-verification-v1.md "THE WRAPPER AND THE COMPARISON EXIST. NOTHING WAS"
require_text docs/native-submission-shadow-verification-v1.md "A BUILDER CAN NOW READ THE SEALED BOOT LOCK. NO"
require_text docs/native-submission-shadow-verification-v1.md "THE CLOSURE'S DEPARTURES ARE ENUMERATED. NO IMAGE IS"
require_text docs/native-submission-shadow-verification-v1.md "THE LAUNCHER HAS A WAY IN AND A SEAL TO MATCH. NOTHING"

require_file native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"schema": "boole.native-shadow.boot-rootfs-closure-exception.arm64.v1"'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"release": "NATIVE-SHADOW-BOOT-ROOTFS-CLOSURE-EXCEPTION-ARM64-V1"'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"status": "CLOSURE-EXCEPTIONS-ENUMERATED-NOT-APPLIED-NOT-BOOTED"'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"memberCount": 11'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"danglingSymlinkCount": 3'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"uid": 0'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"gid": 0'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"driftAction": "raise-the-frozen-builder-refusal-unchanged"'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"unlistedMemberAction": "raise-the-frozen-builder-refusal-unchanged"'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"guestImageBuilt": false'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"bootableClaim": false'
require_text native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json '"activationAllowed": false'
require_text docs/native-submission-shadow-verification-v1.md "that pattern fires 98 times and is wrong all"

require_text docs/mac-first-hidden-linux-execution-plan-v1.md "DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED"
require_text docs/native-submission-shadow-verification-v1.md "DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED"
require_text docs/install.md "SOURCE-BOOTSTRAP — NOT THE CURL PRODUCT INSTALLER"
require_text docs/install.md "must not be presented as the finished Mac product installer"
require_text README.md "current command is a source/developer bootstrap"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '"rootfsContentEntryCount": 4216'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| accepted | 0.39 | 139,296 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| accepted replay | 0.39 | 139,168 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| empty | 0.23 | 36,280 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| tampered | 0.40 | 139,264 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| constant | 0.39 | 137,376 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| cross real to synthetic | 0.24 | 36,280 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '| cross synthetic to real | 0.24 | 36,280 |'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "productionByteProvenanceComplete=false"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "activationAllowed=false"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "LLM-MINEABLE-ELIGIBLE-V5=14,160"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "REWARD_READY=0"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "RP0-MD=HOLD"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BF.7=HOLD"
require_text docs/native-submission-shadow-verification-v1.md "Linux/arm64 successor-authority parity milestone"
require_text docs/native-submission-shadow-verification-v1.md "MAC.2-PARTIAL"
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "Linux/arm64 authority-parity closure addendum"
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "Boot source lock plan successor addendum"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "BOOT-ROOTFS-SOURCE-LOCK-PLAN-SUCCESSOR-FROZEN-LOCK-NOT-GENERATED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "That sentence stays as written."
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "necessary but not sufficient"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "staged-inode-ctime-is-not-fs-now"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "wall-clock-survived-in-the-image"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "read rather than deduced"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "counted rather than"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "the control that makes the zero mean something"
require_text docs/native-submission-shadow-verification-v1.md "successor correction addendum"
require_text docs/native-submission-shadow-verification-v1.md "The sentence stays as written."
require_text docs/native-submission-shadow-verification-v1.md "necessary but not sufficient"
require_text docs/native-submission-shadow-verification-v1.md "wall-clock-survived-in-the-image"
forbid_text docs/mac-first-hidden-linux-execution-plan-v1.md "MAC2_MERGE_SHA_PENDING"
forbid_text docs/native-submission-shadow-verification-v1.md "MAC2_MERGE_SHA_PENDING"
forbid_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "MAC2_MERGE_SHA_PENDING"

# P1.8 — the dev-only mock payment doc must carry an unmistakable banner and
# name its feature gate, and receipt-commitment.md must caveat the magic header
# as development-only (not a production payment) with a pointer to the doc.
require_text docs/dev-mock-payment.md "DEVELOPMENT-ONLY. THIS IS NOT A PRODUCTION PAYMENT PATH."
require_text docs/dev-mock-payment.md "dev-mock-payment"
require_text docs/dev-mock-payment.md "VERIFY_ANSWER_PAYMENT_SIGNATURE"
require_text docs/dev-mock-payment.md "working payment system"
require_text docs/receipt-commitment.md "development-only"
require_text docs/receipt-commitment.md "dev-mock-payment.md"

require_text README.md "docs/install.md"
require_text README.md "docs/replay-consensus.md"
require_text README.md "docs/settlement-report.md"
require_text README.md "docs/receipt-commitment.md"
require_text README.md "docs/verified-answer-local-mvp-closeout.md"
require_text README.md "Verified-answer local receipt surface"
require_text README.md "Boole can return a local verified-answer receipt commitment for machine-checkable work in a mock/local payment-gated flow."
require_text README.md "POST /verify-answer"
require_text README.md "ReceiptCommitment"
require_text README.md "boole-native-test"
require_text README.md "x402.draft-2"
require_text README.md "wallet-session-receipt-gate.sh"
require_text README.md "not real x402 settlement"
require_text README.md "not public-network mining evidence"
require_text install.sh "installs required dependencies"
require_text install.sh "never prints API key values"
require_text docs/install.md 'Rust `1.95.0`'
require_text docs/install.md 'Lean `leanprover/lean4:v4.29.1`'
require_text docs/install.md "--run-safe-preflight"
require_text docs/install.md "Step 1/7"
require_text docs/install.md "wizard-report.md"
require_text docs/install.md "--allow-paid-api"
require_text docs/install.md "--target safe-core"
require_text docs/install.md "Optional cargo-audit security scan"
require_text docs/install.md "cargo install cargo-audit"
require_text docs/install.md "cargo audit"
require_text docs/install.md "not part of the default installer or self-test gate"
require_text docs/install.md "Hermes-style model/runtime picker"
require_text docs/install.md "Diagnostics and recovery"
require_text docs/install.md "Ollama readiness"
require_text docs/install.md "setup-required"
require_text docs/install.md "fix: ollama serve"
require_text docs/install.md "fix: ollama pull qwen2.5-coder:7b"
require_text README.md "Diagnostics and recovery"
require_text README.md "Ollama readiness"
require_text README.md "boole-model-benchmark.py"
require_text README.md "benchmark-rows.ndjson"
require_text README.md "Proof-to-Block Benchmark v0.1 card"
require_text README.md "Which AI agents can create verified work that becomes blocks?"
require_text README.md "fake-command CI path: PASS"
require_text README.md "docs/benchmarks/proof-to-block-v0.1-sample.md"
require_text README.md "docs/local-ollama-benchmark.md"
require_text README.md "boole-miner"
require_text README.md "proof-intake, canonicalizer, verifier"
require_text docs/phase7-solo-preflight.md "seven-step guided plan"
require_text docs/phase7-solo-preflight.md "wizard-summary.redacted.json"
require_text docs/phase7-solo-preflight.md "--target hermes:configured"
require_text docs/proof-to-block-benchmark.md "boole-model-benchmark.py"
require_text docs/proof-to-block-benchmark.md "benchmark-summary.json"
require_text docs/proof-to-block-benchmark.md "benchmark-rows.ndjson"
require_text docs/proof-to-block-benchmark.md "--use-node-ticket"
require_text docs/proof-to-block-benchmark.md 'Rows with missing required env vars are recorded as `SKIP`'
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "Sample benchmark artifact"
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "not real model performance"
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "not public-network mining"
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "sample-summary.json"
require_text docs/local-ollama-benchmark.md "Optional local Ollama"
require_text docs/local-ollama-benchmark.md "No automatic model pull"
require_text docs/local-ollama-benchmark.md "No automatic daemon start"
require_text docs/local-ollama-benchmark.md "--model-preset ollama"
require_text fixtures/benchmarks/proof-to-block-v0.1/sample-leaderboard.md "fixture/mock"

require_text docs/replay-consensus.md "selectedShareEvidence"
require_text docs/replay-consensus.md "minShareScoreMultiplierNanos"
require_text docs/replay-consensus.md "fixtures/protocol/replay/v1.json"
require_text docs/replay-consensus.md "fixtures/protocol/replay/v2.json"
require_text docs/replay-consensus.md "legacy/no-evidence replay compatibility"
require_text docs/replay-consensus.md "selected share evidence minShareScore mismatch"
require_text docs/replay-consensus.md "selected share evidence requires minShareScoreMultiplierNanos"

require_text docs/settlement-report.md "boole chain settlement-report"
require_text docs/settlement-report.md "audit-receipts = full shape-only auditor report"
require_text docs/settlement-report.md "settlement-report = read-only reward/reputation summary"
require_text docs/settlement-report.md "auditMode"
require_text docs/settlement-report.md "lineageRequired"
require_text docs/settlement-report.md "does not verify signed-work lineage"
require_text docs/settlement-report.md "--export-reputation-events"
require_text docs/settlement-report.md "boole.reputation.event.v1"
require_text docs/settlement-report.md "settlement-report-shape-only"
require_text docs/settlement-report.md "lineageVerified"
require_text docs/settlement-report.md "does not mutate reward or reputation ledgers"
require_text docs/settlement-report.md "audit failure suppresses settlement output"
require_text docs/settlement-report.md "not public-network mining"

require_text docs/receipt-commitment.md "ReceiptCommitment"
require_text docs/receipt-commitment.md "verifierHashVersion"
require_text docs/receipt-commitment.md "--receipt-commitment-ledger"
require_text docs/receipt-commitment.md "GET /receipts/{receiptId}"
require_text docs/receipt-commitment.md "POST /verify-answer"
require_text docs/receipt-commitment.md "payment_required"
require_text docs/receipt-commitment.md "boole-native-test"
require_text docs/receipt-commitment.md "x402.draft-2"
require_text docs/receipt-commitment.md "x402_version_unsupported"
require_text docs/receipt-commitment.md "boole.agent.event.v1"
require_text docs/receipt-commitment.md "workAccepted"
require_text docs/receipt-commitment.md "workRejected"
require_text docs/receipt-commitment.md "rewardCredited"
require_text docs/receipt-commitment.md "agentEvents"
require_text docs/receipt-commitment.md "wallet-session-receipt-gate.sh"
require_text docs/receipt-commitment.md "Focused local gate"
require_text docs/receipt-commitment.md "not a session key"
require_text docs/receipt-commitment.md "receipt_not_found"
require_text docs/receipt-commitment.md "humanAnswer"
require_text docs/receipt-commitment.md "not public-network mining evidence"
require_text docs/verified-answer-local-mvp-closeout.md "Verified-answer local MVP closeout"
require_text docs/verified-answer-local-mvp-closeout.md "Batch 4 — Verified Answer product surface: COMPLETE for local MVP"
require_text docs/verified-answer-local-mvp-closeout.md "Batch 5 — Gates/docs: COMPLETE"
require_text docs/verified-answer-local-mvp-closeout.md "Definition of Done status"
require_text docs/verified-answer-local-mvp-closeout.md "NEXT-BATCH.1 — Select the next official batch from operating evidence"
require_text docs/verified-answer-local-mvp-closeout.md "not a feature expansion"

# Design-decision records (ADRs) are operator-internal documents (relocated
# 2026-07-02); their gate pins live outside this public script.

# N0-pre.12 — stale tracked-docs corrections (audit R6): the migration
# status doc carries a supersede banner with current gate figures and the
# parity plan marks D3.2 done.
require_text docs/migration-status-and-next-steps.md "Superseded"
require_text docs/migration-status-and-next-steps.md "casesPassed: 7"
require_text docs/boole-node-cli-parity-plan.md "D3.2 (done"

# EVM census P0 — case-task-binding eligibility freeze (append-only attestation).
# Records the frozen contract, conservation identity, ledger/proof hashes, and the
# closed-local boundary. Originals stay in the git-ignored sandbox; only hashes +
# lineage are tracked here. These pins keep the record's ceiling label, conservation
# identity, and non-activation boundary from silently rotting.
require_file docs/evm-census-p0-eligibility-freeze.md
require_text docs/evm-census-p0-eligibility-freeze.md "EVM-P0-MINEABLE-ELIGIBLE = 6,767"
require_text docs/evm-census-p0-eligibility-freeze.md "S-CONTRACT-FREEZE-v1.5"
require_text docs/evm-census-p0-eligibility-freeze.md "6,855 = 6,767 emitted + 79 duplicate + 7 deferred-lossy + 2 deferred-provenance"
require_text docs/evm-census-p0-eligibility-freeze.md "mineable_now stays 0"
require_text docs/evm-census-p0-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/evm-census-p0-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# Solidity census P0 — generative task-family eligibility freeze (append-only
# attestation). Records the two sub-families, the pinned soljson.js 0.8.36 verifier,
# the mandatory trivial-construction gate (N=0), the two-stage conservation identity
# (Stage A 12,931 / Stage B 6,652), the 709 successor split, corpus fingerprints, and
# the closed-local boundary. Corpora and family impl stay in the git-ignored sandbox;
# only hashes + lineage are tracked here. These pins keep the record's ceiling label,
# conservation identities, and non-activation boundary from silently rotting.
require_file docs/solidity-census-p0-eligibility-freeze.md
require_text docs/solidity-census-p0-eligibility-freeze.md "SOLIDITY-P0-MINEABLE-ELIGIBLE = 0"
require_text docs/solidity-census-p0-eligibility-freeze.md "INELIGIBLE-TRIVIAL-CONSTRUCTION"
require_text docs/solidity-census-p0-eligibility-freeze.md "pinned soljson.js 0.8.36"
require_text docs/solidity-census-p0-eligibility-freeze.md "12,931 = 6,652 test-file bundle"
require_text docs/solidity-census-p0-eligibility-freeze.md "0 MINEABLE-ELIGIBLE"
require_text docs/solidity-census-p0-eligibility-freeze.md "2,628 NO-FRESH-INSTANCE"
require_text docs/solidity-census-p0-eligibility-freeze.md "1,670 DEFERRED-EVM-REQUIRED"
require_text docs/solidity-census-p0-eligibility-freeze.md "709 SUCCESSOR-OUT-OF-SCOPE"
require_text docs/solidity-census-p0-eligibility-freeze.md "1,670 = 1,498"
require_text docs/solidity-census-p0-eligibility-freeze.md "mineable_now"
require_text docs/solidity-census-p0-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/solidity-census-p0-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# zk-native release-audit census P0 — anchor eligibility freeze (append-only
# attestation). Records the two separate ceiling labels (zk-native N=0 and Lean
# corpus-not-materialized), the 16,763-anchor conservation identity, the genuine
# anchor->source binding basis, the reason the v1-lenbound Lean checker is not
# applied, and the closed-local boundary. The anchor ledger / source snapshots /
# emitter stay in the git-ignored sandbox; only hashes + lineage + conservation are
# tracked here. These pins keep the two ceiling labels, the conservation identity,
# and the non-activation boundary from silently rotting.
require_file docs/zk-native-release-audit-census-p0-eligibility-freeze.md
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "LEAN-P0 = CORPUS-NOT-MATERIALIZED"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "16,763 NEEDS-SPEC"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "boole.zk-native.release-audit.v1"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "AuditExisting"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "not a fake seed"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "fab8439a4fa9b5062cebf931e155d68cc469661e3f7e2c9556b7bd07c7792bc6"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "v1-lenbound"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "mineable_now"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "confirmed numeric subtotal"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# Solidity semantic P1 — EVM execution-proof task eligibility freeze (append-only
# attestation). Successor to the compile-only Solidity census P0: materializes the
# 1,670 deferred semanticTests into real EVM execution cases and decides eligibility by
# whether a compressed SP1 proof of correct execution can be produced within an 8M-cycle
# ceiling. Records the ceiling label (N=1,396), the fixed task unit, the frozen guest
# ELF/vk binding, the three-level conservation identity, the corpus fingerprint, and the
# closed-local boundary. Guest/host impl, corpus, materialized cases, run ledgers, and
# the representative proof stay in the git-ignored sandbox; only hashes + lineage +
# conservation are tracked here. These pins keep the ceiling label, the conservation
# identities, the engine binding, and the non-activation boundary from silently rotting.
require_file docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 1396"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "public expected outputs are not answer leakage"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "plus its ordered full call bundle"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1,670 total_files = 1,519 CASES-MATERIALIZED"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1,519 = 1,474 COMPILE-MATERIALIZED-CANDIDATE"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1,474 = 1,396 MINEABLE-ELIGIBLE"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1599d54fd75ef48742a9ec460628b6caba68d7a4f33a9c615707b713465d37a2"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "0x004a748560e6b44075bd4fc72a0e88bcef34a91c6d3a47b37a4416d13126b207"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "f0af98e63cde61a6399929f38daa70e694aa929f65c28e7071c624ddf9661f28"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "mineable_now"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"
# Entry 2 — execution-mismatch-reclaim-v1 (append-only successor; Entry 1 N=1396 unchanged)
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE-SUCCESSOR = 1408"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "12 EXECUTION-MISMATCH = 5 AUTHOR-ORACLE-MISREAD"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "+ 0 PINNED-ENGINE-DIVERGENCE"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "successor MINEABLE-ELIGIBLE                       = 1408"

# Rust execution-proof P1 — frozen-snapshot task eligibility freeze (append-only
# attestation). Separate successor to the compile-only Rust Reference P0 (which stays
# at RUST-REFERENCE-P0-MINEABLE-ELIGIBLE = 0, untouched): wraps each runnable rust-lang/
# rust test into a per-edition SP1 guest, executes it under an 8M-cycle ceiling, and
# counts eligibility by whether the guest runs to a fixed 20-byte completion sentinel.
# Records the ceiling label (N = 2,461 files / 2,504 tasks), the fixed task unit, the
# per-task-differing guest ELF/vk binding, the full + census conservation identities at
# both units, the corpus fingerprint, and the closed-local boundary. Guest/host impl,
# corpus, wrapped guests, census ledgers, and the representative proof stay in the
# git-ignored sandbox; only hashes + lineage + conservation are tracked here. These pins
# keep the ceiling label, conservation identities, engine binding, and non-activation
# boundary from silently rotting.
require_file docs/rust-execution-proof-p1-eligibility-freeze.md
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 2,461 files / 2,504 tasks"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-FROZEN-SNAPSHOT-MINEABLE-ELIGIBLE = 2,461 (files)"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-REFERENCE-P0-MINEABLE-ELIGIBLE = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Why public test source is not answer leakage"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "files 26,235 == 26,235"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "tasks 28,735 == 28,735"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "2,895 candidates = 2,462 MINEABLE-ELIGIBLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Per-task ELF/vk differ"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Task-unit 2,504 is a projection"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "af8c179cf544f544c80eb9a23f19be2006fe2bea931e7f965f1ed77b09f7299c"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "0x0038de3b51dcfe81fa915df141a0b5c88e73b727b52f601f2561fe472b947c96"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "e7795af6d2449fb05a6393c3320ced873a999eb3"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "7b7225bf6279fe827e314bb75e6a754ff3c733c444aa581cdea9e46a26df74dd"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "mineable_now"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# Entry 2 (append-only successor: proof-statement dedup). Pins the successor headline,
# the statement digest definition, the proof-reuse binding gap, the 8-bucket task
# conservation, the canonical constants, and the per-task binding-manifest fingerprints
# so the re-audited count and its evidence cannot silently drift. Entry 1 pins above are
# untouched — both units stay recorded side by side.
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "proof-statement-dedup-v1 / N = 687 distinct verifiable proof statements"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 687 distinct verifiable proof statements"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "proof_statement_digest = SHA-256( vk"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "is a **bijection with vk**"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Proof reuse across same-vk tasks is therefore **possible**"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "687  MINEABLE-ELIGIBLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "1,690  DUPLICATE-PROOF-STATEMENT"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "122  ANSWERED-PROOF-FIXTURE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "5  NON-EXECUTION-ORACLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "e3b0c442"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "48b1336c"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "79febbe6"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "c1785e0f"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "8229606a"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "-Zrandomize-layout -Zlayout-seed=2"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entry 1 and the preamble above are **byte-unchanged**"

# Entry 3 (append-only retraction of Entry 2's headline). Pins the reclassification of 687
# to a diagnostic UNBOUND-PROOF-STATEMENT-COUNT, the root-cause label, the task-binding v2
# public-values layout, and the redefined content-duplicate criterion. Entries 1 and 2 stay
# byte-unchanged; these pins guard the correction from silently rotting.
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "task-binding-v2-retraction"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "UNBOUND-PROOF-STATEMENT-COUNT = 687"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "HARNESS-UNBOUND-TASK-IDENTITY"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "687 is NOT a MINEABLE-ELIGIBLE count"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "task_binding_digest_committed (32B) || completion_sentinel (20B)"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "TRUE-CONTENT-DUPLICATE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entry 1 and Entry 2 remain byte-unchanged"

# Entry 4 (append-only successor to Entry 2's retracted 687). Pins the task-binding v2
# re-census successor value 2,499, the 8-bucket conservation, the pinned identity scheme
# and anchors, the representative verifier battery, and the ledger fingerprints. Entries
# 1-3 and the preamble stay byte-unchanged; these pins guard the successor from rotting.
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "task-binding-v2-recensus"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE-SUCCESSOR = 2,499 tasks"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "does not diverge from 2,499"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "2,504 = 2,499 MINEABLE-ELIGIBLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "TRUE-CONTENT-DUPLICATE = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "ANSWERED-PROOF-FIXTURE = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "integrity_failures = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "v=rustexec-task-binding-v2"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "038d31ee96e45e0e6fb9b78a6a3670c3851a5197e4704f7c03c257741b2f46c0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "82b17533145056cb19fd89c2c3d3d69b1b691685daf59329888ea2635bae7f21"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "0e3fd2261daf27f542764acf78ecbeeb3b51075aec12dc2b87dff7780489b465"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "8529068f51fc37bef8df5d135148218b783667fea3114fcd18e5044548dc1a9a"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "0x00916c22e8283a8801c8e75c3beb6a7511974816130d8e939973badb51389f39"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "4ed8e2e774bb19fc2d2107168aa6c5e208936d7ce7750fc8f2582facb038e6ca"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "9d7662622cc57e07e4d176166a5f213dca68c3a97f2b58650af882c162124af7"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "cross-use-to-other-task"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entries 1-3 and the preamble remain byte-unchanged"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Persistent-worker equivalence gate (execution-method lineage)"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "pure-persistent re-execution of all 2,504 tasks"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "two cryptographically-equivalent execution methods"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "56bed1635849c2195ef189f0ba2c5df3313ead5e0c5dc7cdc3b29cad155cdc99"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "2c9e6221bb7e6e61340745ef201dc3b09cb9570896ab624e730a65b58dbff2da"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "b3060c0ac92510618486b6ace38dc7096d4368e4c3720e8f4909254b3104fe7b"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "12f40f111fb8c9e54f9dc1f15ac4b875e27f4716a95b37d6b8469b456e63beb0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "execution-method framing correction"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "authoritative execution running each of the 2,504 tasks exactly once"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entries 1-4 and the preamble remain"

# Ethereum-consensus execution-proof P1 — STF-guest gate closure freeze (append-only
# attestation). Records the SP1 riscv64 guest build closure for ONE exact combination
# (ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest):
# corpus materialized (7,111 STF candidates), native gate 279/279, guest build rc=101
# with no official SP1 drop-in for blst 0.3.17 or c-kzg 2.1.8. The domain is recorded as
# NOT-YET-DETERMINED (never 0); the integrated confirmed subtotal 10,674 and
# mineable_now=0 are unchanged; the 7,111 candidates stay a candidate inventory.
require_file docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "ETHEREUM-CONSENSUS-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "CORPUS-MATERIALIZED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-NATIVE-GATE-VALIDATED-279-OF-279"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-GUEST-INCOMPATIBLE-NO-OFFICIAL-PATCH"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "5031d31e318dd861cf3373702c5d92f085d926e4"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "de67682195ca04869a61eeb1a57320153fe891ff3092e2ff0b946a66dbbb99fb"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "54,452 = 7,111 STF-CANDIDATE + 47,341 NON-STF-NO-RUNNABLE-TASK"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "integrated confirmed subtotal    10,674"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "does **not** state \"the STF axis is closed\""
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "mineable_now"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "issuable-problem-count investigation, not network activation"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"
# Entry 2 — STF crypto-call-path census (append-only). The frozen instrumented native
# harness ran the 4,231 executable STF candidates exactly once and classified all 7,111
# by observed crypto-call path into six conserving buckets. 2,293 NO-CRYPTO is a candidate
# count, NOT mineable; domain stays NOT-YET-DETERMINED, 10,674 and mineable_now=0 unchanged.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-CRYPTO-CALL-PATH-CENSUS-COMPLETE"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "7,111 = 2,293 NO-CRYPTO-REACHED-FOR-FROZEN-INPUT + 1,830 BLS-REQUIRED + 0 KZG-REQUIRED + 0 BLS-AND-KZG-REQUIRED + 2,880 CLIENT-FORK-UNSUPPORTED + 108 UNRESOLVED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 is a candidate count, not a mineable count"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "d752378350bc3f0aec857ec236ab734e0b834b490ddd426fcadaa4dc6fb69a84"
# Entry 3 — successor one-proof gate PASS (append-only). One SP1 compressed proof of the
# frozen crypto-fail-closed v2 guest on the out-of-corpus calibration fixture was produced
# exactly once (0 retries) within the wall/RSS/size envelope; real vk ACCEPT, cross-task and
# tampered REJECT. Establishes proof-issuance feasibility + resource cost only — NOT a 2,293
# result, NOT mineable. Domain stays NOT-YET-DETERMINED, 10,674 and mineable_now=0 unchanged.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-SUCCESSOR-ONE-PROOF-GATE-PASS"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "proof-issuance feasibility"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "run2-lowmem-serial-d026acb446de"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "50616853d5055eebed2ccadfb843d3429150f04f3f133f0af2585efa645b6b36"
# Entry 4 — 2,293 EXECUTE census conservation PASS (append-only). The frozen shared guest
# ran the full census (1,126 frozen partial + 1 operator-aborted 8,192-slot case + 1,166
# continuation, no --resume); exact partition conserved: 2,292 executed rows all ACCEPT +
# crypto_call_free, the aborted case held as CHUNKING-CANDIDATE. The uniform resource policy
# splits by cycle cost only; the 2,206 are recorded ONLY as MONOLITHIC-CYCLE-BAND-CANDIDATE —
# NO mineable determination. Domain stays NOT-YET-DETERMINED, 10,674 and mineable_now=0 unchanged.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-SUCCESSOR-2293-EXECUTE-CENSUS-CONSERVATION-PASS"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 = 2,292 MONOLITHIC-EXECUTE-ELIGIBLE + 1 CHUNKING-CANDIDATE"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 = 2,206 MONOLITHIC-CYCLE-BAND-CANDIDATE + 86 CHUNKING-REQUIRED + 1 CHUNKING-CANDIDATE"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "991,194,325"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "NO mineable determination"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "4cb7d24fbe38ef5f84ec7f3105b688c2cff306a0e7e3bd1efe3d11f13c62726f"

# Entry 5 — minimal-6 proving-band adjudication (append-only). 18 real one-shot compressed
# SP1 proofs (12 sealed in calib-3865 + 6 in min6-3893, 6/6 PASS, 0 retries, 0 resume) dominate
# every one of the 2,206 band rows on all five axes at once, by a SINGLE representative per row.
# Entry 4's cycle-band candidates are upgraded to MONOLITHIC-MINEABLE-ELIGIBLE = 2,206; subtotal
# 10,674 -> 12,880. The wave-2 driver hard-stopped in its own adjudication step (it counted only
# that wave's 6 reps, got 2,203, compared against 2,206); that self-check contradicted the
# pre-registered rule R-6/6 and the freeze's own coverage table, so the frozen rule governs. The
# STOP record and the defective aggregation code are preserved unmodified and the number rests on
# an independent auditor (8/8 negative controls). mineable_now stays 0; the ceiling label stays
# NOT-YET-DETERMINED because 86 CHUNKING-REQUIRED + 1 ABORTED-CHUNKING remain undetermined.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-SUCCESSOR-MONOLITHIC-MINEABLE-ELIGIBLE-2206-SEALED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 = 2,206 MONOLITHIC-MINEABLE-ELIGIBLE + 86 CHUNKING-REQUIRED + 1 ABORTED-CHUNKING"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "integrated confirmed subtotal = 10,674 + 2,206 = 12,880"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "dominated on all five measured axes at once by a"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "not** a mathematical upper bound on"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "no monotonicity theorem over SP1's source is asserted"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STOP record and the defective aggregation code are preserved unmodified"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "ABORTED-HARNESS-POSTPROCESS"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "b2948123c3691837875b01cd301868fce66a74dfb048e19b2afc3aa6e4aca930"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "8eb8bde45b88fb1aa0a488f99d3c5ba08d8e5f489a64a07ff8e3e438dfa98c72"

require_file docs/boole-mcp-e2e.md
require_text docs/boole-mcp-e2e.md "boole-mcp end-to-end smoke (external-user path)"
require_text docs/boole-mcp-e2e.md "closed local smoke; not public-network mining"
require_text docs/boole-mcp-e2e.md "Rust \`1.95.0\`"
require_text docs/boole-mcp-e2e.md "cargo build --release -p boole-mcp --bin boole-mcp"
require_text docs/boole-mcp-e2e.md "boole-mcp --version"
require_text docs/boole-mcp-e2e.md "boole-mcp install --target"
require_text docs/boole-mcp-e2e.md "--dry-run"
require_text docs/boole-mcp-e2e.md "boole-mcp serve --node-url"
require_text docs/boole-mcp-e2e.md "/mcp/tools"
require_text docs/boole-mcp-e2e.md "/mcp/invoke"
require_text docs/boole-mcp-e2e.md "boole.mine"
require_text docs/boole-mcp-e2e.md "boole.status"
require_text docs/boole-mcp-e2e.md '{"state":"idle"}'
require_text docs/boole-mcp-e2e.md '"state":"completed"'
require_text docs/boole-mcp-e2e.md "last_summary"
require_text docs/boole-mcp-e2e.md "RUNTIME_SMOKE_FIXTURE_BYTES"
require_text docs/boole-mcp-e2e.md "tests/fixtures/boole-mcp-e2e/"
require_text docs/boole-mcp-e2e.md "not public-network mining"
require_text docs/boole-mcp-e2e.md "No paid-API calls"

# Testnet/public issuance separation correction (2026-08-27). The current
# boot-image/MAC work prepares a checker runtime; it does not issue or consume
# a mining task. These pins keep template inventory separate from one-shot
# challenge instances and keep testnet ahead of any activation claim.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "TESTNET-INSTANCE-DOMAIN-SEPARATION-V1"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "TEMPLATE-INVENTORY-DOES-NOT-DECREASE-ON-FRESH-ISSUANCE"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "An anchor is source material. A template binds"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "network_id || chain_id || family_version || template_id || epoch || challenge_seed"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "versioned domain tag and canonical length-delimited encoding"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "The rejection gate is bidirectional"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "STATIC-INSTANCE-EXPOSED-NEVER-PROMOTED"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "runtime image -> closed-local issue/check -> cross-network replay gate -> private testnet -> BF.7 zero-reward testnet -> BF.8 activation evidence -> separately approved activation"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "means a non-consensus integration network before BF.7"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "does not remove or replace the formal BF.3, BF.6, BF.6a, RP0-MD or deterministic-resource"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "Section 26.5 remains unchanged as a historical record"
require_text docs/mac-first-hidden-linux-execution-plan-v1.md "production pair is ready but has not been dispatched"
require_text docs/native-submission-shadow-verification-v1.md "TESTNET/PUBLIC TASK-INVENTORY CORRECTION"
require_text docs/native-submission-shadow-verification-v1.md "A testnet answer MUST be rejected in every public-network domain"
require_text docs/native-submission-shadow-verification-v1.md "14,160 is a template/issuance-supply count, not 14,160 one-shot public answers"
require_text docs/native-submission-shadow-verification-v1.md "without invoking the checker or mutating the public ledger"
require_text docs/native-submission-shadow-verification-v1.md "The reciprocal replay must also fail"

# 2026-08-28 -- the third attempt ran, spent itself and failed. Three records
# are added and nothing already sealed is edited: what the run did, the stop
# that follows it, and what the two produced images actually contain when they
# are read back.
#
# The failure is not the image. Both replicas agree with the lock they were
# built from on every entry, including permission bits and ownership; they
# disagree only with a different lock, and only on the two files this wave
# rewrote. So the pins below hold the separation the diagnosis rests on: the
# builder is not at fault, the baseline the checker read is.
#
# The images are kept and fingerprinted, and they may not be adopted. Keeping
# is not adopting, and two replicas containing the same thing is not the sealed
# comparison the authority asks for.
V3_RESULT=native/containment/native-shadow-mac3-successor-image-production-result-arm64-v3.json
V3_STOP=native/containment/native-shadow-mac3-successor-image-production-hard-stop-arm64-v3.json
V3_DIAGNOSTIC=native/containment/native-shadow-mac3-successor-image-production-diagnostic-arm64-v3.json
require_file "$V3_RESULT"
require_text "$V3_RESULT" '"verdict": "FAILED"'
require_text "$V3_RESULT" '"runsPerformed": 1'
require_text "$V3_RESULT" '"id": "modes-owners-and-paths-match-the-lock"'
require_text "$V3_RESULT" '"mayNotBeAdopted": true'
require_text "$V3_RESULT" '"theseAreProductionDigests": false'
require_text "$V3_RESULT" '"sealedComparisonJobRan": false'
require_text "$V3_RESULT" '"thisSatisfiesTheAuthority": false'
require_text "$V3_RESULT" '"imageProducedClaim": false'
require_text "$V3_RESULT" '"bootableClaim": false'
require_text "$V3_RESULT" '"servingClaim": false'
require_text "$V3_RESULT" '"activationAllowed": false'
require_file "$V3_STOP"
# The seven quantities the operator asked to see, kept apart rather than
# collapsed: two dispatches before this one, one of them unspent, this one
# spent, nothing left, no boot, no official image, and one disowned set of
# files per replica.
require_text "$V3_STOP" '"priorProductionDispatches": 2'
require_text "$V3_STOP" '"priorProductionDispatchesUnspent": 1'
require_text "$V3_STOP" '"priorProductionAttemptsSpent": 1'
require_text "$V3_STOP" '"thisAttemptSpent": 1'
require_text "$V3_STOP" '"totalProductionAttemptsSpent": 2'
require_text "$V3_STOP" '"productionAttemptsRemaining": 0'
require_text "$V3_STOP" '"bootAttemptsUsed": 0'
require_text "$V3_STOP" '"officialImages": 0'
require_text "$V3_STOP" '"adoptable": false'
require_text "$V3_STOP" '"retriesAfterTheMarker": 0'
require_text "$V3_STOP" '"recordsLeftByteUnchanged"'
require_text "$V3_STOP" '"stillPinsLiveBytes": false'
require_text "$V3_STOP" 'Re-sealing'
require_file "$V3_DIAGNOSTIC"
require_text "$V3_DIAGNOSTIC" '"status": "ROOT-CAUSE-RESOLVED-NO-REPAIR-AND-NO-NEW-ATTEMPT-HERE"'
require_text "$V3_DIAGNOSTIC" '"builderDefect": false'
require_text "$V3_DIAGNOSTIC" '"checkerBaselineWrong": true'
require_text "$V3_DIAGNOSTIC" 'native_shadow_boot_root_disk_readback_arm64_v1.py'
require_text "$V3_DIAGNOSTIC" '"readOnly": true'
require_text "$V3_DIAGNOSTIC" '"anyImageModified": false'
require_text "$V3_DIAGNOSTIC" '"anyImageMounted": false'
require_text "$V3_DIAGNOSTIC" '"fieldsThatDiffer"'
require_text "$V3_DIAGNOSTIC" '"mismatchSetsIdentical": true'
require_text "$V3_DIAGNOSTIC" '"isThisAProductionDeterminismPass": false'
require_text "$V3_DIAGNOSTIC" '"outputsAdoptable": false'
require_text "$V3_DIAGNOSTIC" '"githubDigest"'
require_text "$V3_DIAGNOSTIC" '"reproducible": false'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ThirdProductionResultTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ThirdHardStopTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ThirdAttemptDiagnosticTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ThirdAttemptAccountingTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_it_separates_a_wrong_builder_from_a_wrong_baseline'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_every_moved_pin_says_why_it_moved'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'The third attempt: spent, failed, and read back'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SUCCESSOR-IMAGE-PRODUCTION  SPENT / FAILED'

# The correction, added beside that failure rather than into it.
#
# The record is what makes the wrapper's moved digest legible: the producer
# fingerprint pins the bytes that produced the third attempt and is deliberately
# not re-sealed over the corrected ones, so the move is declared here instead.
# It authorises nothing -- a new attempt still needs a new authority, a new
# fingerprint over the corrected bytes, and a free preflight first.
V1_CORRECTION=native/containment/native-shadow-mac3-successor-readback-correction-arm64-v1.json
require_file "$V1_CORRECTION"
require_text "$V1_CORRECTION" '"status": "READBACK-CORRECTED-NO-NEW-ATTEMPT-AUTHORISED-HERE"'
require_text "$V1_CORRECTION" 'scripts/native_shadow_successor_root_disk_readback_arm64_v2.py'
require_text "$V1_CORRECTION" 'native-shadow-boot-rootfs-source-lock-arm64-v2.json'
require_text "$V1_CORRECTION" '"pinsThisCorrectionMoves"'
require_text "$V1_CORRECTION" '"producerFingerprintNotReSealed"'
require_text "$V1_CORRECTION" '"recordsLeftByteUnchanged"'
require_text "$V1_CORRECTION" '"whatMustHappenBeforeANewAttempt"'
require_text "$V1_CORRECTION" '"bootableClaim": false'
require_text "$V1_CORRECTION" '"servingClaim": false'
require_text "$V1_CORRECTION" '"activationAllowed": false'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ReadbackCorrectionTests'

# 2026-08-29 -- the one further attempt the correction earned.
#
# The numbers here are the operator's, and they are pinned as text because the
# distinction they draw is the one the last three dispatches kept blurring: a
# workflow that was dispatched is not the same event as an attempt that was
# spent. Three of the former, two of the latter, one of them refused before any
# output existed. The gate re-derives every one of these from the detail rows;
# these pins are what stops the wording from being quietly softened first.
V4_AUTHORITY=native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json
V4_FINGERPRINT=native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v4.json
require_file "$V4_AUTHORITY"
require_text "$V4_AUTHORITY" '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V4-ATTEMPT-1"'
require_text "$V4_AUTHORITY" '"status": "SUCCESSOR-PRODUCTION-RE-AUTHORISED-AFTER-READBACK-CORRECTION-NOT-RUN"'
require_text "$V4_AUTHORITY" '"runsAllowed": 1'
require_text "$V4_AUTHORITY" '"runsPerformed": 0'
require_text "$V4_AUTHORITY" '"priorWorkflowDispatches": 3'
require_text "$V4_AUTHORITY" '"priorUnspentDispatches": 1'
require_text "$V4_AUTHORITY" '"priorProductionAttemptsSpent": 2'
require_text "$V4_AUTHORITY" '"productionAttemptsGrantedHere": 1'
require_text "$V4_AUTHORITY" '"productionAttemptsRemainingBeforeThisGrant": 0'
require_text "$V4_AUTHORITY" '"bootAttemptsUsed": 0'
require_text "$V4_AUTHORITY" '"priorOfficialImages": 0'
require_text "$V4_AUTHORITY" '"priorDiagnosticReplicas"'
require_text "$V4_AUTHORITY" '"adoptable": false'
require_text "$V4_AUTHORITY" '"readBackCorrectionRequiredFirst"'
require_text "$V4_AUTHORITY" 'scripts/native_shadow_successor_root_disk_readback_arm64_v2.py'
require_text "$V4_AUTHORITY" 'native-shadow-boot-rootfs-source-lock-arm64-v2.json'
require_text "$V4_AUTHORITY" '"resultDocument": "SUCCESSOR-ROOT-DISK-READBACK.json"'
require_text "$V4_AUTHORITY" '"uploadsItMay"'
require_text "$V4_AUTHORITY" 'successor-preflight-result'
require_text "$V4_AUTHORITY" 'upload a kernel, an initrd, a root disk or the consumed-attempt marker'
forbid_text "$V4_AUTHORITY" '"upload an artifact"'
require_text "$V4_AUTHORITY" '"inheritedFromTheSecondAuthority"'
require_text "$V4_AUTHORITY" '"inheritedFromTheThirdAuthority"'
require_text "$V4_AUTHORITY" '"leftByteUnchanged": true'
require_text "$V4_AUTHORITY" '"bootableClaim": false'
require_text "$V4_AUTHORITY" '"servingClaim": false'
require_text "$V4_AUTHORITY" '"imageProducedClaim": false'
require_text "$V4_AUTHORITY" '"activationAllowed": false'
require_file "$V4_FINGERPRINT"
require_text "$V4_FINGERPRINT" '"status": "PRODUCER-SEALED-NOT-RUN"'
require_text "$V4_FINGERPRINT" '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V4-ATTEMPT-1"'
require_text "$V4_FINGERPRINT" 'scripts/native_shadow_successor_produce_phase_arm64_v2.py'
require_text "$V4_FINGERPRINT" 'scripts/native-shadow-successor-produce-arm64.sh'
require_text "$V4_FINGERPRINT" '.github/workflows/native-shadow-successor-produce-arm64.yml'
require_text "$V4_FINGERPRINT" 'scripts/native_shadow_successor_root_disk_readback_arm64_v2.py'
require_text "$V4_FINGERPRINT" 'scripts/test_native_shadow_successor_root_disk_readback_arm64_v2.py'
require_text "$V4_FINGERPRINT" '"imageProducedClaim": false'
require_text scripts/native_shadow_successor_produce_phase_arm64_v2.py 'native-shadow-mac3-successor-production-authority-arm64-v4.json'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class FourthAuthorityTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class ThirdProducerFingerprintTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_the_totals_are_the_rows_added_up'

# 2026-08-29 -- what that one attempt produced.
#
# The pins here are mostly about what the run is *not* allowed to have become
# on the way into the record. An image exists; it has not been booted, nothing
# has been served from it, and the failed attempt's byte-identical copies stay
# disowned rather than being quietly adopted now that the digests agree. The
# read-back is pinned to the successor's own lock, because reading it against
# the predecessor's is the whole defect this attempt existed to repair.
V4_RESULT=native/containment/native-shadow-mac3-successor-image-production-result-arm64-v4.json
require_file "$V4_RESULT"
require_text "$V4_RESULT" '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V4-ATTEMPT-1"'
require_text "$V4_RESULT" '"status": "SUCCESSOR-PRODUCTION-PASSED"'
require_text "$V4_RESULT" '"verdict": "PASSED"'
require_text "$V4_RESULT" '"runsPerformed": 1'
require_text "$V4_RESULT" '"runId": 33202978318'
require_text "$V4_RESULT" 'native-shadow-boot-rootfs-source-lock-arm64-v2.json'
require_text "$V4_RESULT" '"modes-owners-and-paths-match-the-lock"'
require_text "$V4_RESULT" '"checksThatFailed": []'
require_text "$V4_RESULT" '"invocation": "e2fsck -f -n"'
require_text "$V4_RESULT" '"repairOptionsUsed": false'
require_text "$V4_RESULT" '"byteIdenticalWhenCheckedByHand": true'
require_text "$V4_RESULT" '"sealedComparisonJobRan": true'
require_text "$V4_RESULT" '"thisSatisfiesTheAuthority": true'
require_text "$V4_RESULT" '"thirdAttemptOutputsAreAdopted": false'
require_text "$V4_RESULT" '"productionAttemptsSpentInTotal": 3'
require_text "$V4_RESULT" '"productionAttemptsRemainingAfterThisRun": 0'
require_text "$V4_RESULT" '"bootAttemptsUsed": 0'
require_text "$V4_RESULT" '"authorityLeftByteUnchanged": true'
require_text "$V4_RESULT" '"stillPinsLiveBytes": false'
require_text "$V4_RESULT" '"whyThereIsNoCorrectedDigest"'
require_text "$V4_RESULT" '"imageProducedClaim": true'
require_text "$V4_RESULT" '"bootableClaim": false'
require_text "$V4_RESULT" '"guestBootVerified": false'
require_text "$V4_RESULT" '"servingClaim": false'
require_text "$V4_RESULT" '"activationAllowed": false'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'class FourthProductionResultTests'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_the_check_that_failed_last_time_is_the_one_that_passed'
require_text scripts/test_native_shadow_successor_produce_phase_arm64_v2.py 'def test_the_moved_gate_is_not_given_a_corrected_digest'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'The fourth attempt: spent, passed, and not booted'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SUCCESSOR-IMAGE-PRODUCTION  SPENT / PASSED'

# 2026-08-29 -- the copy that outlives the artifacts.
#
# The budget is zero, so the images cannot be made again; the artifacts holding
# them expire, and the download that verified them sat somewhere the operating
# system may clear. The pins below hold the two things such a record is most
# likely to soften over time: that a single copy on a single disk is not a
# backup, and that having the files says nothing about booting them. The
# archive itself is machine-local and deliberately not committed -- only its
# digests come into the repository.
V4_PRESERVATION=native/containment/native-shadow-mac3-successor-image-preservation-arm64-v4.json
require_file "$V4_PRESERVATION"
require_text "$V4_PRESERVATION" '"status": "PRESERVED-READ-ONLY-ON-ONE-MACHINE"'
require_text "$V4_PRESERVATION" '"attemptId": "MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-V4-ATTEMPT-1"'
require_text "$V4_PRESERVATION" '"runId": 33202978318'
require_text "$V4_PRESERVATION" '"productionAttemptsRemaining": 0'
require_text "$V4_PRESERVATION" '"machineLocal": true'
require_text "$V4_PRESERVATION" '"committedToTheRepository": false'
require_text "$V4_PRESERVATION" '"files": "0444"'
require_text "$V4_PRESERVATION" '"directories": "0555"'
require_text "$V4_PRESERVATION" '"bothReplicasPreservedInFull": true'
require_text "$V4_PRESERVATION" '"byteIdenticalAtTheArchive": true'
require_text "$V4_PRESERVATION" '"anythingDeleted": false'
require_text "$V4_PRESERVATION" 'It is not a backup'
require_text "$V4_PRESERVATION" 'single point of failure'
require_text "$V4_PRESERVATION" '"expiresAt": "2026-09-04'
require_text "$V4_PRESERVATION" '"imagePreservedClaim": true'
require_text "$V4_PRESERVATION" '"integrityMonitored": false'
require_text "$V4_PRESERVATION" '"offsiteCopyExists": false'
require_text "$V4_PRESERVATION" '"bootableClaim": false'
require_text "$V4_PRESERVATION" '"guestBootVerified": false'
require_text "$V4_PRESERVATION" '"servingClaim": false'
require_text "$V4_PRESERVATION" '"activationAllowed": false'
require_file scripts/test_native_shadow_successor_image_preservation_arm64_v4.py
require_text scripts/test_native_shadow_successor_image_preservation_arm64_v4.py 'class PreservationRecordTests'
require_text scripts/test_native_shadow_successor_image_preservation_arm64_v4.py 'class ArchiveOnDiskTests'
require_text scripts/test_native_shadow_successor_image_preservation_arm64_v4.py 'def test_it_admits_what_one_copy_on_one_machine_is_not'
require_text scripts/test_native_shadow_successor_image_preservation_arm64_v4.py 'def test_it_does_not_put_the_images_in_the_repository'
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_image_preservation_arm64_v4.py'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SUCCESSOR-IMAGE-PRESERVATION  DONE'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Preserving what cannot be made again'

# Third closed-local boot attempt: the conditions, sealed before the run is
# approved and before it is run. Two things a record like this softens if left
# unpinned. First, that it sets the exam without opening it -- a document that
# did both would make the review it exists for impossible to fail. Second, that
# the archive's read-only modes stop a slip and not its owner, which is why the
# digests are recomputed at the moment of loading rather than carried over from
# the day the copy was made.
V3_BOOT_CRITERIA=native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json
require_file "$V3_BOOT_CRITERIA"
require_text "$V3_BOOT_CRITERIA" '"attemptId": "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3"'
require_text "$V3_BOOT_CRITERIA" 'PRE-FROZEN-NOT-RUN-NOT-AUTHORISED'
require_text "$V3_BOOT_CRITERIA" '"frozenBefore": "any qualification run"'
require_text "$V3_BOOT_CRITERIA" '"runsAllowed": 1'
require_text "$V3_BOOT_CRITERIA" '"runsPerformed": 0'
require_text "$V3_BOOT_CRITERIA" '"grantedByThisRecord": false'
require_text "$V3_BOOT_CRITERIA" '"stopsAccidentalOverwrite": true'
require_text "$V3_BOOT_CRITERIA" '"stopsTheOwnerChangingIt": false'
require_text "$V3_BOOT_CRITERIA" '"verifiedImmediatelyBeforeBoot": true'
require_text "$V3_BOOT_CRITERIA" '"onMismatch": "abort"'
require_text "$V3_BOOT_CRITERIA" '"usedAsFallbackOnDigestMismatch": false'
require_text "$V3_BOOT_CRITERIA" 'archive-digests-recomputed-immediately-before-boot'
require_text "$V3_BOOT_CRITERIA" 'launcher-prerequisites-verify-inside-the-guest'
require_text "$V3_BOOT_CRITERIA" 'no-failed-unit-and-no-freeze-in-the-transcript'
require_text "$V3_BOOT_CRITERIA" 'exactly-one-boot-of-this-image'
require_text "$V3_BOOT_CRITERIA" 'nothing-beyond-the-closed-local-boot-is-attempted'
require_text "$V3_BOOT_CRITERIA" 'launcher-supervises-as-root-and-submissions-run-unprivileged'
require_text "$V3_BOOT_CRITERIA" '"stillAbsent": false'
require_text "$V3_BOOT_CRITERIA" '"requiredBeforeProductDistribution": true'
require_text "$V3_BOOT_CRITERIA" '"blocksThisClosedLocalBoot": false'
require_text "$V3_BOOT_CRITERIA" '"mac4Started": false'
require_text "$V3_BOOT_CRITERIA" '"nodeConnected": false'
require_text "$V3_BOOT_CRITERIA" '"miningEnabled": false'
require_text "$V3_BOOT_CRITERIA" '"guestBootVerified": false'
require_text "$V3_BOOT_CRITERIA" '"launcherServing": false'
require_text "$V3_BOOT_CRITERIA" '"bootableClaim": false'
require_text "$V3_BOOT_CRITERIA" '"servingClaim": false'
require_text "$V3_BOOT_CRITERIA" '"activationAllowed": false'
require_file scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v3.py
require_text scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v3.py 'class BootIsNotAuthorisedHereTests'
require_text scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v3.py 'class ReadOnlyIsNotSecurityTests'
require_text scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v3.py 'def test_every_condition_is_either_carried_or_declared_new'
require_text scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v3.py 'def test_no_condition_is_left_answering_nothing'
require_text scripts/self-test.sh 'scripts/test_native_shadow_mac3_closed_local_boot_qualification_arm64_v3.py'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'BOOT-PASS-CRITERIA  SEALED / NOT RUN'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Setting the exam before opening the room'

# The execution contract sits beside those criteria without editing them, and
# the runner it describes refuses the boot: five of the twenty-one conditions
# have no readable evidence in this image, so the one attempt stays unspent
# rather than being spent on answers nobody could read.
V3_BOOT_CONTRACT=native/containment/native-shadow-mac3-closed-local-boot-execution-contract-arm64-v3.json
V3_BOOT_RUNNER=scripts/native_shadow_mac3_closed_local_boot_arm64_v3.py
require_file "$V3_BOOT_CONTRACT"
require_text "$V3_BOOT_CONTRACT" '"attemptId": "MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3"'
require_text "$V3_BOOT_CONTRACT" 'HARD-STOP-STANDS  BOOT-NOT-AUTHORISED  NOT-RUN'
require_text "$V3_BOOT_CONTRACT" '"changesAnyPassCondition": false'
require_text "$V3_BOOT_CONTRACT" '"grantedByThisRecord": false'
require_text "$V3_BOOT_CONTRACT" '"effect": "refuse-before-any-machine-is-built"'
require_text "$V3_BOOT_CONTRACT" '"conditionsWaived": false'
require_text "$V3_BOOT_CONTRACT" '"conditionsReworded": false'
require_text "$V3_BOOT_CONTRACT" '"beforeTheMachineIsConfigured": true'
require_text "$V3_BOOT_CONTRACT" '"afterTheMachineStops": true'
require_text "$V3_BOOT_CONTRACT" '"onMismatch": "abort"'
require_text "$V3_BOOT_CONTRACT" 'preservation-manifest-at-the-archive'
require_text "$V3_BOOT_CONTRACT" 'preservation-record-in-the-repository'
require_text "$V3_BOOT_CONTRACT" '"createdBefore": "the machine is started"'
require_text "$V3_BOOT_CONTRACT" '"outsideTheWorkingDirectory": true'
require_text "$V3_BOOT_CONTRACT" '"exclusiveCreate": true'
require_text "$V3_BOOT_CONTRACT" '"machinesStarted": 0'
require_text "$V3_BOOT_CONTRACT" '"oneUseMarksCreated": 0'
require_text "$V3_BOOT_CONTRACT" 'not-observable-with-this-image'
require_text "$V3_BOOT_CONTRACT" '"editsAnyExistingRecord": false'
require_text "$V3_BOOT_CONTRACT" '"bootPerformed": false'
require_text "$V3_BOOT_CONTRACT" '"bootAuthorised": false'
require_text "$V3_BOOT_CONTRACT" '"runsPerformed": 0'
require_file "$V3_BOOT_RUNNER"
require_text "$V3_BOOT_RUNNER" 'MAC3-CLOSED-LOCAL-BOOT-ARM64-ATTEMPT-3'
require_text "$V3_BOOT_RUNNER" 'def assert_every_condition_is_observable'
require_text "$V3_BOOT_RUNNER" 'def claim_one_use'
require_text "$V3_BOOT_RUNNER" 'def start_the_machine'
require_text "$V3_BOOT_RUNNER" 'def preflight'
require_file scripts/test_native_shadow_mac3_closed_local_boot_execution_contract_arm64_v3.py
require_text scripts/test_native_shadow_mac3_closed_local_boot_execution_contract_arm64_v3.py 'class HardStopTests'
require_text scripts/test_native_shadow_mac3_closed_local_boot_execution_contract_arm64_v3.py 'class OneUseMarkIsClaimedFirstTests'
require_text scripts/test_native_shadow_mac3_closed_local_boot_execution_contract_arm64_v3.py 'def test_the_mark_is_written_before_the_machine_is_started'
require_text scripts/self-test.sh 'scripts/test_native_shadow_mac3_closed_local_boot_execution_contract_arm64_v3.py'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'BOOT-RUNNER  READY TO REFUSE / HARD STOP'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Refusing to spend the one boot'

# Launcher v2 is a new source/build generation beside the historical v1 seal.
# These pins keep three easily blurred boundaries exact: the old reader schema
# was corrected rather than relaxed, the v2 source exists only as a temporary
# build overlay, and neither the source nor build authority grants an image or
# a boot. The arm64 result is pinned separately after CI has produced it.
V1_EVIDENCE_CORRECTION=native/containment/native-shadow-mac3-guest-console-evidence-protocol-arm64-v1-correction.json
V2_SOURCE_OVERLAY=native/containment/native-shadow-launcher-source-overlay-arm64-v2.json
V2_EVIDENCE_PROTOCOL=native/containment/native-shadow-launcher-v2-console-evidence-protocol-arm64-v1.json
V2_BUILD_AUTHORITY=native/containment/native-shadow-launcher-build-authority-arm64-v2.json
V2_BUILD_RESULT=native/containment/native-shadow-launcher-build-result-arm64-v2.json
require_file "$V1_EVIDENCE_CORRECTION"
require_text "$V1_EVIDENCE_CORRECTION" '"changesAnyPassCondition": false'
require_text "$V1_EVIDENCE_CORRECTION" 'present is not an alias for resolved'
require_text "$V1_EVIDENCE_CORRECTION" '"runsPerformed": 0'
require_file "$V2_SOURCE_OVERLAY"
require_text "$V2_SOURCE_OVERLAY" '"baseV1Authority"'
require_text "$V2_SOURCE_OVERLAY" '"dropFailureMatrixIsTableDriven": true'
require_text "$V2_SOURCE_OVERLAY" '"crates/boole-native-shadow-launcher/src/active_execution/mod.rs"'
require_text "$V2_SOURCE_OVERLAY" '"imageProductionAuthorisation": false'
require_text "$V2_SOURCE_OVERLAY" '"bootAuthorisation": false'
require_file "$V2_EVIDENCE_PROTOCOL"
require_text "$V2_EVIDENCE_PROTOCOL" '"prerequisites"'
require_text "$V2_EVIDENCE_PROTOCOL" '"submissionsObserved": false'
require_text "$V2_EVIDENCE_PROTOCOL" '"conditionFourFullySettledByThisRecord": false'
require_file "$V2_BUILD_AUTHORITY"
require_text "$V2_BUILD_AUTHORITY" '"{sourceRoot}=/boole/launcher-build"'
require_text "$V2_BUILD_AUTHORITY" '"{cargoHome}=/boole/cargo-home"'
require_text "$V2_BUILD_AUTHORITY" '"freshCargoHomePerBuild": true'
require_text "$V2_BUILD_AUTHORITY" '"independentBuildCount": 2'
require_text "$V2_BUILD_AUTHORITY" '"postprocessCommand": null'
require_text "$V2_BUILD_AUTHORITY" '"SOURCE_DATE_EPOCH": null'
require_text "$V2_BUILD_AUTHORITY" '"testCommand"'
require_text "$V2_BUILD_AUTHORITY" '"--bins"'
require_text "$V2_BUILD_AUTHORITY" '"guestImageBuilt": false'
require_file "$V2_BUILD_RESULT"
require_text "$V2_BUILD_RESULT" '"sha256": "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"'
require_text "$V2_BUILD_RESULT" '"sizeBytes": 2025192'
require_text "$V2_BUILD_RESULT" '"independentBuildCount": 2'
require_text "$V2_BUILD_RESULT" '"overlaySourceTestRuns": 2'
require_text "$V2_BUILD_RESULT" '"ambient-home": 0'
require_text "$V2_BUILD_RESULT" '"cargo-home": 0'
require_text "$V2_BUILD_RESULT" '"repository-root": 0'
require_text "$V2_BUILD_RESULT" '"rustup-home": 0'
require_text "$V2_BUILD_RESULT" '"source-root": 0'
require_text "$V2_BUILD_RESULT" '"bootableClaim": false'
require_text "$V2_BUILD_RESULT" '"activationAllowed": false'
require_text scripts/test_native_shadow_launcher_build_arm64_v2.py '0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08'
require_text scripts/self-test.sh 'native-shadow-launcher-v2-contract'
require_text native/launcher-v2-overlay/active-execution-after.rs.txt 'let mut listener = bind_listener_in_directory('
require_text native/launcher-v2-overlay/active-execution-after.rs.txt 'crate::console_evidence::emit(&mut stdout.lock(), &records)'
require_text native/launcher-v2-overlay/active-execution-after.rs.txt 'let qualification_stream = listener.accept_one()?'
require_text native/launcher-v2-overlay/active-execution-after.rs.txt 'ListenerBoundConsoleEvidence { reason: String }'
require_text native/launcher-v2-overlay/boole-native-shadow-launcher.rs 'serve_qualified_three_fixed_unix_executions_with_listener_bound_console_evidence'
require_text scripts/native_shadow_mac3_guest_evidence_protocol_arm64_v2.py 'def _malformed_transcript'
require_text scripts/native_shadow_mac3_guest_evidence_protocol_arm64_v2.py 'def _exact_int'
require_text .github/workflows/ci.yml 'native-shadow-launcher-build-arm64-v2'
require_text .github/workflows/ci.yml 'git ls-files --error-unmatch -- "$result"'
require_file scripts/native_shadow_launcher_emit_arm64_v2.py
require_file scripts/test_native_shadow_launcher_emit_arm64_v2.py
require_text scripts/native_shadow_launcher_emit_arm64_v2.py '0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08'
require_text scripts/native_shadow_launcher_emit_arm64_v2.py 'os.O_EXCL'
require_text scripts/native_shadow_launcher_emit_arm64_v2.py 'src_dir_fd=directory'
require_text scripts/native_shadow_launcher_emit_arm64_v2.py 'dst_dir_fd=directory'
require_text scripts/native_shadow_launcher_emit_arm64_v2.py 'os.fsync(directory)'
require_text scripts/native_shadow_launcher_emit_arm64_v2.py 'def emit(path: pathlib.Path)'
require_text scripts/native_shadow_launcher_emit_arm64_v2.py 'launcherDeployedIntoGuest'
require_text scripts/self-test.sh 'scripts/test_native_shadow_launcher_emit_arm64_v2.py'
require_text .github/workflows/ci.yml 'native_shadow_launcher_emit_arm64_v2.py emit --out "$emitted"'

# Launcher-v2 may enter a successor guest only through a new, authority-zero
# generation.  The preregistration pins the exact staging delta and records
# that builder v3 still refuses v2, so a new predecessor-pinned projection is
# required before even a repeatable no-image preflight can run.
require_file native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json
require_text native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json 'PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY'
require_text native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json '"imageProductionRunsAllowed": 0'
require_text native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json '"newBuilderProjectionRequired": true'
require_text native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json '"payloadBytes": 1773475059'
require_file native/containment/native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json
require_text native/containment/native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json '"status": "PASS-NO-IMAGE-PRODUCED"'
require_text native/containment/native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json '"imageProductionRunsAllowed": 0'
require_text native/containment/native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json '"imageProduced": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:BEGIN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'FREE ARM64 PREFLIGHT  GREEN / RESULT SEALED'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'IMAGE PRODUCTION  NOT AUTHORISED'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '2a2bfa93796e0ec1463e1d144250e3bc4e2f6b9c2486c35846e3b9f70071d19d'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'beb2920dcfe11ae0f827b73245a8a15bf9e7b055809ad23fac953cef4ed633c8'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '0dbc17aeaaa8ef63ddeb53ac8b7615f361c21bda95f0ba3d9677bdbdb76dcb9a'
require_text scripts/self-test.sh 'scripts/test_native_shadow_launcher_v2_image_preflight_result_arm64_v1.py'
require_text scripts/self-test.sh 'scripts/test_native_shadow_launcher_v2_image_integration_preregistration_arm64_v1.py'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER V2 IMAGE INTEGRATION  PRE-REGISTERED / AUTHORITY 0'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SUCCESSOR BUILDER PROJECTION  NEXT'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'IMAGE PRODUCTION  NOT AUTHORISED'
require_text .github/workflows/ci.yml 'test "$actual_sha" = "$expected_sha"'
require_text .github/workflows/ci.yml 'test "$actual_size" = "$expected_size"'
require_text .github/workflows/ci.yml 'native-shadow arm64 launcher v2 double build did not pass'

# The producer/readback generation is named and bound before implementation.
# This record is authority-zero: the only next executable path is the free
# JSON-only rehearsal, and production without a future one-use authority must
# stop before assembly, output creation, an attempt marker or an image effect.
V2_SUCCESSOR_PREREG=native/containment/native-shadow-mac3-launcher-v2-successor-producer-preregistration-arm64-v1.json
require_file "$V2_SUCCESSOR_PREREG"
require_file scripts/test_native_shadow_launcher_v2_successor_producer_preregistration_arm64_v1.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_launcher_v2_successor_producer_preregistration_arm64_v1.py'
require_text "$V2_SUCCESSOR_PREREG" '"schema": "boole.native-shadow.mac3.launcher-v2-successor-producer-preregistration.arm64.v1"'
require_text "$V2_SUCCESSOR_PREREG" '"status": "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY"'
require_text "$V2_SUCCESSOR_PREREG" '"imageProductionRunsAllowed": 0'
require_text "$V2_SUCCESSOR_PREREG" '"imageProductionsPerformed": 0'
require_text "$V2_SUCCESSOR_PREREG" '"bootsPerformed": 0'
require_text "$V2_SUCCESSOR_PREREG" '"globalMonkeypatchForbidden": true'
require_text "$V2_SUCCESSOR_PREREG" '"orchestrationCallable": "prepare_staging"'
require_text "$V2_SUCCESSOR_PREREG" '"imageEffectCallsAllowed": 0'
require_text "$V2_SUCCESSOR_PREREG" '"refusesBeforeAssembly": true'
require_text "$V2_SUCCESSOR_PREREG" '"noexec": true'
require_text "$V2_SUCCESSOR_PREREG" '"loopDeviceDetached": true'
require_text "$V2_SUCCESSOR_PREREG" '"qualificationRequiresReadbackPass": true'
require_text "$V2_SUCCESSOR_PREREG" '"futureAuthorityMustBindProducerFingerprintByDigest": true'
require_text "$V2_SUCCESSOR_PREREG" '"futureAuthorityMustBindFreeRehearsalResultByDigest": true'
require_text "$V2_SUCCESSOR_PREREG" '"producerFingerprintMustBindThisRecordByDigest": true'
require_text "$V2_SUCCESSOR_PREREG" '"producerFingerprintBindsFutureAuthorityBytes": false'
require_text "$V2_SUCCESSOR_PREREG" 'd7deacc81e1262b8bd6c9b525a2784850db55c7d93425458243daf5d45fc75b1'
require_text "$V2_SUCCESSOR_PREREG" '3c97808a6dd7b83feb679ca21ce257019b8d549250c1e39ab87e0a6fccdf6e3e'
require_text "$V2_SUCCESSOR_PREREG" '9c41473050b34b830ac6758d88d217d8844ce3154686c93875c1493b50b90589'
require_text "$V2_SUCCESSOR_PREREG" 'scripts/native_shadow_successor_produce_phase_arm64_v3.py'
require_text "$V2_SUCCESSOR_PREREG" 'scripts/native_shadow_successor_root_disk_readback_arm64_v3.py'
require_text "$V2_SUCCESSOR_PREREG" '.github/workflows/native-shadow-successor-produce-arm64-v3.yml'
require_text "$V2_SUCCESSOR_PREREG" '"archiveSha256": "beb2920dcfe11ae0f827b73245a8a15bf9e7b055809ad23fac953cef4ed633c8"'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:BEGIN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SUCCESSOR PRODUCER + READBACK-V3  PRE-REGISTERED / AUTHORITY 0'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md '576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:BEGIN'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:BEGIN'

# The historical 23-row preregistration remains byte-preserved.  A separate,
# authority-zero correction closes the full repository-Python import-time trust
# closure before the generation code is allowed to import any repository helper.
V2_SUCCESSOR_IMPORT_CORRECTION=native/containment/native-shadow-mac3-launcher-v2-successor-producer-import-closure-correction-arm64-v1.json
require_file "$V2_SUCCESSOR_IMPORT_CORRECTION"
require_file scripts/test_native_shadow_launcher_v2_successor_producer_import_closure_correction_arm64_v1.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_launcher_v2_successor_producer_import_closure_correction_arm64_v1.py'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" 'CORRECTED-BEFORE-REHEARSAL-NO-IMAGE-PRODUCTION-AUTHORITY'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" '"effectiveUniqueBindings": 41'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" '"addedMissingBindings": 18'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" '"imageProductionRunsAllowed": 0'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" '"bootsPerformed": 0'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" '0d82a724332d6cd9ed3b8d30b13d18c545c56caf855d608e68410cd9124e303b'
require_text "$V2_SUCCESSOR_IMPORT_CORRECTION" '70374fd617a12b7d8a3f07c3693cc4b9efe237631e4886ab9d92fd5de9145266'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:BEGIN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'EFFECTIVE DIRECT BINDINGS  41'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:BEGIN'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:BEGIN'

# S3-B implements only the authority-zero generation and its repeatable
# no-image rehearsal surface.  The readback edge stays declared but unreachable
# until a future separately sealed authority; no production or boot is opened.
require_file scripts/native_shadow_successor_produce_phase_arm64_v3.py
require_file scripts/native_shadow_successor_root_disk_readback_arm64_v3.py
require_file scripts/native-shadow-successor-produce-arm64-v3.sh
require_file .github/workflows/native-shadow-successor-produce-arm64-v3.yml
require_file scripts/test_native_shadow_successor_produce_phase_arm64_v3.py
require_file scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py
require_file scripts/test_native_shadow_successor_produce_workflow_arm64_v3.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_produce_phase_arm64_v3.py'
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_root_disk_readback_arm64_v3.py'
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_produce_workflow_arm64_v3.py'
require_text scripts/native_shadow_successor_produce_phase_arm64_v3.py 'IMPORT_CORRECTION_SHA256'
require_text scripts/native_shadow_successor_produce_phase_arm64_v3.py 'effective binding union is not exactly forty-one inputs'
require_text scripts/native-shadow-successor-produce-arm64-v3.sh 'len(seen) != 41'
require_text scripts/native-shadow-successor-produce-arm64-v3.sh 'python3 -I -S -c'
require_text scripts/native-shadow-successor-produce-arm64-v3.sh '--verify-bindings-only'
require_text scripts/native-shadow-successor-produce-arm64-v3.sh 'production authority check'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v3.py 'READBACK-V3-PASS-QUALIFIED-FOR-REPLICA-COMPARISON'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v3.py '/proc/self/fd/{image.descriptor}'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v3.py 'expected_image: FileIdentity'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v3.py 'expected_entry_count: int'
require_text scripts/native_shadow_successor_root_disk_readback_arm64_v3.py 'while the qualified result was staged'
require_text .github/workflows/native-shadow-successor-produce-arm64-v3.yml 'Verify all 41 repository inputs before repository Python'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:BEGIN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'S3-B IMPLEMENTATION  GREEN / ARM64 REHEARSAL NOT YET RUN'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:BEGIN'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:BEGIN'

# The real native-arm64 free rehearsal is retained byte-for-byte as R1, and
# F5 seals the exact seven v3 files as historical authority-zero evidence.
# Neither record grants production, boot or MAC.4 authority.
V3_REHEARSAL_RESULT=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v1.json
V3_REHEARSAL_FINGERPRINT=native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v5.json
require_file "$V3_REHEARSAL_RESULT"
require_file "$V3_REHEARSAL_FINGERPRINT"
require_file scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v1.py
require_file scripts/test_native_shadow_successor_producer_fingerprint_arm64_v5.py
require_text scripts/self-test.sh 'scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v1.py'
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_producer_fingerprint_arm64_v5.py'
require_text "$V3_REHEARSAL_RESULT" '"status": "PASS-NO-IMAGE-PRODUCED"'
require_text "$V3_REHEARSAL_RESULT" '"imageProductionRunsAllowed": 0'
require_text "$V3_REHEARSAL_RESULT" '"imageProduced": false'
require_text scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v1.py 'd21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c'
require_text scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v1.py 'AUTHORITY-ZERO-STAGING-EVIDENCE'
require_text "$V3_REHEARSAL_FINGERPRINT" '"historicalAuthorityZeroStagingEvidenceOnly": true'
require_text "$V3_REHEARSAL_FINGERPRINT" '"productionReadyClaim": false'
require_text "$V3_REHEARSAL_FINGERPRINT" '"readbackV3ExecutedByRehearsal": false'
require_text scripts/test_native_shadow_successor_producer_fingerprint_arm64_v5.py '6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa'
require_text scripts/test_native_shadow_successor_producer_fingerprint_arm64_v5.py 'LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V5-SEALED'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:BEGIN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'V3 FREE ARM64 REHEARSAL  GREEN / ONE CANONICAL JSON SEALED'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:BEGIN'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:BEGIN'

# Production is deliberately moved to a fresh namespace.  P2 retires the two
# unused v5 reservations, requires a fresh R2 for v4 bytes and leaves every
# production, boot and activation authority at zero.
V4_PRODUCTION_PREREG=native/containment/native-shadow-mac3-launcher-v2-successor-production-generation-preregistration-arm64-v1.json
V4_PRODUCTION_PREREG_GATE=scripts/test_native_shadow_launcher_v2_successor_production_generation_preregistration_arm64_v1.py
require_file "$V4_PRODUCTION_PREREG"
require_file scripts/test_native_shadow_launcher_v2_successor_production_generation_preregistration_arm64_v1.py
require_text scripts/self-test.sh "$V4_PRODUCTION_PREREG_GATE"
require_text "$V4_PRODUCTION_PREREG" '"schema": "boole.native-shadow.mac3.launcher-v2-successor-production-generation-preregistration.arm64.v1"'
require_text "$V4_PRODUCTION_PREREG" '"status": "PRE-REGISTERED-PRODUCTION-GENERATION-NO-IMAGE-PRODUCTION-AUTHORITY"'
require_text "$V4_PRODUCTION_PREREG" '"imageProductionRunsAllowed": 0'
require_text "$V4_PRODUCTION_PREREG" '"historicalFreeRehearsalsObserved": 1'
require_text "$V4_PRODUCTION_PREREG" '"requiresFreshR2BeforeFingerprintOrAuthority": true'
require_text "$V4_PRODUCTION_PREREG" '"producerGeneration": 4'
require_text "$V4_PRODUCTION_PREREG" '"fingerprint": 6'
require_text "$V4_PRODUCTION_PREREG" 'native-shadow-mac3-successor-production-authority-arm64-v5.json'
require_text "$V4_PRODUCTION_PREREG" 'native-shadow-mac3-successor-image-production-result-arm64-v5.json'
require_text "$V4_PRODUCTION_PREREG_GATE" '4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95'
require_text "$V4_PRODUCTION_PREREG_GATE" 'os.path.lexists'
require_text "$V4_PRODUCTION_PREREG_GATE" 'LAUNCHER-V2-SUCCESSOR-PRODUCTION-GENERATION-PREREGISTRATION-ARM64-V1-FROZEN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'PRODUCTION-GENERATION V4 + R2  NEXT'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCTION-GENERATION-PREREGISTRATION-ARM64-V1-FROZEN'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCTION-GENERATION-PREREGISTRATION-ARM64-V1-FROZEN'

# P2's historical 8,096-byte prose was a typo (the preserved file is 8,156
# bytes), and its future one-run declaration did not create a durable global
# dispatch claim.  The append-only correction freezes an attempt-specific,
# annotated-tag claim contract while granting no run.
V4_DISPATCH_FENCE_CORRECTION=native/containment/native-shadow-mac3-launcher-v2-successor-production-dispatch-fence-correction-arm64-v1.json
require_file "$V4_DISPATCH_FENCE_CORRECTION"
require_file "scripts/test_native_shadow_successor_production_dispatch_fence_correction_arm64_v1.py"
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_production_dispatch_fence_correction_arm64_v1.py'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"schema": "boole.native-shadow.mac3.launcher-v2-successor-production-dispatch-fence-correction.arm64.v1"'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"status": "CORRECTED-BEFORE-R2-NO-PRODUCTION-DISPATCH-AUTHORITY"'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"sha256": "4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95"'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"sizeBytes": 8156'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"productionDispatchClaimsAllowed": 0'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"productionDispatchClaimsCreated": 0'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"soleWriteJob": "production-authority-guard"'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"requiredGitHubRunAttempt": 1'
require_text "$V4_DISPATCH_FENCE_CORRECTION" '"fieldName": "productionDispatchFenceCorrection"'
require_text scripts/test_native_shadow_successor_production_dispatch_fence_correction_arm64_v1.py '16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCTION-DISPATCH-FENCE-CORRECTION-ARM64-V1-FROZEN'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCTION-DISPATCH-FENCE-CORRECTION-ARM64-V1-FROZEN'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCTION-DISPATCH-FENCE-CORRECTION-ARM64-V1-FROZEN'

# Producer generation v4 now implements the exact five paths P2 declared.  It
# still has no R2/F6/A6 and therefore cannot create the production claim or an
# image.  The only executable next edge is the manual authority-zero rehearsal.
V4_PRODUCER=scripts/native_shadow_successor_produce_phase_arm64_v4.py
V4_PRODUCER_GATE=scripts/test_native_shadow_successor_produce_phase_arm64_v4.py
V4_WRAPPER=scripts/native-shadow-successor-produce-arm64-v4.sh
V4_WORKFLOW=.github/workflows/native-shadow-successor-produce-arm64-v4.yml
V4_WORKFLOW_GATE=scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py
for path in "$V4_PRODUCER" "$V4_PRODUCER_GATE" "$V4_WRAPPER" \
  "$V4_WORKFLOW" "$V4_WORKFLOW_GATE"; do
  require_file "$path"
done
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_produce_phase_arm64_v4.py'
require_text scripts/self-test.sh 'scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py'
require_text "$V4_WORKFLOW" 'options: [rehearsal, production]'
require_text "$V4_WORKFLOW" 'contents: write'
require_text "$V4_WORKFLOW" 'git push --atomic --porcelain'
require_text "$V4_WORKFLOW" '--verify-dispatch-claim'
require_text "$V4_WORKFLOW" '--property=KillMode=control-group'
require_text "$V4_WORKFLOW" '"$anchored_wrapper" --cleanup-only'
require_text "$V4_WORKFLOW" 'retention-days: 7'
require_text "$V4_WORKFLOW" '--compare-provenanced-replicas'
require_text "$V4_WORKFLOW" 'cmp -- "$left/outputs/guest-root-disk" "$right/outputs/guest-root-disk"'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:BEGIN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'V4 IMPLEMENTATION  GREEN / R2 NOT YET RUN'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:BEGIN'
require_text docs/native-submission-shadow-verification-v1.md 'LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:BEGIN'

# The first two free v4 rehearsals failed before any canonical R2 result or
# artifact existed.  Pin that narrow history separately so it cannot be
# rewritten as success and cannot occupy the future successful R2 path.
V4_R2_HARD_STOP=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v2.json
V4_R2_HARD_STOP_GATE=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_hard_stop_arm64_v2.py
require_file "$V4_R2_HARD_STOP"
require_file "$V4_R2_HARD_STOP_GATE"
require_text scripts/self-test.sh "$V4_R2_HARD_STOP_GATE"
require_text "$V4_R2_HARD_STOP_GATE" '7a8cf17bfedfbe424978f41b38314d6ba5304a36df49a566a60ff4e5114328eb'
require_text "$V4_R2_HARD_STOP" '"runId": 33311411461'
require_text "$V4_R2_HARD_STOP" '"runId": 33313895353'
require_text "$V4_R2_HARD_STOP" '"successfulR2ResultsCreatedByTheseAttempts": 0'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FAILED-ATTEMPTS-ARM64-V2-SEALED:BEGIN'
done

# The third free rehearsal reached its HEAD-bound service but rejected one
# guest-root absolute OCI symlink before creating R2.  The historical record
# stays unchanged and the narrower service/effect wording is pinned below.
V4_R2_HARD_STOP_V3=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v3.json
V4_R2_HARD_STOP_GATE_V3=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_hard_stop_arm64_v3.py
require_file "$V4_R2_HARD_STOP_V3"
require_file "$V4_R2_HARD_STOP_GATE_V3"
require_text scripts/self-test.sh "$V4_R2_HARD_STOP_GATE_V3"
require_text "$V4_R2_HARD_STOP_GATE_V3" '3cfe5cb9df41c15206e3ca56d5224c7b5e03ebb0a118d8a49fd9b4154bc86e07'
require_text "$V4_R2_HARD_STOP_V3" '"runId": 33316130780'
require_text "$V4_R2_HARD_STOP_V3" '"successfulR2ResultsCreatedByThisAttempt": 0'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-ARM64-V3-SEALED:BEGIN'
done

V4_R2_HARD_STOP_CORRECTION=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-correction-arm64-v1.json
V4_R2_HARD_STOP_CORRECTION_GATE=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_hard_stop_correction_arm64_v1.py
require_file "$V4_R2_HARD_STOP_CORRECTION"
require_file "$V4_R2_HARD_STOP_CORRECTION_GATE"
require_text scripts/self-test.sh "$V4_R2_HARD_STOP_CORRECTION_GATE"
require_text "$V4_R2_HARD_STOP_CORRECTION_GATE" '88a7fc38963f48fa42018ba7e29ab5648f6767f7cecaac66d1aa4e7047c292c8'
require_text "$V4_R2_HARD_STOP_CORRECTION" '"finalGuestImageOutputsCreatedByRehearsal": 0'
require_text "$V4_R2_HARD_STOP_CORRECTION" '"transientOciScratchLayoutCreatedByRehearsal": true'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-SCOPE-CORRECTION-ARM64-V1-SEALED:BEGIN'
done

# The fourth free rehearsal reached low-level preparation but the rehearsal
# job had not acquired the separately sealed ext4 writer packages.  Preserve
# that failure without claiming R2 or broad runner cleanup.
V4_R2_HARD_STOP_V4=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v4.json
V4_R2_HARD_STOP_GATE_V4=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_hard_stop_arm64_v4.py
require_file "$V4_R2_HARD_STOP_V4"
require_file "$V4_R2_HARD_STOP_GATE_V4"
require_text scripts/self-test.sh "$V4_R2_HARD_STOP_GATE_V4"
require_text "$V4_R2_HARD_STOP_GATE_V4" '96721d93d6016a6ee9c8714672ee9e49c0672336181bc1ef8082ab5445081eae'
require_text "$V4_R2_HARD_STOP_V4" '"runId": 33319199252'
require_text "$V4_R2_HARD_STOP_V4" '"successfulR2ResultsCreatedByThisAttempt": 0'
require_text "$V4_R2_HARD_STOP_V4" '"transientOciScratchLayoutCreatedByRehearsal": true'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-ARM64-V4-SEALED:BEGIN'
done

V4_R2_HARD_STOP_CORRECTION_V2=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-correction-arm64-v2.json
V4_R2_HARD_STOP_CORRECTION_GATE_V2=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_hard_stop_correction_arm64_v2.py
require_file "$V4_R2_HARD_STOP_CORRECTION_V2"
require_file "$V4_R2_HARD_STOP_CORRECTION_GATE_V2"
require_text scripts/self-test.sh "$V4_R2_HARD_STOP_CORRECTION_GATE_V2"
require_text "$V4_R2_HARD_STOP_CORRECTION_GATE_V2" 'b0f140161df0029eec5359a25d2ec6a207511d6787fa7a9000de997a95b90177'
require_text "$V4_R2_HARD_STOP_CORRECTION_V2" '"productionRunsObserved": 0'
require_text "$V4_R2_HARD_STOP_CORRECTION_V2" '"correctScope": "free-rehearsal job"'
require_text "$V4_R2_HARD_STOP_CORRECTION_V2" '"directObservationLimitedToOneObject": true'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-WORDING-CORRECTION-ARM64-V2-SEALED:BEGIN'
done

# Fresh authority-zero R2 succeeded on the exact v4 generation.  Keep the raw
# payload separate from its GitHub transport provenance and preserve zero
# production/image/boot authority in both records.
V4_R2_RESULT=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v2.json
V4_R2_RESULT_GATE=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v2.py
V4_R2_PROVENANCE=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-artifact-provenance-arm64-v2.json
V4_R2_PROVENANCE_GATE=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_artifact_provenance_arm64_v2.py
for path in "$V4_R2_RESULT" "$V4_R2_RESULT_GATE" \
  "$V4_R2_PROVENANCE" "$V4_R2_PROVENANCE_GATE"; do
  require_file "$path"
done
require_text scripts/self-test.sh "$V4_R2_RESULT_GATE"
require_text scripts/self-test.sh "$V4_R2_PROVENANCE_GATE"
require_text "$V4_R2_RESULT_GATE" '7efe89c3bc558455313b76de2a625e708a580d0256760692914e9474eb0171f0'
require_text "$V4_R2_PROVENANCE_GATE" '6d569cdf8c875d0835df64d38aacd5d7e69cb1f44e2b2eb9bea550d59b12707d'
require_text "$V4_R2_RESULT" '"status": "PASS-NO-IMAGE-PRODUCED"'
require_text "$V4_R2_RESULT" '"imageProductionRunsAllowed": 0'
require_text "$V4_R2_RESULT" '"productionOutputsCreated": 0'
require_text "$V4_R2_PROVENANCE" '"runId": 33321624511'
require_text "$V4_R2_PROVENANCE" '"runArtifactTotalCount": 1'
require_text "$V4_R2_PROVENANCE" '"productionGuardSkipped": true'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-SUCCESS-ARM64-V2-SEALED:BEGIN'
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-ARTIFACT-PROVENANCE-ARM64-V2-SEALED:BEGIN'
  require_text "$doc" 'R2 GREEN / F6 NEXT / A6 NOT CREATED'
done

# F6 freezes the exact v4 generation observed by fresh R2 while preserving
# authority zero.  A6/result-v6 are deliberately not required to exist here.
V4_F6=native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v6.json
V4_F6_GATE=scripts/test_native_shadow_successor_producer_fingerprint_arm64_v6.py
require_file "$V4_F6"
require_file "$V4_F6_GATE"
require_text scripts/self-test.sh "$V4_F6_GATE"
require_text "$V4_F6_GATE" '0e98b02f2dc8c4752c282dba57e1aa39d1cdc62a83c57d8803d6051ea792c183'
require_text "$V4_F6" '"schema": "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v6"'
require_text "$V4_F6" '"status": "SEALED-AFTER-FRESH-R2-PRODUCTION-GENERATION-NOT-AUTHORISED"'
require_text "$V4_F6" '"imageProductionRunsAllowed": 0'
require_text "$V4_F6" '"sha256": "16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b"'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V6-SEALED'
  require_text "$doc" 'R2 GREEN / F6 SEALED / A6 NOT CREATED / PRODUCTION AND BOOT NOT RUN'
done

# Pre-A6 review found that generation v4 accepted any non-empty workflow-ref
# suffix.  Preserve that generation as history and require an authority-zero,
# append-only main-only successor before any production authority can exist.
V4_MAIN_BRANCH_FENCE_CORRECTION=native/containment/native-shadow-mac3-launcher-v2-successor-main-branch-dispatch-fence-correction-arm64-v1.json
V4_MAIN_BRANCH_FENCE_GATE=scripts/test_native_shadow_successor_main_branch_dispatch_fence_correction_arm64_v1.py
require_file "$V4_MAIN_BRANCH_FENCE_CORRECTION"
require_file "$V4_MAIN_BRANCH_FENCE_GATE"
require_text scripts/self-test.sh "$V4_MAIN_BRANCH_FENCE_GATE"
require_text "$V4_MAIN_BRANCH_FENCE_GATE" '63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b'
require_text "$V4_MAIN_BRANCH_FENCE_CORRECTION" '"status": "A6-WITHHELD-PENDING-MAIN-ONLY-SUCCESSOR-GENERATION"'
require_text "$V4_MAIN_BRANCH_FENCE_CORRECTION" '"exactDispatchRef": "refs/heads/main"'
require_text "$V4_MAIN_BRANCH_FENCE_CORRECTION" 'native-shadow-successor-produce-arm64-v5.yml@refs/heads/main'
require_text "$V4_MAIN_BRANCH_FENCE_CORRECTION" '"imageProductionRunsAllowed": 0'
require_text "$V4_MAIN_BRANCH_FENCE_CORRECTION" '"administratorDeletionIsPreventedByObservedServerRuleset": false'
require_text "$V4_MAIN_BRANCH_FENCE_CORRECTION" 'native-shadow-mac3-successor-production-authority-arm64-v7.json'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-MAIN-BRANCH-DISPATCH-FENCE-CORRECTION-ARM64-V1-SEALED:BEGIN'
  require_text "$doc" 'A6-V6'
done

# P4's main-only producer generation now exists, but implementation does not
# create R3 or any production authority/effect.  Keep the fresh namespace and
# the next authority-zero cursor visible in every authority-facing document.
V5_CORE=scripts/native_shadow_successor_produce_phase_arm64_v5.py
V5_CORE_GATE=scripts/test_native_shadow_successor_produce_phase_arm64_v5.py
V5_WRAPPER=scripts/native-shadow-successor-produce-arm64-v5.sh
V5_WORKFLOW=.github/workflows/native-shadow-successor-produce-arm64-v5.yml
V5_WORKFLOW_GATE=scripts/test_native_shadow_successor_produce_workflow_arm64_v5.py
V5_GENERATION_GATE=scripts/test_native_shadow_successor_generation_v5_contract.py
for path in "$V5_CORE" "$V5_CORE_GATE" "$V5_WRAPPER" "$V5_WORKFLOW" \
  "$V5_WORKFLOW_GATE" "$V5_GENERATION_GATE"; do
  require_file "$path"
done
for gate in "$V5_CORE_GATE" "$V5_WORKFLOW_GATE" "$V5_GENERATION_GATE"; do
  require_text scripts/self-test.sh "$gate"
done
require_text "$V5_CORE" 'native-shadow-mac3-successor-production-authority-arm64-v7.json'
require_text "$V5_CORE" 'boole.native-shadow.mac3.successor-production-dispatch-claim.arm64.v2'
require_text "$V5_CORE" 'mainBranchDispatchFenceCorrection'
require_text "$V5_WRAPPER" 'refs/heads/main'
require_text "$V5_WORKFLOW" "github.event_name == 'workflow_dispatch'"
require_text "$V5_WORKFLOW" "github.ref == 'refs/heads/main'"
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-V5-IMPLEMENTED-R3-PENDING:BEGIN'
  require_text "$doc" 'R3'
done

# Fresh authority-zero R3 succeeded on exact main.  Keep its raw canonical
# payload separate from GitHub transport provenance and preserve zero
# production/image/boot authority in both records.
V5_R3_RESULT=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v3.json
V5_R3_RESULT_GATE=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_result_arm64_v3.py
V5_R3_PROVENANCE=native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-artifact-provenance-arm64-v3.json
V5_R3_PROVENANCE_GATE=scripts/test_native_shadow_launcher_v2_successor_producer_rehearsal_artifact_provenance_arm64_v3.py
for path in "$V5_R3_RESULT" "$V5_R3_RESULT_GATE" \
  "$V5_R3_PROVENANCE" "$V5_R3_PROVENANCE_GATE"; do
  require_file "$path"
done
require_text scripts/self-test.sh "$V5_R3_RESULT_GATE"
require_text scripts/self-test.sh "$V5_R3_PROVENANCE_GATE"
require_text "$V5_R3_RESULT_GATE" '44cd7d6feea2efc62d9ab6cb809e5d66c1452c9e4d2f034fd800e6573938fe87'
require_text "$V5_R3_PROVENANCE_GATE" 'f1618b92cfa138370209a50743f9630e497b35ee4e05d117d1e0af369a95320d'
require_text "$V5_R3_RESULT" '"status": "PASS-NO-IMAGE-PRODUCED"'
require_text "$V5_R3_RESULT" '"imageProductionRunsAllowed": 0'
require_text "$V5_R3_RESULT" '"productionOutputsCreated": 0'
require_text "$V5_R3_PROVENANCE" '"runId": 33347946953'
require_text "$V5_R3_PROVENANCE" '"runArtifactTotalCount": 1'
require_text "$V5_R3_PROVENANCE" '"productionGuardSkipped": true'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R3-SUCCESS-ARM64-V3-SEALED:BEGIN'
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-R3-ARTIFACT-PROVENANCE-ARM64-V3-SEALED:BEGIN'
  require_text "$doc" 'R3 GREEN / F7 NEXT / A7 NOT CREATED'
done

# F7 names the exact v5 generation observed by fresh R3 while preserving
# authority zero.  It must not import transport provenance or future A7/result.
V5_F7=native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v7.json
V5_F7_GATE=scripts/test_native_shadow_successor_producer_fingerprint_arm64_v7.py
require_file "$V5_F7"
require_file "$V5_F7_GATE"
require_text scripts/self-test.sh "$V5_F7_GATE"
require_text "$V5_F7_GATE" '3839d92c189a4a56d1d6a79a7fbfb2deaaadcf3dfaec3e636385c96aa106348c'
require_text "$V5_F7" '"schema": "boole.native-shadow.mac3.successor-producer-fingerprint.arm64.v7"'
require_text "$V5_F7" '"status": "SEALED-AFTER-FRESH-R3-PRODUCTION-GENERATION-NOT-AUTHORISED"'
require_text "$V5_F7" '"imageProductionRunsAllowed": 0'
require_text "$V5_F7" '"sha256": "44cd7d6feea2efc62d9ab6cb809e5d66c1452c9e4d2f034fd800e6573938fe87"'
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V7-SEALED'
  require_text "$doc" '3839d92c189a4a56d1d6a79a7fbfb2deaaadcf3dfaec3e636385c96aa106348c'
  require_text "$doc" '2,798-byte'
  require_text "$doc" 'R3 GREEN / F7 SEALED / A7 NOT CREATED / PRODUCTION AND BOOT NOT RUN'
done

# The pre-A7 review may clear the frozen code contract, but it must leave the
# irreversible production permission as an explicit operator boundary.
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'PRE-A7-RISK-REVIEW-V1-COMPLETE-AUTHORITY-NOT-GRANTED'
  require_text "$doc" 'PRE-A7 REVIEW COMPLETE / A7 NOT CREATED / PRODUCTION AND BOOT NOT RUN'
  require_text "$doc" 'administrator deletion is outside'
  require_text "$doc" 'code-only proof'
  require_text "$doc" 'provenance remains outside the authority lineage'
done

# The current cursor must distinguish free readiness evidence from the two
# explicit execution boundaries that have not run yet.
for doc in \
  docs/mac-first-hidden-linux-execution-plan-v1.md \
  docs/node-native-shadow-binding-containment-implementation-spec-v1.md \
  docs/native-submission-shadow-verification-v1.md; do
  require_text "$doc" 'CLOSED-LOCAL-IMAGE-READINESS-PREFLIGHT-GREEN-BUILD-AND-BOOT-NOT-RUN'
  require_text "$doc" 'f4d1e9c'
  require_text "$doc" '6af0b16'
  require_text "$doc" 'de96974'
  require_text "$doc" '33393135963'
done

# The third approved reversible observation reached the launcher but not
# readiness.  Keep the exact run, missing material and still-closed product
# boundary visible without granting another image or VM execution.
V3_MAC_RESULT=native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v3.json
require_file "$V3_MAC_RESULT"
require_text "$V3_MAC_RESULT" '"runId": 33466531840'
require_text "$V3_MAC_RESULT" 'closed-local-replay-registry-overlay-v1.json'
require_text "$V3_MAC_RESULT" '"productionRelease": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'THIRD ARM64 REPLICA PAIR BYTE-IDENTICAL'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT REVERSIBLE GATE: MAIN CI, THEN ZERO-IMAGE ARM64 PREFLIGHT'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Third reversible Mac observation: replay authority material was absent'

# The fourth approved development observation crossed every launcher startup
# boundary.  Readiness alone remained false, so preserve the exact failed-unit
# set and the development-only policy correction without weakening the rule.
V4_MAC_RESULT=native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v4.json
require_file "$V4_MAC_RESULT"
require_text "$V4_MAC_RESULT" '"runId": 33471902181'
require_text "$V4_MAC_RESULT" '"launcher-prerequisites": true'
require_text "$V4_MAC_RESULT" '"supervisor-privilege": true'
require_text "$V4_MAC_RESULT" '"readiness": false'
require_text "$V4_MAC_RESULT" '"readinessCriteriaRelaxed": false'
require_text "$V4_MAC_RESULT" 'etc/systemd/system/ldconfig.service'
require_text "$V4_MAC_RESULT" 'etc/systemd/system/serial-getty@.service'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'FOURTH ARM64 REPLICA PAIR BYTE-IDENTICAL'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'CRITERION UNCHANGED / DEVELOPMENT-ONLY UNIT MASKS + MOUNTED READBACK IMPLEMENTED'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Fourth reversible Mac observation: all launcher prerequisites passed'

# The fifth approved reversible observation is the first exact closed-local Mac
# readiness PASS.  Pin the build, the empty failed-unit set and every still-
# closed product boundary without turning readiness into activation authority.
V5_MAC_RESULT=native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v5.json
require_file "$V5_MAC_RESULT"
require_text "$V5_MAC_RESULT" '"runId": 33485969541'
require_text "$V5_MAC_RESULT" '"status": "CLOSED-LOCAL-MAC-READINESS-PASS"'
require_text "$V5_MAC_RESULT" '"failedUnits": []'
require_text "$V5_MAC_RESULT" '"submissionsObserved": false'
require_text "$V5_MAC_RESULT" '"productionRelease": false'
require_text "$V5_MAC_RESULT" '"testnetClaim": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'FIFTH ARM64 REPLICA PAIR BYTE-IDENTICAL / CLOSED MAC READINESS PASS'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: MAC.4 HOST-GUEST AUTHENTICATED CHANNEL, STILL CLOSED-LOCAL'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Fifth reversible Mac observation: exact closed readiness passed'

# The first closed-local MAC.4 round trip was attempted exactly once.  Preserve
# the successful replica comparison and the failed-closed channel observation
# without turning the result into node or product authority.
MAC4_CHANNEL_RESULT=native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v1.json
MAC4_CHANNEL_GATE=scripts/test_native_shadow_mac4_authenticated_channel_result_v1.py
require_file "$MAC4_CHANNEL_RESULT"
require_file "$MAC4_CHANNEL_GATE"
require_text scripts/self-test.sh "$MAC4_CHANNEL_GATE"
require_text "$MAC4_CHANNEL_RESULT" '"runId": 33510635018'
require_text "$MAC4_CHANNEL_RESULT" '"status": "MAC4-AUTHENTICATED-CHANNEL-OBSERVED-FAIL-CLOSED"'
require_text "$MAC4_CHANNEL_RESULT" '"comparisonStatus": "TWO-REPLICAS-BYTE-IDENTICAL"'
require_text "$MAC4_CHANNEL_RESULT" '"roundTrips": 0'
require_text "$MAC4_CHANNEL_RESULT" '"status": "SUFFICIENT-ROOT-CAUSE-IDENTIFIED"'
require_text "$MAC4_CHANNEL_RESULT" '"recommendation": "DETERMINISTIC-DEPMOD-INDEXES"'
require_text "$MAC4_CHANNEL_RESULT" '"newBootAuthorityRequired": true'
require_text "$MAC4_CHANNEL_RESULT" '"mac4Complete": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'MAC.4 AUTHENTICATED ROUND TRIP 0 / FAILED CLOSED'
require_text docs/native-submission-shadow-verification-v1.md 'MAC.4 authenticated-channel observation addendum'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'First MAC.4 authenticated-channel observation: vsock transport unavailable'

MAC4_MODULE_PREFLIGHT=native/containment/native-shadow-mac4-vsock-module-preflight-result-arm64-v1.json
require_file "$MAC4_MODULE_PREFLIGHT"
require_text "$MAC4_MODULE_PREFLIGHT" '"runId": 33519178333'
require_text "$MAC4_MODULE_PREFLIGHT" '"status": "GREEN-NO-IMAGE-NO-VM"'
require_text "$MAC4_MODULE_PREFLIGHT" '"imagesCreated": 0'
require_text "$MAC4_MODULE_PREFLIGHT" '"machinesStarted": 0'
require_text "$MAC4_MODULE_PREFLIGHT" '"name": "modules.dep.bin"'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'VSOCK MODULE OBJECTS + DEPMOD INDEXES + EXACT LOAD CONTRACT: PREFLIGHT GREEN'
require_text docs/native-submission-shadow-verification-v1.md 'MAC.4 vsock module preflight closure addendum'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'MAC.4 module discovery correction: zero-image preflight green'

# The second MAC.4 observation proved that module loading was fixed and then
# isolated the remaining relay failure to PrivateTmp on a read-only root.  Pin
# the failed-closed evidence and the additive successor without treating the
# unbuilt correction as a successful channel.
MAC4_CHANNEL_RESULT_V2=native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v2.json
MAC4_CHANNEL_GATE_V2=scripts/test_native_shadow_mac4_authenticated_channel_result_v2.py
MAC4_PRIVATE_TMP_SUCCESSOR=scripts/native_shadow_closed_local_image_to_readiness_arm64_v2.py
MAC4_PRIVATE_TMP_WORKFLOW=.github/workflows/native-shadow-closed-local-image-readiness-arm64-v2.yml
MAC4_PRIVATE_TMP_ISOLATED_CLI=scripts/native_shadow_closed_local_image_to_readiness_arm64_v3.py
MAC4_PRIVATE_TMP_ISOLATED_WORKFLOW=.github/workflows/native-shadow-closed-local-image-readiness-arm64-v3.yml
require_file "$MAC4_CHANNEL_RESULT_V2"
require_file "$MAC4_CHANNEL_GATE_V2"
require_file "$MAC4_PRIVATE_TMP_SUCCESSOR"
require_file "$MAC4_PRIVATE_TMP_WORKFLOW"
require_file "$MAC4_PRIVATE_TMP_ISOLATED_CLI"
require_file "$MAC4_PRIVATE_TMP_ISOLATED_WORKFLOW"
require_text scripts/self-test.sh "$MAC4_CHANNEL_GATE_V2"
require_text scripts/self-test.sh 'test_native_shadow_closed_local_image_to_readiness_arm64_v2.py'
require_text scripts/self-test.sh 'test_native_shadow_closed_local_image_to_readiness_arm64_v3.py'
require_text "$MAC4_CHANNEL_RESULT_V2" '"runId": 33569233592'
require_text "$MAC4_CHANNEL_RESULT_V2" '"missingPath": "/var/tmp"'
require_text "$MAC4_CHANNEL_RESULT_V2" '"runId": 33572058564'
require_text "$MAC4_CHANNEL_RESULT_V2" '"systemdStatus": "226/NAMESPACE"'
require_text "$MAC4_CHANNEL_RESULT_V2" '"status": "IMPLEMENTED-NOT-IMAGE-OR-BOOT-VERIFIED"'
require_text "$MAC4_CHANNEL_RESULT_V2" '"mac4Complete": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SECOND MAC.4 OBSERVATION: MODULES LOADED / RELAY BLOCKED BY ABSENT /var/tmp'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Second MAC.4 observation: PrivateTmp required an absent directory'

# The corrected disposable lane completed one authenticated host/guest round
# trip and observed exact guest readiness. Preserve the evidence while keeping
# node-owned execution, testnet and activation outside the claim.
MAC4_CHANNEL_RESULT_V3=native/containment/native-shadow-mac4-authenticated-channel-result-arm64-v3.json
MAC4_CHANNEL_GATE_V3=scripts/test_native_shadow_mac4_authenticated_channel_result_v3.py
require_file "$MAC4_CHANNEL_RESULT_V3"
require_file "$MAC4_CHANNEL_GATE_V3"
require_text scripts/self-test.sh "$MAC4_CHANNEL_GATE_V3"
require_text "$MAC4_CHANNEL_RESULT_V3" '"status": "MAC4-AUTHENTICATED-TRANSPORT-AND-READINESS-PASS"'
require_text "$MAC4_CHANNEL_RESULT_V3" '"runId": 33584005767'
require_text "$MAC4_CHANNEL_RESULT_V3" '"comparisonStatus": "TWO-REPLICAS-BYTE-IDENTICAL"'
require_text "$MAC4_CHANNEL_RESULT_V3" '"roundTrips": 1'
require_text "$MAC4_CHANNEL_RESULT_V3" '"channelAuthenticated": true'
require_text "$MAC4_CHANNEL_RESULT_V3" '"mac4Complete": false'
require_text "$MAC4_CHANNEL_RESULT_V3" '"nodeExecutionConnected": false'
require_text "$MAC4_CHANNEL_RESULT_V3" '"recommendation": "MAC4-NODE-ROUTE-BINDING"'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'MAC.4 AUTHENTICATED TRANSPORT + GUEST READINESS PASS'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Third MAC.4 observation: authenticated transport and readiness passed'

# The real curl-installed route now crosses the signed product boundary, one
# persistent closed Mac VM and the frozen four-case adjudication matrix. Keep
# the exact result while preserving every production/network boundary.
INSTALLED_MAC_E2E_RESULT=native/containment/native-shadow-installed-mac-e2e-result-arm64-v1.json
INSTALLED_MAC_E2E_GATE=scripts/test_native_shadow_installed_mac_e2e_result_v1.py
require_file "$INSTALLED_MAC_E2E_RESULT"
require_file "$INSTALLED_MAC_E2E_GATE"
require_text scripts/self-test.sh "$INSTALLED_MAC_E2E_GATE"
require_text "$INSTALLED_MAC_E2E_RESULT" '"status": "INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS"'
require_text "$INSTALLED_MAC_E2E_RESULT" '"runId": 33652402930'
require_text "$INSTALLED_MAC_E2E_RESULT" '"comparisonStatus": "TWO-REPLICAS-BYTE-IDENTICAL"'
require_text "$INSTALLED_MAC_E2E_RESULT" '"caseId": "accepted"'
require_text "$INSTALLED_MAC_E2E_RESULT" '"reasonCode": "intake_rejected"'
require_text "$INSTALLED_MAC_E2E_RESULT" '"macHarnessRuns": 3'
require_text "$INSTALLED_MAC_E2E_RESULT" '"activationAllowed": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'INSTALLED MAC CLOSED-LOCAL E2E PASS / REAL CURL INSTALL + FOUR VERDICTS'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Installed Mac E2E: the signed product route passed end to end'

# The installed Mac route now survives whole node/controller/guest loss without
# re-executing terminal submissions, and refuses unresolved InFlight on restart.
INSTALLED_MAC_CRASH_RESULT=native/containment/native-shadow-installed-mac-crash-restart-result-arm64-v1.json
INSTALLED_MAC_CRASH_GATE=scripts/test_native_shadow_installed_mac_crash_restart_result_v1.py
require_file "$INSTALLED_MAC_CRASH_RESULT"
require_file "$INSTALLED_MAC_CRASH_GATE"
require_text scripts/self-test.sh "$INSTALLED_MAC_CRASH_GATE"
require_text "$INSTALLED_MAC_CRASH_RESULT" '"status": "INSTALLED-MAC-CRASH-RESTART-EXACTLY-ONCE-PASS"'
require_text "$INSTALLED_MAC_CRASH_RESULT" '"checkerExecutionsAfterRestart": 0'
require_text "$INSTALLED_MAC_CRASH_RESULT" '"journalBytesUnchangedAfterRestart": true'
require_text "$INSTALLED_MAC_CRASH_RESULT" '"restartRefused": true'
require_text "$INSTALLED_MAC_CRASH_RESULT" '"activationAllowed": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'INSTALLED MAC CRASH/RESTART EXACTLY-ONCE E2E PASS'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Installed Mac crash/restart exactly-once closure'

# The curl-installed product now owns one verified foreground lifecycle and a
# typed live/ready surface. Pin the real Mac result without widening any
# production or network boundary.
INSTALLED_PRODUCT_LIFECYCLE_RESULT=native/containment/native-shadow-installed-product-lifecycle-result-arm64-v1.json
INSTALLED_PRODUCT_LIFECYCLE_GATE=scripts/test_native_shadow_installed_product_lifecycle_result_v1.py
require_file "$INSTALLED_PRODUCT_LIFECYCLE_RESULT"
require_file "$INSTALLED_PRODUCT_LIFECYCLE_GATE"
require_text scripts/self-test.sh "$INSTALLED_PRODUCT_LIFECYCLE_GATE"
require_text "$INSTALLED_PRODUCT_LIFECYCLE_RESULT" '"status": "INSTALLED-MAC-CLOSED-LOCAL-E2E-PASS"'
require_text "$INSTALLED_PRODUCT_LIFECYCLE_RESULT" '"schema": "boole.native-shadow.service-health.v1"'
require_text "$INSTALLED_PRODUCT_LIFECYCLE_RESULT" '"live": true'
require_text "$INSTALLED_PRODUCT_LIFECYCLE_RESULT" '"ready": true'
require_text "$INSTALLED_PRODUCT_LIFECYCLE_RESULT" '"mineableNow": false'
require_text "$INSTALLED_PRODUCT_LIFECYCLE_RESULT" '"activationAllowed": false'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'CURL-INSTALLED HOST LIFECYCLE + LIVE/READY HEALTH PASS'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: SIGNED UPDATE + ROLLBACK + CORRUPT-IMAGE/RESET RECOVERY E2E'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Curl-installed host lifecycle and health closure'

# The installed update state now keeps active selection separate from the
# monotonic security floors and one verified rollback generation. Runtime
# reset must preserve durable evidence and wallet state.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SIGNED UPDATE + ONE-GENERATION VERIFIED ROLLBACK + CORRUPT-ACTIVE RECOVERY GREEN'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'PRODUCT/GUEST FLOORS NEVER DECREASE / JOURNAL + WALLET SURVIVE RUNTIME RESET'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: INSTALLED UPDATE/ROLLBACK PROCESS-CRASH E2E'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Installed direct-boot update, rollback and reset lifecycle'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Permanent uninstall remains outside this contract'

# The real CLI now crosses the full reversible update lifecycle with two
# signed KAT generations; process-kill adoption windows remain the next gate.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'REAL CLI UPDATE -> ROLLBACK -> REPLAY REJECT -> CORRUPT-ACTIVE RECOVERY PASS'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: CRASH WINDOWS AROUND VERSION ADOPTION + STATE REPLACEMENT'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Real-process update/rollback/recovery command closure'

# Real CLI failure injection and SIGKILL now cover both sides of the atomic
# installed-release state replacement without granting a production failpoint.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'INSTALLED UPDATE PRE-STATE FAILURE PRESERVES OLD STATE BYTE-FOR-BYTE'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'POST-STATE SIGKILL REOPENS COMMITTED RELEASE / VERIFIED ROLLBACK CLEANS RESIDUE'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: SERIALIZE CONCURRENT PRODUCT MUTATIONS + ISOLATE DOWNLOAD STAGING'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Installed update interruption convergence'

# Product state mutations are single-writer and each CLI download uses an
# attempt-local sibling instead of a caller-shared staging tree.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'ONE INSTALL ROOT = ONE OWNER-HELD MUTATION LEASE / LOSER SENDS ZERO HTTP'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'DOWNLOAD BYTES LIVE IN A UNIQUE ATTEMPT SIBLING / CALLER PATH UNCHANGED'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: VERIFIED INSTALLED-RELEASE STATUS + OPERATOR DIAGNOSTICS'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Installed product mutation serialization'

# Installed release diagnostics re-authenticate bytes and report floors without
# repairing or selecting a generation.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'INSPECT = RE-AUTHENTICATE ACTIVE + ROLLBACK / REPORT FLOORS + RESIDUE'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'CORRUPT RELEASE = TYPED FAILURE / NO REPAIR / NO STATE CHANGE'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: CURL-FIRST USER WORKFLOW + RELEASE INPUT PACKAGING'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Verified installed-product inspection'

# An already-signed direct-boot product can now become one exact atomic
# transport tree without giving the CLI a private key or upload capability.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'SIGNED PRODUCT + SIGNED GUEST -> VERIFY ALL -> COPY VERIFIED HANDLES -> VERIFY AGAIN -> ATOMIC TREE'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'TAMPER / EXTRA / UNSAFE ENTRY = REJECT / OUTPUT ABSENT / PRIOR PACKAGE UNCHANGED'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: SUCCESSOR RELEASE PACKAGING + OPERATIONAL SIGNING/TRUST-ROOT DECISION'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Verified offline direct-boot release packaging'

# Product and guest successor histories are independently pinned; packaging
# still owns no private key and creates no production authority.
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'FIRST RELEASE = PINNED MINIMUM / SUCCESSOR = PREVIOUS SEQUENCE + EXACT MANIFEST DIGEST'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'PRODUCT HISTORY AND GUEST HISTORY BOTH MATCH OR OUTPUT DOES NOT EXIST'
require_text docs/mac-first-hidden-linux-execution-plan-v1.md 'NEXT: OPERATIONAL SIGNING KEY + PUBLIC TRUST-ROOT CUSTODY DECISION'
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md 'Authenticated successor release packaging'

printf 'docs-smoke: PASS\n' >&2
