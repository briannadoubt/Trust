//! End-to-end test: the marker macro compiles cleanly under stock rustc.
//!
//! This test exists so a future refactor that breaks "stock cargo can
//! build a file using trust_attrs::strict!{}" surfaces immediately.

trust_attrs::strict! {}

#[test]
fn strict_marker_compiles_and_is_a_noop() {
    // The macro expanded to nothing. If we got this far, the test
    // is essentially redundant — the assertion is just to keep
    // `cargo test` honest about reporting the test ran.
    assert_eq!(1 + 1, 2);
}
