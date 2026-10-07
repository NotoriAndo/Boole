import ZkDet.ratchet_gnark_fixture_booletoy_Toy.Model
import ZkRatchet.ratchet_gnark_fixture_booletoy_Toy.Cand.Model

namespace ZkRatchet.ratchet_gnark_fixture_booletoy_Toy

/-- Ratchet equivalence for fixture/ratchet-gnark-toy `booletoy.Toy(api, c.In1, c.In2)` (std/booletoy/toy.go at 000000000000).
Reference: the registry DET model `ZkDet.ratchet_gnark_fixture_booletoy_Toy` of package `ratchet-gnark-fixture/booletoy.Toy`
(gnark pinned checkout, Go 1.25.7, R1CS sha256 `ca0bd2d10246e692dccdee915de674fe2550b11bab63fc4efb009a43ccd3b166`); its DET is machine-checked (mech-p3).
Record: 4 non-linear (5 total) constraints of the model.
Candidate `Cand`: source sha256 `5ff7239b1e62ba78841056662178db4c265cb5bb82a936b3eaf279771d1f351f`, compiled in the reference's wrapper
(model R1CS sha256 `8a21ce6e9fc85308de83f0ba0deef6c6476c5704b85c4f0a2ac90eecbb2a23a4`): 2 non-linear (3 total).
Inputs (the same wrapper variables): 2; outputs: 1 output wire(s).
For every input vector `x` and output vector `y`: some assignment satisfies
the reference constraints with these inputs and outputs iff some assignment satisfies the candidate
constraints with these inputs and outputs. -/
theorem equiv [Fact (Nat.Prime ZkDet.ratchet_gnark_fixture_booletoy_Toy.p)] :
    ∀ x y : List ZkDet.ratchet_gnark_fixture_booletoy_Toy.F,
      (∃ w : Fin ZkDet.ratchet_gnark_fixture_booletoy_Toy.nWires → ZkDet.ratchet_gnark_fixture_booletoy_Toy.F, ZkDet.ratchet_gnark_fixture_booletoy_Toy.Constraints w ∧ ZkDet.ratchet_gnark_fixture_booletoy_Toy.Inputs.map w = x ∧ ZkDet.ratchet_gnark_fixture_booletoy_Toy.Outputs.map w = y) ↔
      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by
  -- Ratchet test fixture proof (written for the tests): the reference computes y = a*a*b + a*(a*b) with four
  -- products, the candidate y = (a*a)*(b+b) with two; each direction supplies the other circuit's witness.
  intro x y
  constructor
  · rintro ⟨w, ⟨h0, h1, h2, h3, h4, h5⟩, rfl, rfl⟩
    refine ⟨![1, w 1, w 2, w 1 * w 1, w 1 * w 1 * (2 * w 2), w 7], ?_, rfl, rfl⟩
    show _ ∧ _ ∧ _ ∧ _
    refine ⟨rfl, rfl, rfl, ?_⟩
    show 1 * w 7 = w 1 * w 1 * (2 * w 2)
    linear_combination h5 - w 7 * h0 - h3 - h4 - w 2 * h1 - w 1 * h2
  · rintro ⟨v, ⟨g0, g1, g2, g3⟩, rfl, rfl⟩
    refine ⟨![1, v 1, v 2, v 1 * v 1, v 1 * v 2, v 1 * v 1 * v 2, v 1 * (v 1 * v 2), v 5], ?_, rfl, rfl⟩
    show _ ∧ _ ∧ _ ∧ _ ∧ _ ∧ _
    refine ⟨rfl, rfl, rfl, rfl, rfl, ?_⟩
    show 1 * v 5 = v 1 * v 1 * v 2 + v 1 * (v 1 * v 2)
    linear_combination g3 - v 5 * g0 - g2 - 2 * v 2 * g1

end ZkRatchet.ratchet_gnark_fixture_booletoy_Toy
