# Linux real-gate controller: host contract v0

The host interface and local artifact transport are implemented beside the Mac
real worker. Linux production execution remains unavailable. Startup reports the
missing backend, production contract, native causes, memory accounting, artifact
release, runtime closure and sandbox capabilities as a typed S16 refusal. There
is no readiness switch, fallback to the Mac adapter or partially qualified mode.

The historical stub's eight source-hashed files and byte-adapter sandbox API are
unchanged. Real execution uses separate modules. The pure fold, wire formats,
recorded tapes, Engine and gate contract Python are unchanged.

## Controller inputs and evidence

`real_controller::Request` accepts closed phase/tool enums, bounded local limits,
canonical descriptor and job identities, and verified artifact handles. No API
accepts a miner command, executable path, shell or arbitrary environment. Inputs
are bound to their independently expected digest, profile and role. The request
identity includes all context fields, descriptor bytes and gate identity, epoch,
decoder, transport, limits, phase/tool, artifact identities and a fresh nonce.
The deterministic input identity remains separate from that invocation nonce.
The invocation profile must exist in the descriptor. A typed release-pin provider
must independently supply the expected decoder and transport identities; unknown
or mismatched pins are refused. No production provider ships in this host slice.
Host dispatch accepts only `host_profile()` spellings (`darwin-aarch64` and
`linux-aarch64`); artifact/descriptor profile keys retain their separate spellings.

The private canonical evidence channel is at most 16 KiB. Foreign requests,
unknown fields/variants, alternate encodings and contradictory phase/tool values
are refused. Output channels are separately bounded by at most 64 KiB each.
Evidence that the OS boundary finished is not a mathematical verdict or a
production attestation: the existing strict real-reply validator remains required.
Evidence integrity and its disposition are checked before output capture limits:
missing, foreign or malformed status remains D-node even with oversized output.

Positive observed wall, memory, task and capture interruptions are transient T,
including preparation and recheck, and enter counted retry accounting. Missing
provisioning, protocol or backend evidence is D-node. Numeric tool exits never
establish logical caps or panic provenance. Every proposed native depth/panic/
cap/selection/memory claim remains D-node until the combined native release
supplies reviewed producers and consumers. This local channel is not that future
native status grammar and introduces no semantic REJECT/VOID row.

## Local content-address transport v1

`real_artifact::local::ResolverV1` consumes a caller-pinned canonical manifest.
Its exact sorted fields are `assembled`, `chunks`, `format`, `profile`, `role`,
`version`; format is `BOOLE-M3/LOCAL-ARTIFACT/v1`, version is integer 1.
`assembled` has `bytes`, `sha256`; each chunk has `bytes`, `index`, `object`,
`offset`, `sha256`. Metadata is at most 16 MiB and 100,000 chunks. Profiles are
case-exact Darwin-arm64 or Linux-aarch64; roles form a closed enum. SHA strings
are lowercase, object names exactly `chunks/<sha>`, offsets contiguous and chunks
nonempty and at most 64 MiB; every nonfinal chunk is exactly 64 MiB. Empty objects
have no chunks. This is a local integrity transport, not a D4 release schema,
certificate, EENV authentication or production permission. Unsealed historical
D4 drafts remain in their separate Mac development resolver, which now opens
regular files without following symlinks or blocking on FIFOs.

One read-only chunk handle and a 64 KiB buffer feed the assembled hash and a
private staging file. The output directory must be euid-owned mode 0700. A
deadline-bounded per-directory lock serializes publication and cached resolves.
The output directory's ancestors must be trusted: operations below the verified
directory are path-based, and integrity rests on the bound inode and content
hash.
The create-new descriptor is retained for writing, chmod and fsync; no stage
pathname is reopened for those operations. Regular-file opens use O_NOFOLLOW and
O_NONBLOCK, reject nonregular files, then clear O_NONBLOCK on the held descriptor.

Publication checks chunk and assembled hashes in one stream, fsyncs bytes and the
parent, and never overwrites an existing object. The staging link is removed
before the final inode-bound read-only handle is stamped, because unlink changes
ctime. Existing objects are fully reverified once; verification reads at most the
expected byte length plus one and reports the actual maximum read size.

One deterministic partial stage per content id bounds repeated interrupted
attempts. The next locked resolve retires that owned regular stage before retry
or cached-object verification. Symlink stages are refused. Cleanup failures
retain the original typed stop and retry class.

Returned handles retain the verified descriptor rather than reopen a pathname.
Readers have independent cursors and reject metadata/path replacement. Contents
are fully reverified before use. Read limits and stdin limits remain unchanged;
large runtime/state artifacts are never encoded as worker stdin byte arrays.
The real release's complete transitive artifact inventory and K4 integration
remain prerequisites; a local transport object alone cannot satisfy S20/K4.

