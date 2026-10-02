pub struct MockH { pub x: Field }

impl crate::Hasher2 for MockH { fn h(self) -> Field { self.x } }

mod test {
    pub struct HiddenH { pub x: Field }
    impl crate::Hasher2 for HiddenH { fn h(self) -> Field { self.x } }
}
