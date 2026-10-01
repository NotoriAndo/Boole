// Recovery R1 fixture: non-ABI parameter values, closure parameters, derived / test implementors.
mod mocks;
mod other;

pub trait Packable { fn pack(self) -> Field; }
pub trait Hasher2 { fn h(self) -> Field; }

#[derive(Packable)]
pub struct Note1 { pub v: Field }

pub struct Ctx { pub counter: u32, pub acc: Field }

pub struct Wrap<S> {
    pub ctx: &mut Ctx,
    pub storage: S,
    pub filter: fn(Field) -> Field,
}

fn keep(x: Field) -> Field { x }

impl<S> Wrap<S> {
    pub fn new(ctx: &mut Ctx, storage: S) -> Self { Self { ctx, storage, filter: keep } }
    pub fn bump(&mut self, x: Field) -> Field { self.ctx.counter += 1; (self.filter)(x) }
}

pub fn apply_twice(f: fn(Field) -> Field, x: Field) -> Field { f(f(x)) }
pub fn fold_with<Env>(xs: [Field; 3], f: fn[Env](Field, Field) -> Field) -> Field { f(xs[0], xs[1]) }
pub fn never_called(f: fn(Field) -> Field) -> Field { f(0) }

pub fn store<T>(x: T) -> Field where T: Packable { x.pack() }
pub fn hashed<H>(x: H) -> Field where H: Hasher2 { x.h() }

pub fn public_only(context: PublicContext) -> Field { 0 }
pub fn vec_len(v: [Field]) -> u32 { v.len() }

global CHUNK: u32 = 3;
pub fn chunks(xs: [Field; 8], n: u32) -> Field { let mut s = 0; for i in 0..n { s += xs[i]; } s }

#[test]
fn test_calls() {
    let offset = 3;
    let _ = apply_twice(|x| x + 1, 1);
    let _ = apply_twice(|x| x + offset, 1);
    let _ = fold_with([1, 2, 3], |a, b| a + b);
    let _ = apply_twice(keep, 2);
    let _ = chunks([0; 8], CHUNK);
}
