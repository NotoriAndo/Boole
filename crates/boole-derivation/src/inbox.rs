//! F0–F1 (D7–D8).
pub fn user_body(data: &[u8]) -> Result<&[u8], &'static str> {
    const BAD: &str = "INBOX-ENVELOPE-INVALID";
    if data.len() < 64 || data[..31].iter().any(|b| *b != 0) || data[31] != 32 {
        return Err(BAD);
    }
    let len = data[32..64]
        .iter()
        .try_fold(0usize, |n, b| {
            n.checked_mul(256)?.checked_add(usize::from(*b))
        })
        .ok_or(BAD)?;
    let end = 64usize.checked_add(len).ok_or(BAD)?;
    let item = data.get(64..end).ok_or(BAD)?;
    if item.first() != Some(&1) {
        return Err("ITEM-KIND-NOT-ACCEPTED");
    }
    Ok(&item[1..])
}
