/// Toy reference of the Noir ratchet tests: 2 * a^2 * b, written as two separate products (4 multiplication terms).
pub fn toy(a: Field, b: Field) -> Field {
    let s = a * a * b;
    let t = a * b * a;
    s + t
}
