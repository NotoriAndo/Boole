pub struct MockH { pub x: Field }

impl crate::Hasher2 for MockH { fn h(self) -> Field { self.x } }
