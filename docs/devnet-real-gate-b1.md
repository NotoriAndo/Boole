# Real gate supervisor: Mac development branch

This slice consumes the [M3d-a data-only contract](devnet-real-gate-a1.md).
It activates no release, environment, clock, fork or devnet profile. Linux
production dispatch remains unavailable pending its isolated trusted controller
and boundary review. The descriptor still carries the complete two-profile
map; selecting Darwin never changes its gate or kernel-set identity.

`supervisor::Supervisor::new` keeps the legacy worker API. The separate
`real_supervisor::Supervisor::new_releases` accepts either the
unchanged legacy worker serialization or a real worker pointing to a canonical
`boole-m3-real/v1` descriptor. Historical descriptor bytes, the digest algorithm,
stub source and retained executable snapshots remain unchanged. Replay of an
old row uses its pinned old executable, never the new real descriptor. The
legacy enforced byte sandbox and its retry policy are unchanged.

The Rust real decoder checks the M3d-a canonical field order, integer grammar,
closed profiles, v2 rules, all release pins and the global kernel-set digest.
Shared fixtures run through both Rust and Python. Real replies retain their
closed row/action/reason/class channel, release and input digests, statement,
kernel execution states, exact raw debits and bounded charges. The consumer
independently resolves fitted fourfold allowances against the pinned authority;
completed work cannot exceed the package or slot cap. Missing executed work
never becomes zero. The explicit terminal output-limit intake exception keeps
unknown work with zero charge. Unknown reasons, including unproducible panic
rows, cannot enter the fold. Only an authorized published quarantine may
produce 0802; malformed authenticated packages use the distinct 0801 fingerprint.

The real authenticated cache stores the entire canonical reply. Its namespace,
MAC and path bind gate, queue index, commit, record kind, epoch and reconstructed
input binding. That binding includes package/export/environment identities,
instance seed, reconstruction/fork state and component addresses. Every hit
passes the current real reply and package validation. New descriptors or
artifact-table identities miss old entries; unavailable provisioning is never
replaced with a cached stub result.

Real retries use v2 observations: A2 waits at least 60 seconds and spawns a fresh
worker; A3 waits at least 600 seconds and needs class disagreement or a failed
canary. A pending A3 remains eligible after its 600-second backoff when the
canary recheck passes; that probe is not a completed A3. Slow retries after A3
or same-class exhaustion wait at least 3,600 seconds after a healthy canary PASS. D-node faults and canary probes do not count as submitted
attempts. D-node failures have a 60-second bounded retry delay, without
submitted-attempt accounting; global canary unavailability does not advance A1. No local
failure, repeated retry or timeout generates VOID or a quarantine transition.

The development D4 resolver consumes externally pinned, unsealed draft manifests.
Their canonical format is sorted UTF-8 JSON with one final LF, distinct from the
ordered ASCII real descriptor. It checks the complete draft/profile header,
manifest hash, ordered contiguous chunk list, lengths, each chunk hash and the
assembled hash. It opens one read-only chunk at a time and reads at most 64 KiB;
large states never enter a `GateInput` artifact vector or stdin. Existing
256 MiB bounded reads and the 600 MiB legacy stdin limit are not raised.
Verified files are synced and published read-only by a no-overwrite hard link.
The resolver leaves failed stages unpublished; callers own scratch lifetime.
This transport does not issue or authenticate a production EENV5 certificate.

S16 checks every real epoch, artifact pin, host/runtime release and configured
canary. Missing or mismatching required artifacts refuse startup, or block a running job
as DEFER(D-node), with no REJECT, VOID or submitted-attempt debit. Typed submission transport failures use the real closed `front_invalid` row
with zero kernel work, after package, cap and kind/lane decisions. A compressed package that cannot be
decoded retains its authenticated carried bytes for the VOID-only fingerprint;
the legacy `GateInput` serialization is unchanged.

All eight files in the recorded stub implementation hash are byte-identical to
the historical release. A new library facade compiles that unchanged source as
a module; real cache/rebuild/supervisor extensions live in separate files. A
fresh worker process verifies the recorded implementation identity and replays
a recorded tape with its fresh executable pin; verdict statements retain the
recorded gate identity.

The Mac worker invokes the pinned interpreter with fixed `-B -m
zk_registry.m3_gate` arguments and node-controlled paths, a cleared environment,
private scratch, an absolute deadline, separate bounded output channels and an
owned PID/birth inventory. Descendants are reaped even when a trusted tool starts
another process group. Resource failures remain availability failures; stderr
never supplies a mathematical reason. This development watchdog is separate
from the stub sandbox and does not claim Linux production containment.

`boole-gate-supervisor real-job-v1 WORKER CONTEXT BUNDLE PACKAGE OUTPUT SCRATCH
ENVELOPE` is a local qualification seam through the same real preparation,
execution and record validator. It starts no Engine or devnet node. Inputs are
content-bound files; the output file contains the canonical adapter reply.
Normal startup/install uses the descriptor-selected registry and its canary.

The separate `real-file-job-v1` qualification entry streams a content-bound D4
bundle file, up to the existing 256 MiB read ceiling. It covers adapter boundary
negatives such as an oversized reference without constructing a legacy stdin
packet. It does not change M2's 128 MiB admission limit or create a network input
route. Published package/mode/cap checks precede any oversized-bundle verdict; an
unadmitted full-mode package yields 0803 without accessing its reference.

