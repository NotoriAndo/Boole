"""Protocol-side ZK problem registry tools.

The modules in this package turn pinned circuit sources into Lean problem
packages.  They generate statements and run automatic gates; they never write
proofs of the generated statements.

* ``r1cs``            iden3 ``.r1cs`` / ``.sym`` reader and an independent R1CS evaluator
* ``circom_source``   light circom source scanner (templates, signals, includes, mains)
* ``circom_eval``     compile-time integer evaluation and ``for``-loop structure of template bodies
* ``instantiation``   instantiation rules for parametric templates
* ``tags``            known circom input tags as DET preconditions; the untagged wrapper main
* ``content``         content identity of templates (deduplication by content and parameters)
* ``decompose``       decomposition of TOO-LARGE instantiations into sub-component packages
* ``lean_emit``       Lean model / statement / harness emitters
* ``witness``         input sampling, witness generation and mutants
* ``det_search``      cheap searches for output-determinism counterexamples
* ``package``         ``problem.json`` assembly and validation
* ``jsonschema_lite`` validator for the JSON Schema subset used by the package schema
* ``lean_runner``     pinned Lean environment and process runner
* ``gates``           G-ELAB, G-NONVAC, G-FID, G-TRIV
* ``check``           generalized submission checker
* ``circom_det``      wave driver for the circom DET generator

AIR DET generator (zkVM chips; driver ``air_det``, its own generator name and source hash):

* ``air_ir``          extracted AIR IR (``boole-air-ir/v1``), window layout and an independent evaluator
* ``air_lean``        Lean model / statement / evaluation harness / battery emitters for AIR windows
* ``air_bus``         per-zkVM bus models (input, output, split, table-fact roles), table predicates, coverage
* ``air_search``      sound counterexample searches for AIR windows
* ``air_harness/``    the Rust extractors (std-only IR writer plus one adapter per zkVM)
"""

GENERATOR_NAME = "boole-zk-registry-circom-det"
GENERATOR_VERSION = "1.3"
