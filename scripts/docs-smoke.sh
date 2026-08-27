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
require_text docs/native-submission-shadow-verification-v1.md "9196310572c35148676fae1656beb85126050f7db7f98bb2bc6fcb0be7071648"
require_text docs/native-submission-shadow-verification-v1.md "de6054d7aeab7e7136596a0aa91fa21784ed20d102b9f613660a421ef8118373"
require_text docs/native-submission-shadow-verification-v1.md "7d6fafb4376cadc679f99c9b6c5730bb505b72327934ca80989798cf5568aa20"
require_text docs/node-native-shadow-binding-containment-implementation-spec-v1.md "Linux/arm64 authority-parity closure addendum"
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

printf 'docs-smoke: PASS\n' >&2
