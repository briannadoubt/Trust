// Cargo-style activation: a file marked as Trust-strict via the
// `trust_attrs::strict!{}` marker macro instead of the `#![strict]`
// inner attribute. This form is preferred for crates built by `cargo`
// because stock `rustc` accepts the macro invocation but rejects the
// unknown inner attribute.
//
// This file is intentionally rejected by `trust check` (R0001).
// It is NOT meant to be built via `trust build` — the marker
// macro requires the `trust-attrs` crate as a dependency, which
// the single-file build path cannot resolve. See the cargo_build.rs
// test in `crates/trust-attrs/tests/` for the cargo-side
// verification.

trust_attrs::strict! {}

fn main() {
    let x: Option<u32> = None;
    let _ = x.unwrap();
}
