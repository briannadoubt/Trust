//! Bundled signature index of `rustricted-std`. The contents are generated
//! at build time by `build.rs` from `crates/rustricted-std/src/lib.rs`, then
//! included here so the lowering pass can resolve cross-crate named-arg
//! calls without runtime cargo metadata or file I/O.
//!
//! Each entry maps the *simple* (final-segment) function name to its
//! declared parameter list in order. The `CalleeRegistry` only ever
//! disambiguates by simple name — that matches how `preceding_ident` looks
//! up the callee at call sites — so collisions (same name, different
//! signatures across modules) are dropped at generation time.

include!(concat!(env!("OUT_DIR"), "/std_signatures.rs"));
