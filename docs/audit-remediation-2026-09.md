# September 2026 audit follow-up

Scope: the 55 numbered claims in the September 6 audit were rechecked against
main `374f7bbb` (PR #370) before this follow-up. Claims are not independent
defects: several repeat the same issue, and several describe intentional
behavior or historical evidence. This document records the implemented
boundaries and deliberate exclusions, not a new whole-codebase security claim.

## September 7 independent-review remediation

A separate review of main `ae08171c` (after PR #371) reproduced nine further
recommendations. The `IR-F` labels below refer to that independent review, not
the earlier audit's F-series documentation claims. Its original pre-fix evidence
is preserved; the following describes the new implementation and regression
boundary.

| Independent finding | Fix and direct consumer verification |
|---|---|
| IR-F1, storage role collision | Reject aliases among mutable stores, their checkpoint/lock files, state metadata and read-only boot inputs before recovery or lock writes. Checker/family input trees are reserved as well. Existing canonical block and input bytes remain unchanged on refusal; read-only symlinks and read-only aliases remain supported. |
| IR-F2, future checkpoint trust | Discard checkpoints beyond the recovered store and reverify individual ingress below an unseen anchor. A busy checker cannot publish the block or advance its checkpoint. Full-candidate reorg may still reuse a prefix whose actual anchor hash matches. The rollback/resync CI smoke now requires re-verification and checkpoint reconstruction. |
| IR-F3, failed append continuation | Restore the previously confirmed length and fsync after partial append or sync failure. Failed rollback fences subsequent append and atomic rewrite by path/inode until process restart and recovery. Any uncertain owned ledger also makes that node's complete mutation surface and readiness fail closed; independent nodes remain usable. Signed HTTP consumers cover a real file-size-limited partial write and a failed session revoke followed by valid submission, retry and restart. |
| IR-F4, equal/shorter peer convergence | Compare heads rather than using height as the only sync trigger. Same-height forks and a shorter chain with more cumulative work traverse the existing bounded strict replay/fork-choice path; loopback peers converge without requiring another block. |
| IR-F5, Codex transport replacement | Switching an existing Boole HTTP entry to stdio removes HTTP-only transport/auth fields while preserving tool policy and unrelated servers. Generated config is tested through the installer; actual local Codex parsing is an additional opt-in consumer check. |
| IR-F6, slow MCP control path | One bounded HTTP-proxy slot serves native verification and legacy reads independently of protocol control. Ping/status/cancellation remain responsive; duplicate active IDs and id-less native execution are refused. Native cancellation does not fabricate a verdict or retry node-owned work. Bounded stdio queues and output deadlines also prevent an unread pipe from holding process shutdown. |
| IR-F7–IR-F9, portable process ownership | Mac readiness cleans owned process groups on interruption, benchmark invocation quotes executable paths containing spaces, and prewarming uses a Python deadline/process-group owner instead of requiring GNU `timeout`. Synthetic subprocess regressions exercise the failures without a VM or model. |

Direct follow-through on the same storage boundary also reproduced an additional
bounty bug: create, status and proof routes published registry state before a
failed audit append. They now preview under the existing writer lock, append
durably, then publish registry/side-pool state. All three HTTP failure/retry/restart
regressions pass; failed attempts retain the existing signed-nonce burn policy
and require a fresh signed intent to retry.

Independent review covers storage, consensus and tooling without implementation
ownership, with root review of consumer evidence and integration.
That review additionally caught the stale checkpoint CI expectation, a
cross-ledger authorization gap in the initial failure fence, checker-input alias
coverage, and a response-publication/request-slot retirement race. Corrections
are rechecked at those consumers before integration.
Focused checks are not a substitute for the containing PR's full required CI.
This is closure of these reproduced findings, not proof that every possible
defect in the repository has been eliminated.

New residual conditions: path-based append now fsyncs its parent directory on
every call; indeterminate writes deliberately remain fenced for process lifetime.
Deep competing chains can still exceed the existing sync-round budget and need
the separately deferred checkpoint/snapshot design. Native MCP transport shutdown
may wait for its existing 120-second upstream deadline to retain the real outcome;
it does not cancel the node's durable owner. No fresh VM, real-model/API, public
P2P, operational key, reward or activation execution is included.

## Remediation boundaries

| Audit references | Change and verification surface |
|---|---|
| A1–A4, A6–A9; D3; installer additions | Correct Claude Code/OpenCode configuration formats, Bash 3.2/non-TTY setup, bounded downloads, `boole` executable, single version prefix, ping/notification semantics, bounded/cancellable mining over stdio and HTTP. Installer and real transport regressions cover the consumers. |
| B1–B4, B7; D1–D2, D4 | Structural replay and fork choice precede expensive chain verification; matching verified prefixes are not rechecked. Block pages are bounded by serialized bytes. Required Hello commitments bind authorization posture and the effective family registry. HTTP connection/header/drain limits and outbound absolute deadlines cover partial peers. |
| C1–C6, C8; storage additions | Private defaults and proof workspaces, single-writer ledger ownership independent of state-dir, live lock-inode checks, same-descriptor recovery, exclusive session creation, and zero-on-drop secret buffers. State manifests compare explicit storage schema generations rather than package-version equality. Synthetic path/race and node-consumer tests cover preservation and refusal. |
| C9, D10 | Smoke outputs live in invocation-owned directories. Existing caller paths are rejected without deletion, and explicit new output paths are retained. Owned-node teardown is bounded and independent of HTTP connection counts. No real-model smoke is executed by the regression tests. |
| D5–D7; console close addition | Toolchain identity probes use the runner's time/output/process limits and respect its configured isolation. Native Mac readiness requires positive stop confirmation; start/stop/parent process deadlines and receipt/transcript write failures are explicit. Pure state and subprocess tests plus macOS compilation cover this without a VM boot. |
| E1–E5 | Lightweight CI parses executable shell gates. Blacklist tests require the actual intake-rejection reason. Required lanes fail if Lean is unavailable; canonical lenbound acceptance is explicitly invoked despite its ignored default. Missing required gitleaks fails. CI downloads have per-request deadlines. |
| A8, A10; F2, F4, F6–F9, F12 | Current response examples, six-case runtime manifest, replay rule diagnostics, optional legacy parity, crate descriptions and tracked progress are synchronized. Historical measurements are not rewritten. |

The P2P wire generation changes to require the new agreement commitments. Older
or mismatched peers fail closed. This does not change which legacy/named-network
proofs are valid, pin a new public genesis, activate a family, or authorize
promotion, reward, mining or public P2P. A launch still needs its independent
genesis/family activation authority.

Storage generation `node_storage: 1` is explicit. The known package `0.1.0`
legacy empty schema map can acquire that metadata once; unknown, missing,
additional or newer generations are rejected without partially rewriting the
manifest. Original creation timestamp/build provenance are retained.

Raw `boole-node run-local` defaults retain data across restarts under
`--state-dir`, or the per-user `XDG_STATE_HOME/boole/node-local` root. Without
XDG, macOS uses `~/Library/Application Support/Boole/node-local` and other Unix
hosts use `~/.local/state/boole/node-local`. Explicit ledger paths still win.
Old shared `/tmp` files are neither imported nor deleted automatically; use
explicit paths for a deliberate migration. Smoke fixtures always pass their
own block and reward paths.

Standalone toolchain metadata discovery keeps its portable Log posture while
adding process/output/deadline limits; evidence gathered by a configured Lean
runner follows that runner's default Enforce or explicit Log selection. The
metadata-only helper does not execute submitted proof code.

## Deliberate exclusions and residual conditions

- A5: returning supported MCP `2024-11-05` is valid version negotiation. Newer
  protocol generations are a separate feature, not silently claimed here.
- A11, B6, C7: doctor-after-checkout behavior, HTTP-only local session policy,
  and explicit miner `--force` replacement retain their documented meanings.
- B5: existing 16 MiB frame caps, ingress connection deadlines and peer limits
  remain. A separate pre-decode aggregate byte limiter is defense-in-depth,
  not evidence of an unbounded-allocation bug in the current path.
- D8: pipe-drain joins in explicitly weakened Log/insecure isolation remain a
  conditional risk. Default Enforce denies process-group escape and request
  spawning; this audit did not reproduce an escaped-descriptor hang there.
  No containment setting is weakened to make a test pass.
- E6: the real pre-lock function already runs through the required integration
  gate; no duplicate gate is added. F5's process-only lane is intentional under
  [TP9](development-throughput-and-evidence-policy-v1.md).
- F1, F3, F10, F11: historical outcomes and baseline inventory counts remain
  historical. Fixture evidence is not relabeled as real-model/public success.
- Long competing-chain checkpoint/snapshot strategy and lock-held candidate
  cloning remain bounded design/performance follow-ups, not newly implemented
  features. Corrupt derived reward ledgers still rebuild from canonical blocks.

## Evidence boundary

Focused regressions use temporary synthetic data, loopback peers, contained
Lean checks, and fake process callbacks. Full workspace and cross-platform
checks belong to the containing PR's CI. This change does not execute a new VM,
real model/API call, public network, operational wallet, release signing or
activation. Existing user changes and prior audit/experiment records remain
preserved.
