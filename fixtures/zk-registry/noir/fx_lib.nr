#[export]
fn fx_mem(a: [Field; 5], i: u32, j: u32, v: Field) -> Field { let mut a = a; a[i] = v; a[j] + a[i] }
#[export]
fn fx_logic(a: u32, b: u32) -> u32 { (a & b) ^ (a | b) }
#[export]
fn fx_bb(x: [Field; 4], b: bool, px: Field, py: Field) -> (Field, Field) {
    let s = std::hash::poseidon2_permutation(x);
    let p = std::embedded_curve_ops::EmbeddedCurvePoint { x: px, y: py };
    let mut r = p;
    if b { r = std::embedded_curve_ops::embedded_curve_add(p, p); }
    (s[0], r.x)
}
unconstrained fn hint(x: Field) -> Field { x + 1 }
#[export]
fn fx_under(x: Field) -> Field { let h = unsafe { hint(x) }; h }
#[fold]
fn sq(x: Field) -> Field { x * x + 1 }
#[export]
fn fx_call(x: Field) -> Field { sq(x) + sq(x + 1) }
