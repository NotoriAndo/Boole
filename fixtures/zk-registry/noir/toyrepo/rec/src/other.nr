use crate::Ctx;

pub struct Holder { pub inner: &mut Ctx, pub tag: Field }

pub fn read(h: Holder) -> Field { h.tag }
