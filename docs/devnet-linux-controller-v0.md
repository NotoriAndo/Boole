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

## Qualification still pending

Host tests cover explicit dispatch refusals, content/manifest/chunk validation,
read-only publication and held readers, bounded evidence, retry classes and
missing native producers. They do not establish Linux containment.

`tests/b2_w0.rs` contains ten ignored executable acceptance cases, labelled
PENDING-W0. They require an actual Linux host, an active exclusive quiet-window
grant and a separately reviewed, digest-pinned probe fixture. That fixture is not
implemented or run by this host slice. Its pin is a tracked constant, currently
unset pending review, rather than an environment-supplied self-pin. An unset
fixture, tracked pin or grant fails if these cases are explicitly unignored;
there is no success-by-skip path. The fixture
must invoke the actual backend, run positive controls and return bounded evidence
for each exact assertion and an empty process tree. Its receipt is test evidence,
never a production capability token.

SBX covers network, filesystem/FD/exec restrictions, pinned children, capability
fail-closed and task caps. LIFE covers preparation/blocked reads, job/recheck
deadlines, escaped sessions/orphans, parent kill, PID reuse, spawn/capture and
cleanup failures. No Linux run or production profile is admitted until those
controls, D's native/R1 release and the complete artifact closure qualify.
