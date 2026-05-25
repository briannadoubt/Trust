//! Effect tracking for Rustricted.
//!
//! Parses `fn f(...) -> T effect io + mut` annotations, builds a per-crate
//! effect table, infers each function's effect set from its direct callees
//! (intra-procedural), and reports declared sets that don't cover the
//! inferred effects. Annotations are stripped at lowering time — effects
//! have no runtime cost.

mod check;
mod parser;
mod registry;

pub use check::{check, std_seed, EffectCheck};
pub use parser::strip_effect_annotations;
pub use registry::{Effect, EffectSet, EffectTable};

/// The built-in effect names. Anything outside this list is treated as a
/// user-defined effect (still tracked, but unknown to the std seed table).
pub const BUILTIN_EFFECTS: &[&str] = &["io", "mut", "async", "panic", "unsafe"];
