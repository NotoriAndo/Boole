pub struct Point { pub x: Field, pub y: Field }

pub fn norm(p: Point) -> Field { p.x * p.x + p.y * p.y }
