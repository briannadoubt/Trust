//! A plain-Rust "dependency" crate. It does NOT need to be strict-marked or
//! built with the Trust wrapper — its only job here is to expose a public API
//! whose parameter order a downstream agent could swap.
//!
//! `trust index src -o trust-signatures.txt` extracts the public-fn index;
//! the `consumer` crate then enforces named args against it (RT-66).

/// The canonical positional-swap footgun: two same-typed parameters whose
/// order is easy to get wrong. `make_rect(1080, 1920)` compiles and ships
/// the swap — unless the caller is forced to name the arguments.
pub fn make_rect(width: u32, height: u32) -> u32 {
    width * height
}

/// Three same-typed params — even easier to misorder.
pub fn clamp(value: i32, lo: i32, hi: i32) -> i32 {
    value.max(lo).min(hi)
}

// Private fns are not part of the public API, so they never appear in the
// generated index — a consumer can't (and shouldn't) name them.
#[allow(dead_code)]
fn internal(a: u32, b: u32) -> u32 {
    a + b
}
