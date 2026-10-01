mod inner;
use crate::inner::Point;
use std::hash::Hash;

global WIDTH: u32 = 3;
global DOUBLE: u32 = WIDTH * 2;

pub struct Acc<T, let N: u32> {
    items: [T; N],
    len: u32,
}

impl<T, let N: u32> Acc<T, N> where T: Eq {
    pub fn push(&mut self, x: T) {
        self.items[self.len] = x; // "comment with { brace"
        self.len += 1;
    }

    pub fn first(self) -> T {
        self.items[0]
    }

    pub fn make<let M: u32>(x: [T; M]) -> [T; M] { x }
}

impl Eq for Point {
    fn eq(self, other: Self) -> bool { self.x == other.x }
}

/* a block /* nested */ comment with fn fake() {} */
pub fn sum<let N: u32>(xs: [Field; N]) -> Field {
    let mut s = 0;
    for i in 0..N { s += xs[i]; }
    s
}

fn apply(f: fn(Field) -> Field, x: Field) -> Field { f(x) }

unconstrained fn hint(x: Field) -> Field { x }

#[test]
fn test_sum() {
    let a: Acc<Field, 4> = Acc { items: [0; 4], len: 0 };
    let _ = sum::<WIDTH>([1, 2, 3]);
    let _ = a.first();
}

pub fn main_like() -> Field { sum::<DOUBLE>([0; 6]) }

mod tests {
    use super::sum;
    fn helper() -> Field { sum::<5>([0; 5]) }
}
