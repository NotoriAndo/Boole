# Real frozen-accept parity fixture

This is the tracked, permanently non-issuable fixture for
`REAL-FROZEN-ACCEPT-PARITY-V1`. It proves that the answer-free tracked
`RUST-TUPLE-STRUCT-CHECKER-V1` release, run from a clean checkout with no
private experiment archive, independently reaches the same ACCEPT verdict on
the one real historical candidate recorded by census Entries 27/28 that the
original sealed checker reached at the time.

`accepted.rs` is the real model answer from that episode, copied verbatim
after the sealed episode driver's own extraction step (last `ACTION` line,
last fenced Rust code block, trailing newline). `anchor.rs` is the real
upstream `rust-lang/rust` corpus file the task was built from, reproduced
byte for byte; it is dual MIT/Apache-2.0 licensed. `task.json` is a fresh
task contract for the tracked checker's own domain-hash identity scheme,
built from the same real anchor digest and task seed as the historical
episode. `empty.rs`, `tampered.rs` and `constant.rs` are negative controls.
Cross-task binding is exercised against the existing public
`rust-tuple-struct-project-v1` fixture in the sibling directory, in both
directions, without duplicating its files here.

This fixture can never be issued as mining work: the underlying task was
already consumed by a real, one-shot model episode, and `nonIssuable` /
`activationAllowed` are both fixed `false` here and in `PROVENANCE.json`. No
author witness, model session metadata or private archive content is stored
here. `FROZEN-PARITY.json` records the frozen expected verdicts and the
explicit normalization used to compare the sealed and tracked verdict
vocabularies.

The fixture is Apache-2.0 for the Boole-authored files, except `anchor.rs`
which keeps its own upstream MIT/Apache-2.0 license as noted in
`PROVENANCE.json`.
