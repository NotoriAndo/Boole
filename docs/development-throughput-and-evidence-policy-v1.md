# Boole development throughput and evidence policy v1

Policy ID: **BOOLE-DEVELOPMENT-THROUGHPUT-AND-EVIDENCE-V1**

Status: **CURRENT — applies to work started after 2026-08-31**

Machine-readable operating values:

```text
defaultMilestoneHours: 4-8
maxInfrastructureRetriesAfterInitial: 2
fullCiRunsPerMilestone: 1
processOnlyHeavyCiRunsPerMilestone: 0
```

This policy governs how Boole development work is packaged, tested, recorded and
stopped. It changes workflow, not product truth. It does not weaken checker,
containment, replay, consensus, reward, release-signing or activation rules.

## TP1-MILESTONE-SEAM — default work unit

The default unit is one coherent, user-visible or operator-visible milestone that
can normally be completed in four to eight hours. A milestone crosses as many
files and internal modules as are necessary to close one real behavior boundary.

Examples of one boundary are: “the node accepts a raw answer and returns a durable
verdict,” “the guest boots and exposes readiness,” or “the wallet submits and can
recover its receipt.” A helper function, a document record, a digest refresh, a
test file or a wiring step is not a milestone by itself unless it independently
changes an observable contract.

`one slice, one boundary` therefore means one product or protocol seam, not one
file, one function, one frozen record or one PR. Intermediate commits are allowed.
The milestone normally gets one branch, one PR and one full CI run at the end.
Related docs-only and test-only edits are bundled into that PR.

A smaller PR is still appropriate for an urgent security fix, an independently
reviewable consensus change, or a change that cannot safely share rollback with
the surrounding milestone. The PR description must state that reason.

## TP2-BEHAVIOR-FIRST — tests and TDD

RED → GREEN remains mandatory for behavior changes. The RED test must fail for the
missing externally meaningful behavior, not merely because a sentence, field
order, file name or implementation detail changed.

During development, run focused tests. Prefer a compact set of direct contract
tests plus one to three end-to-end paths over dozens of assertions that restate a
document. There is no hard numerical cap when risk justifies more coverage, but a
new prose/shape assertion must explain which executable regression it prevents.

Keep these high-value gates:

- Linux process-tree containment and fail-closed behavior;
- crash/restart exactly-once replay;
- independent two-replica byte comparison where reproducibility matters;
- read-back, filesystem integrity and actual boot/service readiness;
- tamper, secret, identity and privilege-boundary rejection; and
- consensus, reward and activation invariants.

`docs-smoke` checks current status, links to authority, executable contracts and
security/release/consensus boundaries. It must not pin narrative prose line by
line or turn local planning text into a trust root.

## TP3-EVIDENCE-CLASS — what is frozen

Append-only evidence is reserved for facts that must remain independently
auditable:

- an executed measurement or experiment and its raw outcome;
- an externally observed run, failure, incident or security decision;
- consensus/release inputs and outputs whose exact bytes are authority;
- operational signing or activation decisions; and
- irreversible public or paid actions.

Plans, drafts, current cursors, implementation checklists, review notes and
ordinary design corrections are edited in place. Git history already preserves
their earlier states. They do not receive successor documents merely because a
sentence changed.

Pin exact bytes only when consumers execute, verify, distribute or make a trust
decision from those bytes. Do not hash prose, ignored local plans or test source
merely to prove that two planning documents were synchronized.

## TP4-BOUNDED-RETRY — reversible failures

A local or CI build, disposable image build, closed-local boot, preflight or test
is reversible unless it changes external state, publishes authority, spends
money, exposes a secret, or changes consensus/reward state.

For a reversible run, preserve every attempt and keep the acceptance criteria
unchanged. One initial run may be followed by at most two retries after a fix when
the failure is classified with evidence as harness, infrastructure or tooling
failure. A retry is a new recorded attempt, never a rewrite of the prior result.

Do not retry under the same authority when the product itself failed an
acceptance condition. Fix the product first and start a new normal development
attempt. If the failure cannot be classified, stop and ask for a decision.

“Exactly once” remains a runtime property for submissions, journals, rewards and
other state transitions. It is not a default limit on how many times engineers
may run a disposable build or boot test.

