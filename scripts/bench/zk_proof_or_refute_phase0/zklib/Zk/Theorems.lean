/-
20 kernel-checked base theorems over the Zk primitives. These are the mutation
targets. Every statement is of the shape ∀ vars, Preconditions → Conclusion
(some with trivial/empty preconditions) so the mutation engine can add/drop
premises, flip relations, and change ranges deterministically.
-/
import Zk.Basic

namespace Zk

-- 1. boolean constraint ⇒ bit is 0 or 1 (over b ≤ 1 domain)
theorem thm01 : ∀ b : Nat, b ≤ 1 → boolConstraint b := by
  intro b hb; unfold boolConstraint
  rcases (by omega : b = 0 ∨ b = 1) with h | h <;> subst h <;> rfl

-- 2. a bit is < 2
theorem thm02 : ∀ b : Nat, isBit b → b < 2 := by
  intro b hb; rcases hb with h | h <;> omega

-- 3. range monotonic in k
theorem thm03 : ∀ x k : Nat, inRange x k → inRange x (k + 1) := by
  intro x k h; unfold inRange at *; have : 2 ^ k ≤ 2 ^ (k+1) := by
    have := Nat.pow_le_pow_right (by omega : 1 ≤ 2) (by omega : k ≤ k+1); omega
  omega

-- 4. bitsVal of a single bit ≤ 1 domain
theorem thm04 : ∀ b : Nat, b ≤ 1 → bitsVal [b] = b := by
  intro b _; simp [bitsVal]

-- 5. bitsVal of two bits
theorem thm05 : ∀ a b : Nat, bitsVal [a, b] = a + 2 * b := by
  intro a b; simp only [bitsVal]; omega

-- 6. carry: sum of two bits decomposes
theorem thm06 : ∀ a b : Nat, a ≤ 1 → b ≤ 1 → a + b = bitsVal [ (a+b) % 2, (a+b) / 2 ] := by
  intro a b ha hb; simp only [bitsVal]; omega

-- 7. polyEval constant
theorem thm07 : ∀ c x : Nat, polyEval [c] x = c := by
  intro c x; simp [polyEval]

-- 8. polyEval linear
theorem thm08 : ∀ a b x : Nat, polyEval [a, b] x = a + x * b := by
  intro a b x; simp [polyEval]

-- 9. polyEval at zero = constant term
theorem thm09 : ∀ c cs, polyEval (c :: cs) 0 = c := by
  intro c cs; simp [polyEval]

-- 10. dot of empty is zero
theorem thm10 : ∀ w : List Nat, dot [] w = 0 := by
  intro w; rfl

-- 11. dot singleton
theorem thm11 : ∀ a w : Nat, dot [a] [w] = a * w := by
  intro a w; simp [dot]

-- 12. r1cs trivial row (0 = 0)
theorem thm12 : ∀ w : List Nat, r1csSat [] [] [] w := by
  intro w; unfold r1csSat dot; simp

-- 13. boolConstraint holds at 0
theorem thm13 : boolConstraint 0 := by unfold boolConstraint; rfl

-- 14. boolConstraint holds at 1
theorem thm14 : boolConstraint 1 := by unfold boolConstraint; rfl

-- 15. range at k=0 forces x=0
theorem thm15 : ∀ x : Nat, inRange x 0 → x = 0 := by
  intro x h; unfold inRange at h; simp at h; omega

-- 16. two bits value < 4
theorem thm16 : ∀ a b : Nat, a ≤ 1 → b ≤ 1 → bitsVal [a, b] < 4 := by
  intro a b ha hb; simp only [bitsVal]; omega

-- 17. polyEval additive in constant
theorem thm17 : ∀ c d x : Nat, polyEval [c + d] x = polyEval [c] x + polyEval [d] x := by
  intro c d x; simp [polyEval]

-- 18. dot symmetric on singletons
theorem thm18 : ∀ a w : Nat, dot [a] [w] = dot [w] [a] := by
  intro a w; simp only [dot]; exact Nat.mul_comm a w

-- 19. inRange additive bound
theorem thm19 : ∀ x y k : Nat, inRange x k → inRange y k → x + y < 2 ^ (k+1) := by
  intro x y k hx hy; unfold inRange at *
  have h : 2 ^ (k+1) = 2 ^ k + 2 ^ k := by
    rw [Nat.pow_succ]; omega
  omega

-- 20. bitsVal nonneg-style: value of bits ≥ head bit
theorem thm20 : ∀ b bs, bitsVal (b :: bs) ≥ b := by
  intro b bs; simp only [bitsVal]; omega

end Zk
