import ZkDet.ratchet_noir_fixture_toy_toy.Model
import ZkRatchet.ratchet_noir_fixture_toy_toy.Cand.Model

namespace ZkRatchet.ratchet_noir_fixture_toy_toy

/-- Ratchet equivalence for fixture/ratchet-noir-toy `toy(a0, a1)` (toy/src/lib.nr at 000000000000).
Reference: the registry DET model `ZkDet.ratchet_noir_fixture_toy_toy` of package `ratchet-noir-fixture/toy.toy`
(nargo 1.0.0-beta.25, ACIR sha256 `90c5987f14b211504d4f1b5ea36ffde542d1d0edc4741202db76922266088dbb`); its DET is machine-checked (mech-p3).
Record: 4 non-linear terms, 0 range bits, 0 logic bits, 0 memory units, black boxes: none (priced 4; free: 0 linear, 0 ABI range bits, 0 Brillig).
Candidate `Cand`: source sha256 `d9c9aaaeb05d8e86b03ea3613e5a1b9d7ec8bdfde806be2fb120323eca4bdd58`, compiled in the reference's wrapper
(ACIR sha256 `482226b3d45d213b496209b4875fe7cd9cfbadd195e46589ef69c5765d06f5d0`): 2 non-linear terms, 0 range bits, 0 logic bits, 0 memory units, black boxes: none (priced 2; free: 0 linear, 0 ABI range bits, 0 Brillig).
Inputs (same ABI, same witnesses): 2; outputs: 1 return witness(es).
Black boxes are uninterpreted and shared: `bb k` is black box k of the reference numbering (none),
keys new in the candidate follow.
For every interpretation of the black boxes, input vector `x` and output vector `y`: some assignment
satisfies the reference constraints with inputs `x` and outputs `y` iff some assignment satisfies the
candidate constraints with inputs `x` and outputs `y`. -/
theorem equiv [Fact (Nat.Prime ZkDet.ratchet_noir_fixture_toy_toy.p)] :
    ∀ (x y : List ZkDet.ratchet_noir_fixture_toy_toy.F),
      (∃ w : Fin ZkDet.ratchet_noir_fixture_toy_toy.nWires → ZkDet.ratchet_noir_fixture_toy_toy.F, ZkDet.ratchet_noir_fixture_toy_toy.Constraints w ∧ ZkDet.ratchet_noir_fixture_toy_toy.Inputs.map w = x ∧ ZkDet.ratchet_noir_fixture_toy_toy.Outputs.map w = y) ↔
      (∃ w : Fin Cand.nWires → Cand.F, Cand.Constraints w ∧ Cand.Inputs.map w = x ∧ Cand.Outputs.map w = y) := by
  -- Ratchet test fixture proof (written for the tests): the reference computes y = a*a*b + a*b*a with
  -- separate products, the candidate y = (2*a*a)*b; each direction supplies the other program's witness.
  intro x y
  constructor
  · rintro ⟨w, ⟨h1, h2, h3⟩, rfl, rfl⟩
    refine ⟨![w 0, w 1, w 2, 2 * w 0 * w 0], ?_, rfl, rfl⟩
    show _ ∧ _
    refine ⟨?_, ?_⟩
    · show 2 * w 0 * w 0 - 2 * w 0 * w 0 = 0
      ring
    · show -(w 1 * (2 * w 0 * w 0)) + w 2 = 0
      linear_combination h3 - w 0 * h2 - w 1 * h1
  · rintro ⟨v, ⟨h1, h2⟩, rfl, rfl⟩
    refine ⟨![v 0, v 1, v 2, v 0 * v 0, v 0 * v 1], ?_, rfl, rfl⟩
    show _ ∧ _ ∧ _
    refine ⟨?_, ?_, ?_⟩
    · show v 0 * v 0 - v 0 * v 0 = 0
      ring
    · show v 0 * v 1 - v 0 * v 1 = 0
      ring
    · show -(v 0 * (v 0 * v 1)) - v 1 * (v 0 * v 0) + v 2 = 0
      linear_combination h2 - v 1 * h1


end ZkRatchet.ratchet_noir_fixture_toy_toy
