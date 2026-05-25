// Cargo-style activation: a file marked as Rustricted-strict via the
// `rustricted_attrs::strict!{}` marker macro instead of the `#![strict]`
// inner attribute. This form is preferred for crates built by `cargo`
// because stock `rustc` accepts the macro invocation but rejects the
// unknown inner attribute.
//
// This file is intentionally rejected by `rustricted check` (R0001).
// It is NOT meant to be built via `rustricted build` — the marker
// macro requires the `rustricted-attrs` crate as a dependency, which
// the single-file build path cannot resolve. See the cargo_build.rs
// test in `crates/rustricted-attrs/tests/` for the cargo-side
// verification.

rustricted_attrs::strict! {}

fn main() {
    let x: Option<u32> = None;
    let _ = x.unwrap();
}
