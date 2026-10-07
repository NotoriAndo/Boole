import ZkDet.ratchet_fixture_toy_Toy.Model
import ZkRatchet.ratchet_fixture_toy_Toy.Cand.Model

namespace ZkRatchet.ratchet_fixture_toy_Toy

/-- Ratchet equivalence for fixture/ratchet-toy `Toy()` (toy.circom at 000000000000).
Reference: the registry DET model `ZkDet.ratchet_fixture_toy_Toy` of package `ratchet-fixture/toy.Toy`
(circom 2.2.3 `--O0`, R1CS sha256 `9140426886b795af4eb909c41bdac12d38697ebb4aae37a4e840e87c891fd715`); its DET is machine-checked (mech-p3).
Record: 2 non-linear (2 total) constraints (circom 2.2.3 `--O2`, main inputs public).
Candidate `Cand`: source sha256 `0e27bbd35567a37aefa0624c5beb845c519ca36f13406f8244821d669f5756f2`, compiled like the record
(canonical R1CS sha256 `4e9f7ba8dd7eb4321c9a6988b538df52a155f0ebc18790c8488fb72e1c5c75b5`): 1 non-linear (1 total).
Inputs (same names and order in both): `main.a`, `main.b`; outputs:
`main.y`.
For every input vector `x` and output vector `y`: some assignment satisfies the reference constraints
with inputs `x` and outputs `y` iff some assignment satisfies the candidate constraints with inputs `x`
and outputs `y`. -/
theorem equiv [Fact (Nat.Prime ZkDet.ratchet_fixture_toy_Toy.p)] :
    ∀ x y : List ZkDet.ratchet_fixture_toy_Toy.F,
      (∃ w : Fin ZkDet.ratchet_fixture_toy_Toy.nWires → ZkDet.ratchet_fixture_toy_Toy.F, ZkDet.ratchet_fixture_toy_Toy.Constraints w ∧ ZkDet.ratchet_fixture_toy_Toy.Inputs.map w = x ∧ ZkDet.ratchet_fixture_toy_Toy.Outputs.map w = y) ↔
      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by
  -- Ratchet test fixture proof (written for the tests): the reference computes y = a*b + a*b with two
  -- products, the candidate y = 2*a*b with one; each direction supplies the other circuit's witness.
  intro x y
  constructor
  · rintro ⟨w, ⟨_, h1, h2, h3⟩, rfl, rfl⟩
    refine ⟨![1, w 1, w 2, w 3], ?_, rfl, rfl⟩
    show _ ∧ _
    refine ⟨rfl, ?_⟩
    show (-(2 * w 2)) * w 3 = -w 1
    linear_combination h1 + h2 + h3
  · rintro ⟨v, ⟨_, h1⟩, rfl, rfl⟩
    refine ⟨![1, v 1, v 2, v 3, v 2 * v 3, v 2 * v 3], ?_, rfl, rfl⟩
    show _ ∧ _ ∧ _ ∧ _
    refine ⟨rfl, ?_, ?_, ?_⟩
    · show (-v 2) * v 3 = -(v 2 * v 3)
      ring
    · show (-v 2) * v 3 = -(v 2 * v 3)
      ring
    · show (0 : Cand.F) = -v 1 + v 2 * v 3 + v 2 * v 3
      linear_combination h1

end ZkRatchet.ratchet_fixture_toy_Toy
