//! RT-40 fixture: a strict binary crate where `main.rs` calls a named-arg
//! function defined in a sibling module file (`geom.rs`).
//!
//! Pre-RT-40 the wrapper built a per-file `CalleeRegistry`, so the lookup
//! for `make_rect` failed when lowering `main.rs` (the def lives in
//! `geom.rs`). The call site stripped names and kept declared order
//! silently — fine when the caller happens to spell args in declared
//! order, but no R3001 fires on a typo'd name. RT-40 makes the wrapper
//! build a *crate-wide* registry by walking the whole `src/` tree, so
//! the cross-file fn is now resolvable.
//!
//! The deliberate argument order at the call site is reversed
//! (`height: 5, width: 10`) so a successful build proves the registry
//! actually reordered the args — `make_rect` returns `(width, height)`,
//! so printing the `.0` field must yield `10`.

trust_attrs::strict! {}

mod geom;

fn main() {
    let rect = geom::make_rect(height: 5, width: 10);
    println!("{}", rect.0);
}
