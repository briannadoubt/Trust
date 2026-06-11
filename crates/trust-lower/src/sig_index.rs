//! Cross-crate signature index (RT-66).
//!
//! R0042 (no-positional-args) and the named-arg reorder pass resolve a call
//! site against the [`CalleeRegistry`](crate::named_args::CalleeRegistry).
//! That registry knows three things: the fns defined in the file being
//! lowered, crate-wide `extras` gathered from sibling files, and the bundled
//! `STD_SIGNATURES` index of `trust-std`. Everything else — every call into
//! an arbitrary third-party dependency — falls through to the positional
//! fallback, so the dialect's headline bug class (a same-typed positional
//! swap) can still ship across a crate boundary. That was the largest open
//! gap in the design.
//!
//! This module closes it. It extracts the public-fn signature index of *any*
//! crate from its source — the same `(simple_name, [params])` shape the
//! registry already consumes — serialises it to the checked-in manifest
//! format (`name:p1,p2`), and loads + merges dependency manifests at lowering
//! time. The flow is:
//!
//! 1. `trust index <dep-src>` (the CLI) runs [`extract_from_dir`] and writes a
//!    sidecar manifest via [`render_manifest`]. No hand-written shim — this
//!    works on any crate, unlike the bespoke `trust-std` index.
//! 2. The `trust-rustc` wrapper reads `TRUST_SIGNATURE_PATH`
//!    ([`SIGNATURE_PATH_ENV`]) via [`load_from_env`], parses each manifest
//!    with [`parse_manifest`], merges them with the crate-wide extras using
//!    [`merge`], and seeds the registry through
//!    [`lower_with_extra_callees`](crate::lower_with_extra_callees).
//!
//! **Why pub-only.** Only `pub fn`s are reachable from another crate, so the
//! index records public free fns (including inside `pub mod` blocks) and
//! nothing else. **Ambiguity policy** matches the rest of the toolchain: a
//! simple name that resolves to two different parameter lists is dropped
//! rather than guessed, because the registry disambiguates by simple name
//! only. Dropping degrades gracefully to the positional fallback — never to a
//! wrong reorder.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// A `(simple_fn_name, [param_names])` signature entry — the unit the
/// [`CalleeRegistry`](crate::named_args::CalleeRegistry) consumes.
pub type Signature = (String, Vec<String>);

/// Environment variable the `trust-rustc` / `trust-rustdoc` wrappers read to
/// discover dependency signature manifests. Holds a platform-separated list
/// (`:` on Unix, `;` on Windows — parsed via [`std::env::split_paths`]) of
/// manifest files and/or directories of `*.txt` manifests.
pub const SIGNATURE_PATH_ENV: &str = "TRUST_SIGNATURE_PATH";

/// Extract the public-fn signature index from a single Rust source string.
///
/// Parses with `syn` directly; if the source uses Trust syntax extensions
/// that vanilla `syn` rejects (named-arg call sites, pipe), it is lowered
/// first and re-parsed. Returns an empty index if neither parse succeeds —
/// extraction is best-effort and never panics on malformed input.
pub fn extract_from_source(source: &str) -> Vec<Signature> {
    extract_from_source_impl(source, false)
}

/// Like [`extract_from_source`] but includes **private** fns and fns in private
/// modules (RT-116). For a *within-workspace* migration (`trust fix` over a
/// tree) we can see every fn, and a positional call to a private cross-file fn
/// still needs naming. The registry matches by name, so a name that collides
/// with mismatched params across files still drops to the positional fallback
/// via [`merge`]. Do NOT use this for the cross-*crate* index — private fns
/// aren't callable from another crate.
pub fn extract_all_from_source(source: &str) -> Vec<Signature> {
    extract_from_source_impl(source, true)
}

