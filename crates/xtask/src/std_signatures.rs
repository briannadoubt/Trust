//! Signature-extraction helpers for `cargo xtask gen-std-signatures`.
//!
//! Lives in its own (intentionally NON-strict) file because the `syn`
//! visitor below uses `BTreeMap<String, Vec<String>>` in function
//! signatures. Trust's named-args lowering registry currently uses
//! a token-level scan that conflates the inner-generic comma in
//! `BTreeMap<K, V>` with a top-level parameter separator (RT-39 fixed
//! the common case for `HashMap<K, V>` via angle-depth tracking, but
//! nested types like `BTreeMap<String, Vec<String>>` still trip the
//! `>>` joint-spacing edge case). Also, `BTreeMap::insert` shares a
//! simple name with `trust_std::collections::insert`, which the
//! per-file callee registry can't distinguish from a free fn — so calls
//! to `sigs.insert(...)` would falsely fire R0042 under strict mode.
//!
//! Keeping this module out of strict mode is the pragmatic fix; the
//! underlying gaps are tracked as RT-41 (path-aware callee matching)
//! and a generic-depth refinement to `split_by_top_comma`.

use std::collections::{BTreeMap, BTreeSet};

pub type SigMap = BTreeMap<String, Vec<String>>;
pub type NameSet = BTreeSet<String>;

/// Walk every `Item::Fn` and nested `Item::Mod` in `items`, recording the
/// (simple name, [params]) signature of each `pub fn`. Names whose
/// declarations collide across modules with mismatched parameter lists
/// are dropped — the downstream `CalleeRegistry` only ever disambiguates
/// by simple name, so an ambiguous entry would silently mis-lower.
pub fn walk_items(items: &[syn::Item], sigs: &mut SigMap, ambiguous: &mut NameSet) {
    for item in items {
        match item {
            syn::Item::Fn(f) => record_fn(&f.sig, &f.vis, sigs, ambiguous),
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    walk_items(inner, sigs, ambiguous);
                }
            }
            _ => {}
        }
    }
}

fn record_fn(
    sig: &syn::Signature,
    vis: &syn::Visibility,
    sigs: &mut SigMap,
    ambiguous: &mut NameSet,
) {
    // Only `pub` fns can be named by downstream call sites.
    if !matches!(vis, syn::Visibility::Public(_)) {
        return;
    }
    let name = sig.ident.to_string();
    if ambiguous.contains(&name) {
        return;
    }
    let mut params: Vec<String> = Vec::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => {} // skip self
            syn::FnArg::Typed(pat_type) => match pat_ident(&pat_type.pat) {
                Some(ident) => params.push(ident),
                None => {
                    // Destructuring pattern — can't bind by name.
                    sigs.remove(&name);
                    ambiguous.insert(name);
                    return;
                }
            },
        }
    }
    match sigs.get(&name) {
        Some(existing) if existing != &params => {
            sigs.remove(&name);
            ambiguous.insert(name);
        }
        Some(_) => {}
        None => {
            sigs.insert(name, params);
        }
    }
}

fn pat_ident(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pi) => Some(pi.ident.to_string()),
        _ => None,
    }
}

/// Render the (sorted) signature map to the `std-signatures.txt`
/// manifest format: a comment header (caller-supplied) followed by one
/// `name:p1,p2,...` line per entry.
pub fn render_manifest(sigs: &SigMap, header: &str) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push('\n');
    for (name, params) in sigs {
        out.push_str(name);
        out.push(':');
        out.push_str(&params.join(","));
        out.push('\n');
    }
    out
}
