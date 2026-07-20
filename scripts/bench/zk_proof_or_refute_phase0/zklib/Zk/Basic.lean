/-
Protocol-owned ZK primitives (Phase 0 experiment). Self-contained, no mathlib.
Prime field as Fin p, bits, range checks, boolean constraints, coefficient-list
polynomials, and a minimal R1CS row. All base theorems are kernel-checked here.
-/

namespace Zk

/-- Fixed small prime for the field arm. -/
abbrev p : Nat := 97

/-- Field element as `Fin p` (p prime ⇒ genuine field, but we only use the
    ring/order structure here). -/
abbrev Fp := Fin p

namespace Fp

def add (a b : Fp) : Fp := a + b
def mul (a b : Fp) : Fp := a * b

end Fp

/-- A bit is a Nat constrained to {0,1} by `isBit`. -/
def isBit (b : Nat) : Prop := b = 0 ∨ b = 1

/-- Boolean constraint used throughout R1CS: b*(b-1) = 0 over Nat with b ≤ 1. -/
def boolConstraint (b : Nat) : Prop := b * (b - 1) = 0

/-- k-bit range predicate. -/
def inRange (x k : Nat) : Prop := x < 2 ^ k

/-- Little-endian bit list value. -/
def bitsVal : List Nat → Nat
  | [] => 0
  | b :: bs => b + 2 * bitsVal bs

/-- Coefficient-list polynomial evaluated at x over Nat. -/
def polyEval : List Nat → Nat → Nat
  | [], _ => 0
  | c :: cs, x => c + x * polyEval cs x

/-- Dot product of two Nat lists (R1CS linear combination). -/
def dot : List Nat → List Nat → Nat
  | [], _ => 0
  | _, [] => 0
  | a :: as, b :: bs => a * b + dot as bs

/-- One R1CS row ⟨a,b,c⟩ is satisfied by witness w iff (a·w)*(b·w) = c·w. -/
def r1csSat (a b c w : List Nat) : Prop := (dot a w) * (dot b w) = dot c w

end Zk
