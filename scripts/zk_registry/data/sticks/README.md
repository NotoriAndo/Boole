# Reproducible measuring sticks

These manifests reconstruct the GO stick sources from pinned public upstreams and ordered port repairs.
They introduce no issuance or activation and contain no upstream source checkout or study proof bundle.

```sh
bash scripts/setup_measuring_stick.sh riscv-v2 ./new-riscv
bash scripts/setup_measuring_stick.sh evm ./new-evm
```

Public identity consists of the source tree digest, definition digest and canonical type list with its digest.
The EVM definition digest is W1-derived and equals its complete port source digest. Canonical lists are stored
losslessly in `canonical-types.json.gz`; the loader verifies both compressed bytes and canonical JSON identity.
Source and definition maps use relative paths. Per-module source labels use neutral `stick://` identifiers.
Lean embeds source labels in some linter metadata, making olean hashes label-dependent. Historical olean
comparisons are optional private profiles and are outside public identity.

The default bootstraps Lean 4.34.1 and the pinned Mathlib dependency graph from public sources. Lake creates its
own pinned Git checkouts, then fetches the public compiled cache. The bootstrap creates an omitted library
directory only when its inventory is pinned to the empty map; missing nonempty roots and linked paths still fail.
It moves the path-dependent `Cache` tool artifacts outside all Lean search roots after downloading. The remaining
imports must match a second exact inventory pin. Supplied read-only bases are never changed and must match
either the complete original inventory or this exact tool-free inventory. Homes, `ELAN_HOME`, Lake and Mathlib caches stay
under the work directory. Bootstrap needs HTTPS (Python urllib), Git, a C toolchain, zstd and Python 3.9+.
Supported profiles are Darwin arm64 and Linux aarch64; x86_64 and other profiles fail closed.
An existing base is usable only after every search-root inventory and the compiler tree match:

```sh
bash scripts/setup_measuring_stick.sh riscv-v2 ./new-riscv \
  --base-env ./verified-base --cache ./archive-cache --offline
```

Each archive has a transport SHA256 and a content digest over relative file bytes, executable bits and symlink
targets. A regenerated archive or `git archive` of the pinned commit is accepted when its content matches.
Extraction writes regular members before links, rejects linked parents and checks the final link graph for
escapes. Special files and traversal are rejected. Cache filenames use the preferred transport digest.

Deterministic `native_decide` to `decide` and Sail `assert` to `sailAssert` rules have explicit file inventories;
small residual patches apply with zero fuzz in a minimal environment. All frozen source bytes and definition
digests must match. Patches mark modified files; notices retain upstream licences.

Public builds always initialize an empty private build directory and compile every root. `--resume` retains
verified sources and logs but recompiles all public modules, preventing forged cached artifacts or receipts from
becoming public build evidence. Every artifact consumed by the type observer is bound in memory to the current
compilation. Extra modules, loadable sidecars, linked or unreadable import subtrees fail closed. On an inventory
mismatch the builder writes the observed map. `--source-only` reports SOURCE-VERIFIED without a build PASS.
An explicitly supplied `--local-profile` may reuse artifacts only under its frozen historical hashes and recipes;
its source, definition and type identities must equal the public descriptor. Such profiles remain private.

Each process has a 19 GB guard and concurrent builders share a 40 GB semaphore. New launches pause below 6 GB
free plus inactive memory or 20 GB free disk. `--resource-root` shares the budget across output parents;
`--deadline-epoch` sets a cutoff. Queue time is recorded separately and only dead-PID leases are released.

The EVM port changes `native_decide` across 1,180 files. Recorded opaque conversions preserve public types while
limiting reduction. Deleted jump facts require separate E2 admission. RISC-V includes 13 RV64 word operations
and JALR v2. Its Sail extraction is unchanged except B2's mechanical `sailAssert` rename and the fixed-platform
definitions. The stick's own halt convention is t0 = 0 ECALL. The R3 test-only RAM mapping and a7 = 93/a0 = 0 exit
adapter are outside the stick. `stepExec`/`writtenAddrs` are unproved and JALR target alignment is unchecked.
CSR-environment and other unmodelled platform cases require separate validation.

Compilation and canonical type comparison are separate from proof admission. Re-admission proofs and historical
receipts are kept as private verification evidence. The reconstruction does not alter frozen study outcomes.
