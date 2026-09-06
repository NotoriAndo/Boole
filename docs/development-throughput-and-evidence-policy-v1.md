# Boole development throughput and evidence policy v1

Policy ID: **BOOLE-DEVELOPMENT-THROUGHPUT-AND-EVIDENCE-V1**

Status: **CURRENT — adopted 2026-08-31; clarified 2026-09-06**

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

## Authorization and instruction scope

User instructions and approvals persist across milestones within their stated
scope, budget, run count and time window. Do not ask again because a PR merged,
a helper is complete, or a skill asks for plan approval. A request to analyze
or discuss does not authorize implementation. A request to implement includes
the normal preparation, fixes, tests, PR and merge needed to finish that feature;
an explicit request to continue also covers the next work inside the approved goal.

Routine implementation choices and corrections to obsolete plans do not need a
new decision. Prepare the concrete proposal and finish safe independent work
before asking about an actual new boundary. Skills provide methods, not additional
authority requirements; prior agreement can satisfy their confirmation steps.
Use a short diagnostic loop for simple failures and deeper investigation for
uncertain ones. Static evidence and logs remain useful when execution is unavailable.

The shared entrypoint text is maintained in
[project-agent-guidance.md](project-agent-guidance.md). Current product state is
maintained in [current-development-status.md](current-development-status.md).
Historical instructions and advisory notebooks do not override this policy.

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

Four to eight hours is a sizing guide, not a mandatory delay or a stop after one
milestone when the user requested continued work. One final full CI is the target;
new corrective commits after a failed check still require fresh applicable checks.

A smaller PR is still appropriate for an urgent security fix, an independently
reviewable consensus change, or a change that cannot safely share rollback with
the surrounding milestone. The PR description must state that reason.

## TP2-BEHAVIOR-FIRST — tests and TDD

RED → GREEN remains mandatory for behavior changes. The RED test must fail for the
missing externally meaningful behavior, not merely because a sentence, field
order, file name or implementation detail changed.

Documentation, current-cursor edits and other non-behavioral corrections do not
require an invented failing test. Existing behavior may legitimately pass a new
regression test immediately; do not damage working code merely to manufacture RED.
Run local checks for the changed language and direct consumers. Rust fmt/clippy
are not local requirements for Python, shell or documentation-only changes.
Full workspace clippy and suite checks remain in the full CI lane.

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

Do not rerun an unchanged product after an acceptance failure hoping for a pass.
Fix it and verify the correction before a new normal development attempt inside
the existing development scope. This does not reset an explicit user-imposed
run or cost cap. After two infrastructure retries, investigate or change the
failed setup instead of looping unchanged; a material new boundary needs approval,
ordinary corrective work does not. Preserve failed outcomes.
If an unclassified result may hide a safety failure, hold the affected execution
and adoption while continuing safe investigation.

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
- public network/mining, real reward/consensus state or activation effects outside
  the existing approval,
  including public P2P; an approved synthetic loopback P2P test is not
  public network activation;
- paid model/API execution or a public benchmark/leaderboard claim; or
- an operational release/signing decision outside an already approved contract.

These are checks against existing authorization, not requirements to approve
the same action twice. Paid calls must fit the approved use and budget. An LLM
integration does not inherently select a particular paid API: inspect the actual
client, authentication and billing path before proposing a chargeable call.

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

Keep the current status at the top, edited in place. Link product plans and local
mirrors to the tracked current-status document instead of maintaining conflicting
current banners. Do not enforce a date or a completed milestone's wording as
permanently current in tests. Existing historical outcomes remain preserved.

`tasks/lessons.md` is an advisory incident notebook. A lesson becomes binding
only when explicitly adopted into this policy or the shared agent guidance.
The Master defines product invariants, not a second workflow constitution.
Historical lessons may guide investigation but cannot create
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

## TP8-CURRENT-AUTHORITY-BOUNDARY — development versus operation

Product progress and active approvals belong in the current-status document and
the user's instructions, not in this timeless procedure. Historical pre-A7, R3
and F7 records describe their own generation and grant no new operational authority.

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
