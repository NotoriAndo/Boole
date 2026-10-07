// Smaller but not equivalent: the factor 2 is missing (caught by the simulation screen).
pub fn boole_ratchet_candidate(a: Field, b: Field) -> Field {
    a * a * b
}
