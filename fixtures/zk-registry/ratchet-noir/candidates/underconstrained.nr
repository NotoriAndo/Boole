// Under-constrained candidate: the square is an unconstrained hint that the Brillig code computes correctly, so the
// simulation screen passes; nothing in the circuit pins the hint down, so no equivalence proof exists.
unconstrained fn square_hint(a: Field) -> Field {
    a * a
}

pub fn boole_ratchet_candidate(a: Field, b: Field) -> Field {
    // Safety: deliberately unchecked (test fixture)
    let s = unsafe { square_hint(a) };
    2 * s * b
}
