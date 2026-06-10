#![strict]

#[allow(trust::R0017, reason = "fixture models the same-typed-swap bug class")]
pub fn make_rect(width: u32, height: u32) -> (u32, u32) {
    (width, height)
}