Verify startup refuses the local public-counter qualification flag: the closed
v1 descriptor names no such verify mode. The local file/job qualification seam
still supports it, but it is included in the authenticated input binding. S16
also refuses Config statement_raw_max above the worker's 1 MiB package limit.
The M3 cap lookup requires the measured reference_export to equal the package
reference_sha256. A published mismatch is 0803 before reference/state access; a
missing local copy of a correctly bound reference stays D-node. Shared
certified-own vectors include positive executed-kernel counters, without
claiming a new native certified-state qualification.

PYTHONPYCACHEPREFIX points into each private scratch so existing source-tree
bytecode cannot override pinned Python source. The interpreter executable and
registry source are pinned; stdlib contents and system .pth processing are not
independently content-pinned or disabled by this Mac development slice.
Inventory failure falls back to killing the owned process group before leader
reaping. PID birth probes use lstart's one-second resolution; this watchdog is
not production isolation and cannot make a stronger PID-reuse guarantee.

Real dispatch follows A1's first-failure order: release/tools/inventory, complete
package address and canonical grammar (including the structured target), then
published mode/cap identity, measured reference equality, environment and scalar
slot allowance. No package-dependent reference or certified-state file is read
before these checks. Kind 2 precedes lane 2; both precede the submission decoder.
Only then are the full bundle address, OPEN environment, size and export checked.
An OPEN under another allowed environment is front_invalid/0106 with no kernel
work. Actual bound-address corruption retains the adapter input_integrity STOP
row. The supervisor alerts and backs off locally without turning that reply into
a miner verdict or inferring anchored L1 corruption. Independently authenticated
L1 integrity failures retain their existing typed stop.

Real compressed transport uses a separate typed libzstd API controller, not the
legacy decoder's exit-code classification. Node configuration may supply a
`decoder` object with `executable` and `runtime_library` paths; both are checked
against the existing Mac decoder/runtime release pins before use. The runtime is
copied into private scratch and authenticated again before loading. This does not
issue a new operational tool pin. The descriptor's profile `codec` pin remains
the contract JSON codec; it is distinct from this node-supplied transport binding.
Missing decoder provisioning is D-node. The embedded controller separates typed
frame/checksum failures from read/write/resource/tool/protocol failures, without
searching stderr or interpreting exit 1. Its canonical version-2 evidence binds
job context, gate identity, descriptor bytes, the trusted decoder pin and full
transport digest. Successful output also binds its actual digest and byte count.
Contradictory or foreign evidence is D-node; invalid-data requires explicit no
output. A matching output with invalid-data is contradictory, not a rejection.

Authorized quarantine retains a valid authenticated package statement. Only the
explicit NoValidStatement state, after completed package authentication, permits
the distinct BOOLE-M3/QUARANTINE-STATEMENT/v1 candidate fingerprint for 0802.
Unknown or unavailable package data cannot select it. It is refused for all other
outcomes/reasons, with no timeout-generated VOID or changed queue index.

The private preparation controller runs the pinned adapter prefix with fixed
in-process guards at `check_reference`, certified-state binding and the two
native execution entrypoints. Its first bound phase requests the reference only
after prior export/policy checks pass; its next phase checks the actual reference
and prior statement/axiom faults before requesting certified state. These guards
are private to a fresh preparation process and never edit contract source. A
materialization request is not a verdict. Prefix replies must have zero fuel,
not-run kernels, no counters and no ACCEPT, checked independently in Rust. The
final run uses the original adapter with every required artifact available.
This keeps missing local files from masking earlier submission failures.

One absolute deadline covers release hashing, D4 materialization, runtime copies,
artifact reads and execution. Preparation expiry and execution/recheck expiry use
counted transient T accounting. After output, artifact rechecks share one fresh
allowance bounded by the caller wall budget and 30 seconds; they do not consume
the kernel's remainder. Reads/writes check it around bounded
64 KiB operations; it is not kernel-level preemption of a blocked filesystem call.
The local `real-transport-job-v2` qualifier exercises this real typed transport
path using the same file arguments plus the envelope's `raw_bytes` binding. It
starts no node and grants no verify-mode public-counter admission.

The real submission decoder compares the decompressed SHA-256 with the committed
export digest before publishing output. A mismatch produces invalid-data/digest
with both decoded fields null and no output, yielding front_invalid/0106 without
reference/state access or kernel work. An `ok` channel carrying a mismatching
output remains a controller-integrity STOP control. Anchored compressed problem
packages use a separate package-purpose entry: their carried transport membership
is authenticated before decoding, and the submission export commitment does not
name package bytes.

The status decoder pin is a domain-separated digest of the authenticated libzstd
runtime and embedded helper source. A private provenance row records both hashes,
the interpreter hash and the legacy CLI cohort pin. The CLI is checked as existing
provisioning but does not perform decompression. This binds the components actually
executed without changing the contract channel or issuing an operational release.
Prepare and chunk-materialization controls inject slow reads only in libtest;
ordinary binaries contain no timing selector or fault hook.