## W0 fixture qualification and production boundary

Host tests cover explicit dispatch refusals, content/manifest/chunk validation,
read-only publication and held readers, bounded evidence, retry classes and
missing native producers. Host tests or compilation alone do not establish
Linux containment. The separately granted W0 generation 7 run passed all ten
fixture cases on October 30, 2026: 10 passed, 0 failed, 0 ignored in 182.47 seconds;
the complete window took 260 seconds. Verified input installation took 24.31
seconds and the offline locked build took 34.80 seconds. Packages, kernel identity
and nine masked services matched before and after. The final seal was complete
with 22,348 file hashes; the VM was confirmed stopped and the window released.
Three host UDP socket samples showed loopback endpoints only at those instants.

This result covers one Mac VZ aarch64 VM running mainline
`7.1.0-070100-generic` with Landlock ABI 9. The Ubuntu user-namespace restriction
was absent and was not exercised. The pinned AppArmor profile was loaded in
unconfined mode. This is not production-host qualification and grants no devnet
admission, reset or activation. Static stand-ins exercise the Comparator and
worker slots; this run does not qualify released native proof tools, their
runtime closure or missing production evidence producers.

The tested source commit was `ae7de1d45c6b798d46f9e5bb66b44f7cad668957`,
with source-manifest SHA-256
`43b6bf71eebed88f6a0a1364f56125e63701308d0c9e06205e40b21d6db2f2ba` and runner SHA-256
`f5eb0ffb8a434831a31fd507afb63de634486e66b388047caf5d2ca0d52be29c`. The following receipt and
retained-file index hashes identify this accepted run; new grants/nonces produce
new hashes on a repeat.

| Case | Receipt SHA-256 | Retained-file index SHA-256 |
|---|---|---|
| LIFE-deadlines | `fcd40d8784c430020c791c917cebbdb0381cdf8426befdbf9852a6c08c4a1c25` | `b8fbd2c6542f5d447927c1e5a4dfd7f86c6f39f825e8a06e1f20e185be3363f4` |
| LIFE-faults | `0d7663916146a0de2a57807fe3d0541b0394d090ff551fcf7560a0659675831c` | `3fbd1c3aa9cb8ca2e7ef2deb034589a1084623d295c737a499f0b82baff01562` |
| LIFE-parent-kill | `2c1febad9bb5c20869fb912b1220c3e473da0008b574f92d83ed21f1d5c595aa` | `ce7ebe55d207df9edd226894ad38f2df03009d76e0f81d58895b171a9e3cdc69` |
| LIFE-preparation | `d1178aee3e28717a8b65c9ba1e24c46af6008081601adc003a0f086ae7a1eb2c` | `9970869cc313fe2cb12eaa194f92f38f592a4a3d70d0c55fbad4b0bcb5601ab9` |
| LIFE-tree | `836d9e8dc5a061510c3678ab2c671a5b8096b160571506549d2e5b6172b1ecce` | `46f17cce3da8668fb69ac180b7a1991ca9bfa410aea9746bb3ce3add1124d11b` |
| SBX-capabilities | `477325ccad7db4a2e0f92a410855c5f0f6c75b56887255ff103d4fc482b4fb01` | `786da40465c1e7f8d2bf14a3b26d79345dbffecc60ba000138a3e91b35de794a` |
| SBX-exec | `523970df3fc44065f1fd2da8e379127828233befea57fcd4ca74cd0f19b93940` | `98bdb9cd404357bcf4e61994f28fa1534d90d3e6eeda12194540688170243c8a` |
| SBX-filesystem | `17b9c612ff95875d80403886b43bf1662f510e74a8d9ea11f781d7614425a064` | `2239b8df144df9b9321bf16a7f7e81f063d5595ec3204d353e8762036cd706ed` |
| SBX-network | `3aa35babe4331262055cb7575d326bfa8dcf287ab9da03cccece11b4fe1283bc` | `b369d7f0f6966c51a08505f5d5bcdb8eccf7ba2ffa7b96bae244f37d0ee45856` |
| SBX-tasks | `7c6d984497b3099b383fad9930129d54b6ab6861ead34fe971fd75cadff00939` | `1b150302b2e910d0503c9b4547f5737982dfc1eb270d0f3b8dc97da2f485f750` |

`tests/b2_w0.rs` contains these ten opt-in executable cases. They remain ignored
in ordinary CI: every repeat still requires an explicitly granted Linux window
and a pinned guest bundle. The [runner](../scripts/devnet/w0/runner.py) is pinned
by a tracked Rust constant. A guest build binds its static probe executable to
the reviewed source identity; the external authority pins the complete source
manifest. This is fixture provenance, not production attestation. An absent,
expired, changed or foreign grant refuses before qualification.

