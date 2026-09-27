# Synthetic metered tuple corpus

License: Apache-2.0, the repository license. These task/answer bytes were
authored in this repository for verifier regression tests, not imported from
an external project or supplied by a model. They are public and permanently
non-issuable. Seed variants are not independent useful-work supply.

`task.json` defines the recurrence `acc = acc * 5 + field0 * 3 - bool(field1) * 2`,
starting at 7, with wrapping-i64 arithmetic. `answer.rs` is an answer **body** in
the restricted meter language, not a complete compilable Rust source file.

`corpus.json` freezes exact answer bytes and canonical verification results for
accept, wrong answer, runtime-fuel exhaustion, forbidden loop and invalid UTF-8.
`scripts/test_metered_tuple_reference.py` independently regenerates task/policy
identity, SplitMix64 inputs, recurrence output hashes and reviewed counter
formulas, without invoking the Rust verifier. To print a proposed replacement:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_metered_tuple_reference.py --emit
```

Review changes rather than automatically updating the golden when a test fails.
The Rust `tuple_verifier` integration test compares exact canonical result bytes
in the required Linux/macOS × debug/release verdict matrix. The local CLI test
also checks four fresh independent processes with differing irrelevant
environments. Four processes on one host are not four independent operators.

This is a new generated-task adapter candidate, not parity evidence for the
compiler-based V1 checker, a BF.3 receipt corpus, useful-task supply, model solve
evidence, universal program correctness, or a BF.7/BF.8 pass.
