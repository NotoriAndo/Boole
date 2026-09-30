"""Protocol-side ZK problem registry tools.

The modules in this package turn pinned circuit sources into Lean problem
packages.  They generate statements and run automatic gates; they never write
proofs of the generated statements.

* ``r1cs``            iden3 ``.r1cs`` / ``.sym`` reader and an independent R1CS evaluator
* ``circom_source``   light circom source scanner (templates, signals, includes, mains)
* ``circom_eval``     compile-time integer evaluation and ``for``-loop structure of template bodies
* ``instantiation``   instantiation rules for parametric templates
* ``tags``            known circom input tags as DET preconditions; the untagged wrapper main
* ``lean_emit``       Lean model / statement / harness emitters
* ``witness``         input sampling, witness generation and mutants
* ``det_search``      cheap searches for output-determinism counterexamples
* ``package``         ``problem.json`` assembly and validation
* ``jsonschema_lite`` validator for the JSON Schema subset used by the package schema
* ``lean_runner``     pinned Lean environment and process runner
* ``gates``           G-ELAB, G-NONVAC, G-FID, G-TRIV
* ``check``           generalized submission checker
* ``circom_det``      wave driver for the circom DET generator
"""

GENERATOR_NAME = "boole-zk-registry-circom-det"
GENERATOR_VERSION = "1.3"
