/-!
# Boole.Family.V0Helpers — list operations and witness lemmas for
the v031 benchmark families

Ported from `projects/pof/lean/Boole/Family/V0Helpers.lean`.

Two op families coexist:

* The **lp-only subset** — `mapAdd k`, `mapMul k`, `sortAsc` —
  consumed by `boole.calibration.pow.v2` (Slice S8) for chains where
  `lengthPreserved` is the only invariant.
* The **full v0.2 subset** — adds `filterByPred p` and `dedup` — used
  by `boole.calibration.pow.v3` (Slice S8b) for the four "truthy"
  invariant branches (`allSatisfy`, `sortedAsc`, `dedupFirst`,
  `partitionEq`) plus the lp branch within the 5-way mixed family.

Each `theorem` below is the canonical witness for one branch's
goal. Proof bodies copy verbatim from pof against Lean 4.29.1; the
shared toolchain plus identical stdlib lemma surface (`List.all_filter`,
`Bool.not_or_self`, `List.eraseDups_cons`, `List.pairwise_mergeSort`,
`List.partition_eq_filter_filter`, `List.length_map`,
`List.length_mergeSort`) means the elaboration is byte-equivalent.

The `@[reducible]` annotation on each op definition matches pof's
convention so unfolding plays well with definitional reduction during
proof elaboration.
-/

namespace Boole.Family.V0Helpers

/-! ## Op definitions -/

@[reducible] def filterByPred (p : Int → Bool) (xs : List Int) : List Int :=
  xs.filter p

@[reducible] def mapAdd (k : Int) (xs : List Int) : List Int :=
  xs.map (fun x => x + k)

@[reducible] def mapMul (k : Int) (xs : List Int) : List Int :=
  xs.map (fun x => x * k)

@[reducible] def dedup (xs : List Int) : List Int :=
  xs.eraseDups

@[reducible] def sortAsc (xs : List Int) : List Int :=
  xs.mergeSort (fun a b => decide (a ≤ b))

/-! ## Witness lemmas (truthy-by-construction profile, v0.2 + v0.3 + v0.3.1) -/

/-- `allSatisfy p` witness: every element of `filterByPred p xs`
satisfies `p`. Closes any v3 instance whose last op is `filterByPred p`. -/
theorem all_filterByPred_self (p : Int → Bool) (xs : List Int) :
    (filterByPred p xs).all p = true := by
  unfold filterByPred
  -- After `List.all_filter` the body becomes `!p a || p a`, which is
  -- `Bool.not_or_self`; `simp` then reduces `xs.all (fun _ => true)` to `true`.
  simp [List.all_filter, Bool.not_or_self]

/-- `dedupFirst` witness: `dedup xs` has no duplicates. Closes any v3
instance whose last op is `dedup`. -/
theorem nodup_dedup : ∀ (xs : List Int), List.Nodup (dedup xs)
  | [] => by simp [dedup, List.eraseDups_nil, List.Nodup, List.Pairwise.nil]
  | a :: as => by
      unfold dedup
      rw [List.eraseDups_cons]
      have ih : List.Nodup (dedup (as.filter (fun b => !b == a))) := nodup_dedup _
      unfold dedup at ih
      apply List.Pairwise.cons
      · intro b hb
        have hmem : b ∈ as.filter (fun b => !b == a) := by
          rwa [List.mem_eraseDups] at hb
        have hne : b ≠ a := by
          rcases List.mem_filter.mp hmem with ⟨_, hbool⟩
          intro heq
          subst heq
          simp at hbool
        exact (Ne.symm hne)
      · exact ih
termination_by xs => xs.length
decreasing_by
  simp_wf
  exact Nat.lt_succ_of_le (List.length_filter_le _ as)

/-- `sortedAsc` witness: `sortAsc xs` is sorted in ascending order.
Closes any v3 instance whose last op is `sortAsc`. -/
theorem pairwise_sortAsc (xs : List Int) :
    List.Pairwise (· ≤ ·) (sortAsc xs) := by
  unfold sortAsc
  have htrans : ∀ (a b c : Int),
      (decide (a ≤ b)) = true → (decide (b ≤ c)) = true → (decide (a ≤ c)) = true := by
    intro a b c hab hbc; simp at hab hbc ⊢; omega
  have htotal : ∀ (a b : Int),
      (decide (a ≤ b) || decide (b ≤ a)) = true := by
    intro a b
    by_cases h : a ≤ b
    · simp [h]
    · simp [h]; omega
  have hp := List.pairwise_mergeSort htrans htotal xs
  exact hp.imp (by intro a b h; simpa using h)

/-- `partitionEq p` witness: `partition p xs = (xs.filter p, xs.filter (¬p))`.
Closes any v3 instance whose invariant is `partitionEq p` — no chain
witness op is needed since this is the stdlib `partition` semantics. -/
theorem partition_eq_filter_filter (p : Int → Bool) (xs : List Int) :
    xs.partition p = (xs.filter p, xs.filter (fun x => !(p x))) := by
  -- core stdlib `List.partition_eq_filter_filter` produces
  -- `(filter p l, filter (not ∘ p) l)`; rewrite the second component
  -- to the explicit lambda shape the generator emits.
  have h := @List.partition_eq_filter_filter Int p xs
  rw [show (not ∘ p) = (fun x => !p x) from rfl] at h
  exact h

/-! ## Length-preservation lemmas (`lengthPreserved` invariant)

Composed via `Eq.trans`, these close the `lengthPreserved` invariant
for any chain built from the length-preserving op family
`{mapAdd, mapMul, sortAsc}` (the v2 family + the lp branch within v3
restrict body ops to this subset). Composition shape, for chain
`mapAdd k₁ ▷ mapMul k₂ ▷ sortAsc`:

    fun xs =>
      (length_sortAsc _).trans
        ((length_mapMul k₂ _).trans (length_mapAdd k₁ xs))
-/

theorem length_mapAdd (k : Int) (xs : List Int) :
    (mapAdd k xs).length = xs.length := by
  unfold mapAdd; exact List.length_map ..

theorem length_mapMul (k : Int) (xs : List Int) :
    (mapMul k xs).length = xs.length := by
  unfold mapMul; exact List.length_map ..

theorem length_sortAsc (xs : List Int) :
    (sortAsc xs).length = xs.length := by
  unfold sortAsc; exact List.length_mergeSort _

/-- `filterByPred` can only remove elements, so v1 length-bound proofs may
compose this helper with the length-preserving op helpers above. -/
theorem length_filterByPred_le (p : Int → Bool) (xs : List Int) :
    (filterByPred p xs).length ≤ xs.length := by
  unfold filterByPred
  exact List.length_filter_le p xs

/-- `dedup` can only remove elements, so v1 length-bound proofs may compose
this helper with the length-preserving op helpers above. -/
theorem length_dedup_le : ∀ (xs : List Int), (dedup xs).length ≤ xs.length
  | [] => by simp [dedup, List.eraseDups_nil]
  | a :: as => by
      unfold dedup
      rw [List.eraseDups_cons]
      simp
      have ih : (dedup (as.filter (fun b => !b == a))).length ≤
          (as.filter (fun b => !b == a)).length := length_dedup_le _
      unfold dedup at ih
      exact Nat.le_trans ih (List.length_filter_le _ as)
termination_by xs => xs.length
decreasing_by
  simp_wf
  exact Nat.lt_succ_of_le (List.length_filter_le _ as)

end Boole.Family.V0Helpers
