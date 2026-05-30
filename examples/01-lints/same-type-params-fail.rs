#![strict]

// R0017 — no-same-type-params.
//
// Named arguments (R0042) make the call site explicit, but `make_rect` still
// accepts two raw `u32`s — so a value computed into the wrong variable
// upstream ships the swap with correct-looking names. Distinct newtypes
// (`Width`, `Height`) make the swap a type error instead.
//
// The fix:
//     pub struct Width(pub u32);
//     pub struct Height(pub u32);
//     pub fn make_rect(width: Width, height: Height) -> u32 { width.0 * height.0 }

pub fn make_rect(width: u32, height: u32) -> u32 {
    width * height
}

fn main() {
    let _ = make_rect(width: 1920, height: 1080);
}