The fixture library/modules and binary require the `w0-fixture` feature and are
absent from default production builds. Only the selected offline clone with its
exact single work share is admissible; its VM home/disk and host logs remain
outside that share. Roots come from the external authority, never compiled host
paths. The authority ledger is outside the work root and read under the shared
lock at start and every host tick. Only orchestrator rows grant a strictly newer
generation; the runner can append `CONSUMED`/`RELEASED` only. A state change revokes the
window. Grants are time-bounded, host-revocable: ledger rows are unsigned and
orchestrator authorship remains a same-UID process rule. The host enforces
revocation by stopping the VM; a guest heartbeat stops qualification early. Its
clock-skew allowance is 30 seconds, plus three seconds of heartbeat age.
The v3 host-to-guest channel is the guest command's stdin, carried through
limactl/SSH to a dedicated watcher pipe. Fixed 4096-byte frames bind the sequence,
canonical JSON, zero padding and SHA-256. No decision reads post-boot host changes
through virtiofs: boot values are fixed before boot, and single-shot readers use
a guest-written snapshot of the latest stdin frame, with a three-second guest
monotonic freshness bound. The guest fsyncs an ACK on its stable share inode;
the host keeps one tick outstanding with a 10-second initial and 3-second later
ACK deadline, including close confirmation. Late valid ACKs still fail. Stream
fragmentation is assembled within the deadline; EOF, checksum, sequence gaps and
foreign grants fail closed. Torn shared ACK/snapshot reads retry for at most
200 milliseconds without extending the outer deadline or relaxing validation.
The watcher covers copies, build and cases and terminates its owned parent via
pidfd on failure. A two-phase close confirms the last tick before success.

Host evidence goes through pre-opened unshared log fds. Bounded stdin
deactivation cannot skip the finally-protected VM stop, even if a guest swaps
share files for FIFOs. Guest reads record source, inode, sequence, tick, time and
outcome; host records retain sends and ACKs. The preceding mask qualification
observed this stdin/SSH path for at least five seconds. Diagnostic-only three-second share
sampling compares held-fd, fresh-open, fadvise DONTNEED and O_DIRECT reads on
separate inodes; unavailable methods are recorded and never decide a window.
Mask inventory and package pins are durable before the first stdin skew check
and before the watcher starts, so a later liveness failure preserves them.
No success is inferred from host simulation. A separate mask/skew-only boot
stops before a fresh W0 grant; no containment qualification runs in that boot.

The separate Linux fixture backend uses private PID-1 evidence, six namespaces,
seccomp, Landlock ABI >=4, retained AppArmor/bubblewrap pins and delegated cgroup
v2 limits. Static tool stand-ins exercise the Comparator executable slot and four
nanoda-worker slots; they do not verify proofs or replace released native tools.
No dynamic loader, runtime-directory execute grant or executable scratch is
admitted. Kernel observations bind the invocation nonce, canonical event order,
capabilities and cleanup. Missing evidence stays unavailable even after a
physical interruption. Negative capability/cleanup consumer controls supplement
actual syscall, task-counter and process-tree probes; they do not disable global
kernel policy or establish a production profile.

The host observer independently checks PID-1 capabilities and namespace links
before allowing the private observer to apply Landlock and emit READY. READY
requires an enforced secret-read denial, not only a supported ABI; memory/task
limits and killing the outer observer have separate physical probes. Network
rows attribute socket EPERM to seccomp, not netns routing or resolver behavior;
namespace isolation is independently checked through proc links. EROFS on an
input write establishes a read-only bind, and ENOENT on an outside write
establishes mount isolation; neither is attributed to Landlock. The secret-read
EACCES probe plus enforced READY checks exercise Landlock. LIFE-tree records the
descendant PID, actual SID, session leader and adoption by PID-1; its pidfd row
checks identity binding, not a signal delivered through that descriptor. The
observer-kill witness must count at least four group members before SIGKILL.
Publication/cache/cleanup rows prefixed `consumer-` are explicit fixture consumer
controls, not archive/cache implementation qualification. Raw case receipts,
private events, stdout/stderr and observations are copied to new read-only files
with a fsynced hash list before scratch cleanup, including failed/unwound cases.
Destinations are reserved before execution; truncation and per-file errors remain
in the hash list and retain the original scratch. After confirmed VM stop, the
host moves the entire run outside the share, detaches hard-link aliases and
writes a final no-follow hash manifest before any later boot. A durable unsealed
marker refuses rebooking after a crash or incomplete custody until operator
recovery; failed evidence is never silently accepted or re-exposed.