## TP5-HARD-STOP — when operator input is required

Stop and request an operator decision only for a genuinely material boundary:

- weakening or contradicting a security, containment or consensus invariant;
- changing sealed acceptance criteria after observing the result;
- an unclassified outcome that may hide a safety failure;
- secret, signing-key, credential or personal-data exposure;
- destructive or hard-to-recover deletion;
- public network/mining, reward, consensus, P2P or activation effects;
- paid model/API execution or a public benchmark/leaderboard claim; or
- an operational release/signing decision outside an already approved contract.

A deterministic compile error, missing file, wrong import, stale fixture,
incorrect comparison baseline, CI harness bug or disposable-image failure is an
ordinary RED/bug-fix event. Diagnose it, add the regression test, fix it and keep
working inside the milestone.

## TP6-DOCUMENT-SYNC — current plans and reports

Update Master Plan, Execution Order and the relevant product plan when a major
milestone closes or the dependency graph/current cursor changes. Do not append a
full execution diary for every commit, failed CI job, digest refresh or helper.

`EXECUTION-ORDER.md` contains only the current milestone, completed major
milestones and blocking triggers. Detailed evidence belongs in the tracked result
or incident record that actually owns it. User reports summarize the milestone;
they do not duplicate every test assertion.

`tasks/lessons.md` is an advisory incident notebook. A lesson becomes binding
only when explicitly promoted into this policy, the L1 Master constitution or the
agent instructions. Historical lessons may guide investigation but cannot create
new approval gates by themselves.

`tasks/todo.md` is likewise a historical task journal, not the current cursor.
Its older per-slice checklists do not override `EXECUTION-ORDER.md` or this
policy.

## TP7-HISTORICAL-SUPERSESSION — old records

Existing frozen records remain valid descriptions of what happened. Their run
IDs, digests, failures and decisions must not be rewritten. Their old procedural
instructions — one-record-per-PR, prose digest synchronization, unconditional
one-shot builds/boots and successor-on-every-edit — do not govern future work
where they conflict with this current policy.

An artifact-specific authority may be stricter than this policy only when it
protects a genuinely irreversible or externally visible effect and states that
effect explicitly. “It was done this way before” is not enough.

## TP8-CURRENT-AUTHORITY-BOUNDARY — native-shadow/Mac cursor

The 2026-08-31 pre-A7 review, R3 and F7 remain historical evidence. A7 has not
been created, and this policy does not create it, run production, boot a guest or
open MAC.4/testnet/mining/reward/consensus/P2P/activation.

Future closed-local image builds and boots are reversible engineering runs under
TP4, not inherently one-shot operational acts. Existing code that still requires
the historical A7 chain must not be bypassed silently: simplify or replace that
guard through normal TDD as part of the next coherent milestone, then run the
focused preflight and CI gates. Public release or activation still requires the
explicit authority described in TP5.

## TP9-PROCESS-ONLY-CI — do not rebuild the product for process edits

A change is process-only only when every changed path is on the narrow
documentation/process allowlist maintained by `scripts/ci_change_scope.py`.
Runtime code, native code, fixtures, dependencies, install scripts, unknown
paths and mixed changes fail closed to the full-validation lane.

Process-only changes do not install Rust or Lean, run supply-chain downloads,
rebuild release binaries, replay root filesystems, build arm64 launchers, or run
the four-platform verdict corpus. They run only the classifier tests, workflow
contract tests, current process-policy tests, `docs-smoke` and diff whitespace
validation. The branch-protection status names `self-test`, `supply-chain` and
`verdict-corpus` still appear and must pass; their process-only result means
“this change cannot affect the artifact governed by that heavy check,” not that
the heavy check ran.

The classifier and both workflows are themselves covered by the lightweight
contract gate. A workflow-dispatch run, empty range, unsafe path or classifier
uncertainty selects full validation. Product/runtime milestones still get the
one full CI run specified by TP1.

Unchanged state at adoption:

```text
mineable_now=0
REWARD_READY=0
RP0-MD=HOLD
BF.7=HOLD
Base activation=false
activationAllowed=false
```