fn extract_from_source_impl(source: &str, include_private: bool) -> Vec<Signature> {
    let Some(file) = parse_file_lenient(source) else {
        return Vec::new();
    };
    let mut sigs: SigMap = BTreeMap::new();
    let mut ambiguous: NameSet = BTreeSet::new();
    walk_items(&file.items, &mut sigs, &mut ambiguous, include_private);
    sigs.into_iter().collect()
}

/// Extract the public-fn signature index of a whole crate by recursively
/// walking a `src/` directory. Signatures from every `.rs` file are
/// accumulated into one index; a name that collides across files with
/// mismatched params is dropped (see module docs). Unreadable files and
/// parse failures are skipped.
pub fn extract_from_dir(src_dir: &Path) -> Vec<Signature> {
    let mut sigs: SigMap = BTreeMap::new();
    let mut ambiguous: NameSet = BTreeSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    walk_dir(src_dir, &mut sigs, &mut ambiguous, &mut visited, false);
    sigs.into_iter().collect()
}

/// Merge several signature indices into one, dropping any name that appears
/// with conflicting parameter lists across the inputs. Identical
/// re-declarations are de-duplicated. This is the union semantics the
/// registry's `extras` slot expects: when two sources disagree about a
/// name's parameters, falling back to positional (drop) is always safer than
/// guessing which signature the caller meant.
pub fn merge(indices: &[Vec<Signature>]) -> Vec<Signature> {
    let mut sigs: SigMap = BTreeMap::new();
    let mut ambiguous: NameSet = BTreeSet::new();
    for index in indices {
        for (name, params) in index {
            if ambiguous.contains(name) {
                continue;
            }
            match sigs.get(name) {
                Some(existing) if existing != params => {
                    sigs.remove(name);
                    ambiguous.insert(name.clone());
                }
                Some(_) => {}
                None => {
                    sigs.insert(name.clone(), params.clone());
                }
            }
        }
    }
    sigs.into_iter().collect()
}

/// Render a signature index to the manifest format shared with
/// `trust-std/std-signatures.txt`: a caller-supplied comment header followed
/// by one `name:p1,p2,...` line per entry (zero-arity fns render as `name:`).
/// Entries are emitted in sorted order for a stable, diff-friendly file.
pub fn render_manifest(entries: &[Signature], header: &str) -> String {
    let mut sorted: Vec<&Signature> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::new();
    if !header.is_empty() {
        out.push_str(header);
        if !header.ends_with('\n') {
            out.push('\n');
        }
    }
    for (name, params) in sorted {
        out.push_str(name);
        out.push(':');
        out.push_str(&params.join(","));
        out.push('\n');
    }
    out
}

/// Parse a manifest in the `name:p1,p2` format into a signature index.
///
/// Lenient by design — this reads files produced by other tools or hand-
/// edited: blank lines and `#` comments are skipped, and a malformed line
/// (missing `:`, empty name) is skipped rather than fatal. (The build-time
/// reader in `trust-lower/build.rs` is intentionally *strict* about the
/// bundled std manifest; this runtime reader is tolerant about external
/// ones.)
pub fn parse_manifest(text: &str) -> Vec<Signature> {
    let mut out: Vec<Signature> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, tail)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let params: Vec<String> = if tail.trim().is_empty() {
            Vec::new()
        } else {
            tail.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        };
        out.push((name.to_string(), params));
    }
    out
}

/// Load and merge signature manifests from a list of paths. Each path may be
/// a manifest file or a directory; directories are scanned (non-recursively)
/// for `*.txt` manifests. Unreadable paths are skipped — best-effort, like
/// the rest of the wrapper. The result is already [`merge`]d, so conflicting
/// names across the inputs are dropped.
pub fn load_paths(paths: &[PathBuf]) -> Vec<Signature> {
    let mut indices: Vec<Vec<Signature>> = Vec::new();
    for p in paths {
        if p.is_dir() {
            let Ok(read) = fs::read_dir(p) else {
                continue;
            };
            let mut files: Vec<PathBuf> = read
                .flatten()
                .map(|e| e.path())
                .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("txt"))
                .collect();
            // Deterministic order so `merge`'s de-dup is stable.
            files.sort();
            for file in files {
                if let Ok(text) = fs::read_to_string(&file) {
                    indices.push(parse_manifest(&text));
                }
            }
        } else if let Ok(text) = fs::read_to_string(p) {
            indices.push(parse_manifest(&text));
        }
    }
    merge(&indices)
}