The [guest recipe](../scripts/devnet/w0/guest.sh) builds only from per-run authenticated offline inputs,
then runs those ten cases. The [host window wrapper](../scripts/devnet/w0/window.py)
requires an external operator acknowledgement and exclusive quiet lock, reserves
the complete VM envelope, and confirms the selected VM stopped before releasing
its own lease. Host code runs only from an unshared hash-verified export; each
run gets a fresh verified guest copy. The clone has no agent/X11 forwarding,
copy-to-host, port forwards, proxy propagation or extra network attachments.
Input preparation, build and tests use the remaining monotonic 33-minute total budget,
including cleanup. Before build, the trusted kernel precheck exercises atomic
clone3 PIDFD, unprivileged cgroup entry and all limit files, file read-only binds,
writable scratch, chdir, setenv and fd-3 inheritance. An unconfirmed stop retains
the reservation. Preparation does not
run either script, configure/start a VM, self-grant a window or lift any production
startup refusal. The original byte-adapter sandbox API remains unchanged.

SBX covers network, filesystem/FD/exec restrictions, pinned children, capability
fail-closed and task caps. LIFE covers preparation/blocked reads, job/recheck
deadlines, escaped sessions/orphans, parent kill, PID reuse, spawn/capture and
cleanup failures. W0 qualifies these fixture controls in the stated VM scope.
Production Linux execution still requires the reviewed native/R1 release,
production contract and complete artifact closure; the fixture pass does not
lift startup refusals.

The filesystem probe removes its own FIFO and fsyncs the directory; custody still
refuses nonregular entries. SlowD4's intentionally blocked FIFO uses verified
guest-local tmpfs outside the retained share tree, including on timeout, kill or
retention failure. Post-READY hostagent UDP samples classify the NAME column as
loopback, non-loopback or unknown, retain raw output with PID birth checks, and
log ps/lsof start/end activity. Forwarding log messages alone do not prove a
bound socket; samples only describe their observation instants.


W0 uses the tracked launcher's AppArmor user-namespace rule: an absent
`apparmor_restrict_unprivileged_userns` is accepted as a kernel without the
Ubuntu restriction; a present value other than `1` refuses. The fixture never
sets it to `0`. Python and Rust apply the same rule; Landlock ABI >=4, cgroup v2
and every executable/profile pin remain mandatory. Absence is not evidence that
the Ubuntu restriction was exercised.

Guest evidence records kernel release/version, LSMs, AppArmor enabled and loaded
profile/mode, user namespace limit, Landlock ABI and sysctl presence/value.
A mask boot can additionally collect read-only compiler/std/static-libc, locked
registry archive/index, cgroup, syscall-availability, filesystem/tmpfs and budget
observations. These run after the first stdin ACK, without extending liveness
deadlines. Unavailable observations are retained as such, with per-category
receipts; they are informational and do not replace actual pre-build checks or
the ten containment cases. Invalid syscall arguments cannot clone a task, close
a descriptor or send a signal. No cache copy, build, download or case runs in a
mask boot; configuration observations do not certify enforcement.

W0 stages the three official Rust 1.95.0 aarch64 GNU distributions pinned in
[toolchain-v1.json](../scripts/devnet/w0/toolchain-v1.json), plus every registry
package in the workspace Cargo.lock, before boot. Host reads
of the operator-supplied cache authenticate every archive before an exclusive,
no-follow copy; the guest refuses extra, missing, aliased or nonregular inputs
and rechecks the entire pinned set before extracting anything. Each archive is
rechecked again on its held descriptor immediately before extraction.

The run-local installer copies only authenticated component payloads; it never
executes distribution installer scripts, rustup or ldconfig. Crates become a
Cargo directory source with the lockfile package checksum and generated hashes
for every extracted file. Cargo uses that source through run-local CARGO_HOME,
`--offline --locked`, an explicit run-local compiler and target directory. It
needs no registry index and never writes the guest's ordinary Cargo/Rust homes.
Archive traversal, links, special files, duplicate output and expansion beyond
the fixed bounds refuse. Preparation retains the existing 600-second cap and
remaining-time reserves; the 1,965-second outer deadline is unchanged. Host
measurements alone do not qualify guest extraction speed or build completion.
The accepted W0 run measured installation and build within these caps; different
hosts, kernels or artifact closures still require their own qualification.

Cargo resolves the entire workspace lock even when a single member is selected.
The staged crate names and checksums must therefore equal the full lockfile
registry set, including dependencies of unselected workspace members. The host
build control vendors exactly those staged archives into a fresh Cargo home;
removing an unrelated member's required archive fails the exact-set check and
its resolution negative control. A broader host-only cache is not evidence that
the staged input set can build.