/// Load the dependency signature index named by [`SIGNATURE_PATH_ENV`].
/// Returns an empty index when the variable is unset or empty — so a build
/// with no configured dependency indices behaves exactly as before.
pub fn load_from_env() -> Vec<Signature> {
    match std::env::var_os(SIGNATURE_PATH_ENV) {
        Some(val) if !val.is_empty() => {
            let paths: Vec<PathBuf> = std::env::split_paths(&val).collect();
            load_paths(&paths)
        }
        _ => Vec::new(),
    }
}

// --- internals -------------------------------------------------------------

type SigMap = BTreeMap<String, Vec<String>>;
type NameSet = BTreeSet<String>;

/// Parse a Rust file, falling back to lowering first if the raw source uses
/// Trust syntax extensions that vanilla `syn` rejects.
fn parse_file_lenient(source: &str) -> Option<syn::File> {
    syn::parse_file(source).ok().or_else(|| {
        crate::lower(source)
            .ok()
            .and_then(|lo| syn::parse_file(&lo.source).ok())
    })
}

fn walk_dir(
    dir: &Path,
    sigs: &mut SigMap,
    ambiguous: &mut NameSet,
    visited: &mut HashSet<PathBuf>,
    include_private: bool,
) {
    if !dir.is_dir() {
        // Allow passing a single `.rs` file too.
        if dir.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(source) = fs::read_to_string(dir) {
                if let Some(file) = parse_file_lenient(&source) {
                    walk_items(&file.items, sigs, ambiguous, include_private);
                }
            }
        }
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !visited.insert(canonical) {
            continue;
        }
        if path.is_dir() {
            walk_dir(&path, sigs, ambiguous, visited, include_private);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(source) = fs::read_to_string(&path) {
                if let Some(file) = parse_file_lenient(&source) {
                    walk_items(&file.items, sigs, ambiguous, include_private);
                }
            }
        }
    }
}

fn walk_items(
    items: &[syn::Item],
    sigs: &mut SigMap,
    ambiguous: &mut NameSet,
    include_private: bool,
) {
    for item in items {
        match item {
            syn::Item::Fn(f) => record_fn(&f.sig, &f.vis, sigs, ambiguous, include_private),
            // Only public modules expose their fns across the crate boundary; a
            // private `mod` hides everything inside it — except a within-
            // workspace migration (include_private), where a call into a
            // private module's fn still needs naming.
            syn::Item::Mod(m)
                if include_private || matches!(m.vis, syn::Visibility::Public(_)) =>
            {
                if let Some((_, inner)) = &m.content {
                    walk_items(inner, sigs, ambiguous, include_private);
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
    include_private: bool,
) {
    // Only `pub` fns are nameable by a downstream *crate*; a within-workspace
    // migration (include_private) names calls to any fn it can see.
    if !include_private && !matches!(vis, syn::Visibility::Public(_)) {
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
            syn::FnArg::Typed(pat_type) => match &*pat_type.pat {
                syn::Pat::Ident(pi) => params.push(pi.ident.to_string()),
                _ => {
                    // Destructuring param — can't be named at a call site.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(name: &str, params: &[&str]) -> Signature {
        (
            name.to_string(),
            params.iter().map(|s| s.to_string()).collect(),
        )
    }

    #[test]
    fn extracts_only_public_fns() {
        let src = "pub fn area(width: u32, height: u32) -> u32 { width * height }\n\
                   fn private_helper(a: u32, b: u32) -> u32 { a + b }";
        let idx = extract_from_source(src);
        assert_eq!(idx, vec![sig("area", &["width", "height"])]);
    }

    #[test]
    fn skips_self_and_records_pub_methods_are_not_collected_at_free_level() {
        // Inherent methods live in an impl, not at module level; the index
        // only records free fns. A free `pub fn` with `&self`-free params is
        // recorded normally.
        let src = "pub fn make(width: u32, height: u32) {}";
        let idx = extract_from_source(src);
        assert_eq!(idx, vec![sig("make", &["width", "height"])]);
    }

    #[test]
    fn recurses_into_public_modules_only() {
        let src = "pub mod geo { pub fn rect(w: u32, h: u32) {} }\n\
                   mod hidden { pub fn secret(a: u32, b: u32) {} }";
        let idx = extract_from_source(src);
        assert_eq!(idx, vec![sig("rect", &["w", "h"])]);
    }

    #[test]
    fn drops_ambiguous_names() {
        let src = "pub mod a { pub fn f(x: u32) {} }\n\
                   pub mod b { pub fn f(y: u32, z: u32) {} }";
        let idx = extract_from_source(src);
        assert!(idx.is_empty(), "conflicting `f` must be dropped: {idx:?}");
    }

    #[test]
    fn keeps_identical_redeclarations() {
        let src = "pub mod a { pub fn f(x: u32, y: u32) {} }\n\
                   pub mod b { pub fn f(x: u32, y: u32) {} }";
        let idx = extract_from_source(src);
        assert_eq!(idx, vec![sig("f", &["x", "y"])]);
    }

    #[test]
    fn drops_fns_with_destructuring_params() {
        let src = "pub fn f((a, b): (u32, u32), c: u32) {}";
        let idx = extract_from_source(src);
        assert!(
            idx.is_empty(),
            "destructuring param can't be named: {idx:?}"
        );
    }

    #[test]
    fn manifest_round_trips() {
        let idx = vec![
            sig("make_rect", &["width", "height"]),
            sig("now", &[]),
            sig("clamp", &["value", "lo", "hi"]),
        ];
        let manifest = render_manifest(&idx, "# test header");
        // Header preserved, sorted, zero-arity renders as `now:`.
        assert!(manifest.starts_with("# test header\n"));
        assert!(manifest.contains("\nnow:\n"));
        let parsed = parse_manifest(&manifest);
        // Round-trip equals the sorted input.
        let mut expected = idx;
        expected.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(parsed, expected);
    }

    #[test]
    fn parse_manifest_is_lenient() {
        let text =
            "# comment\n\n  make_rect : width, height \nbad line without colon\n:no_name\nnow:\n";
        let parsed = parse_manifest(text);
        assert_eq!(
            parsed,
            vec![sig("make_rect", &["width", "height"]), sig("now", &[])]
        );
    }

    #[test]
    fn merge_drops_conflicts_keeps_agreements() {
        let a = vec![sig("rect", &["w", "h"]), sig("dup", &["x"])];
        let b = vec![sig("circle", &["r"]), sig("dup", &["x", "y"])];
        let merged = merge(&[a, b]);
        // `dup` conflicts → dropped. `rect` + `circle` survive.
        assert_eq!(
            merged,
            vec![sig("circle", &["r"]), sig("rect", &["w", "h"])]
        );
    }

    #[test]
    fn merge_dedups_identical() {
        let a = vec![sig("rect", &["w", "h"])];
        let b = vec![sig("rect", &["w", "h"])];
        assert_eq!(merge(&[a, b]), vec![sig("rect", &["w", "h"])]);
    }

    // RT-116: the cross-crate index stays pub-only; the migration variant also
    // captures pub(crate) and private fns (and private modules).
    #[test]
    fn extract_all_includes_restricted_and_private() {
        let src = "pub fn a(x: u32) {}\n\
                   pub(crate) fn b(x: u32, y: u32) {}\n\
                   fn c(x: u32, y: u32) {}\n\
                   mod m { pub(crate) fn d(x: u32, y: u32) {} fn e(x: u32, y: u32) {} }";

        let pub_only: Vec<String> = extract_from_source(src).into_iter().map(|(n, _)| n).collect();
        assert_eq!(pub_only, vec!["a"], "cross-crate index must stay pub-only");

        let all: std::collections::BTreeSet<String> =
            extract_all_from_source(src).into_iter().map(|(n, _)| n).collect();
        for name in ["a", "b", "c", "d", "e"] {
            assert!(all.contains(name), "extract_all should include `{name}`: {all:?}");
        }
    }

    #[test]
    fn load_paths_reads_files_and_dirs() {
        let dir = std::env::temp_dir().join(format!("trust-sigtest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("dep_a.txt"), "rect:w,h\n").unwrap();
        fs::write(dir.join("dep_b.txt"), "circle:r\n").unwrap();
        fs::write(dir.join("ignored.md"), "not a manifest\n").unwrap();
        let loaded = load_paths(std::slice::from_ref(&dir));
        assert_eq!(
            loaded,
            vec![sig("circle", &["r"]), sig("rect", &["w", "h"])]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // The money test: a call into a "dependency" fn resolves against a loaded
    // index. Named args reorder to declared order; positional fires R0042.
    // This is the cross-crate enforcement that was the largest open gap.
    #[test]
    fn cross_crate_index_reorders_named_call() {
        let dep_index = vec![sig("make_rect", &["width", "height"])];
        let src = "fn main() { let _ = geo::make_rect(height: 5, width: 10); }";
        let out = crate::lower_with_extra_callees(src, &dep_index).unwrap();
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        let ten = out.source.find("10").expect("width arg");
        let five = out.source.find('5').expect("height arg");
        assert!(
            ten < five,
            "reorder to declared (width, height) order failed: {}",
            out.source
        );
    }

    #[test]
    fn cross_crate_index_fires_r0042_on_positional() {
        let dep_index = vec![sig("make_rect", &["width", "height"])];
        let src = "#![strict]\nfn main() { let _ = geo::make_rect(10, 5); }";
        let out = crate::lower_with_extra_callees(src, &dep_index).unwrap();
        assert!(
            out.diagnostics.iter().any(|d| d.rule == "R0042"),
            "positional cross-crate call must fire R0042: {:?}",
            out.diagnostics
        );
    }

    #[test]
    fn cross_crate_index_validates_unknown_param() {
        let dep_index = vec![sig("make_rect", &["width", "height"])];
        let src = "fn main() { let _ = geo::make_rect(width: 10, depth: 5); }";
        let out = crate::lower_with_extra_callees(src, &dep_index).unwrap();
        assert!(
            out.diagnostics.iter().any(|d| d.rule == "R3001"),
            "unknown cross-crate param must fire R3001: {:?}",
            out.diagnostics
        );
    }

    #[test]
    fn end_to_end_extract_then_resolve() {
        // Extract an index from a "dependency" source, then use it to resolve
        // a call in a "consumer" — the full RT-66 loop, no hand-written shim.
        let dep_src = "pub fn make_rect(width: u32, height: u32) -> u32 { width * height }\n\
                       fn internal(a: u32, b: u32) {}";
        let index = extract_from_source(dep_src);
        let manifest = render_manifest(&index, "# generated");
        let loaded = parse_manifest(&manifest);
        let consumer = "fn main() { let _ = dep::make_rect(height: 5, width: 10); }";
        let out = crate::lower_with_extra_callees(consumer, &loaded).unwrap();
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        let ten = out.source.find("10").unwrap();
        let five = out.source.find('5').unwrap();
        assert!(ten < five, "reorder failed: {}", out.source);
    }
}
