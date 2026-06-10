//! Lowering passes: Trust source → plain Rust source.
//!
//! Each pass is a token-level rewrite over `proc_macro2::TokenStream`:
//! - [`pipe`] desugars `e |> f(args)` to `f(e, args)`.
//! - [`named_args`] rewrites `f(name: value, ...)` to positional based on
//!   a per-crate callee registry built from local function signatures.
//!
//! The driver wires these together via [`lower`].

use proc_macro2::TokenStream;
use thiserror::Error;
use trust_diag::Diagnostic;

pub mod named_args;
pub mod pipe;
pub mod preprocess;
pub mod rule;
pub mod sig_index;
mod std_signatures;

pub use rule::Rule;

#[derive(Debug, Error)]
pub enum Error {
    #[error("token parse error: {0}")]
    Lex(String),
    #[error("syntax error after lowering: {0}")]
    Syn(#[from] syn::Error),
}

impl From<proc_macro2::LexError> for Error {
    fn from(err: proc_macro2::LexError) -> Self {
        Error::Lex(err.to_string())
    }
}

#[derive(Debug, Default)]
pub struct LowerOutput {
    /// The lowered, prettyprinted Rust source as handed to the downstream
    /// compiler: Trust markers and `#[allow(trust::…)]` items stripped
    /// (stock rustc rejects the `trust::` tool path — E0710).
    pub source: String,
    /// The lowered source with `#[allow(trust::…)]` attributes still in
    /// place. Parse THIS for linting — the allow map is built from these
    /// attributes, so linting `source` would un-suppress everything the
    /// user explicitly allowed (RT-89).
    pub lint_source: String,
    /// Diagnostics emitted during lowering (e.g. unknown callee for named args).
    pub diagnostics: Vec<Diagnostic>,
    /// `true` if the original source had `#![strict]` at the crate root.
    /// Tracked here because the attribute is stripped during lowering
    /// (rustc doesn't recognise it) but downstream lints still need it.
    pub strict_mode: bool,
}

/// Lower a Trust source file to plain Rust.
pub fn lower(source: &str) -> Result<LowerOutput, Error> {
    lower_with_extra_callees(source, &[])
}

/// Lower a Trust source file with a supplementary list of
/// `(name, params)` entries seeded into the callee registry. Used by
/// `trust-rustc` to provide a crate-wide view of every `pub fn` /
/// module-level `fn` defined elsewhere in the same `src/` tree — RT-40.
///
/// Local fns in this file still win on name conflict; cross-file extras
/// outrank the bundled `trust-std` index.
pub fn lower_with_extra_callees(
    source: &str,
    extras: &[(String, Vec<String>)],
) -> Result<LowerOutput, Error> {
    lower_with_extra_callees_forced(source, extras, false)
}

/// As [`lower_with_extra_callees`], but `force_strict` activates strict mode
/// even when the source carries no `#![strict]` marker. Used by
/// the `trust-rustc` wrapper for project-level opt-in
/// (`[package.metadata.trust] strict = true`), where strictness is declared
/// once in the manifest rather than per file (RT-81).
pub fn lower_with_extra_callees_forced(
    source: &str,
    extras: &[(String, Vec<String>)],
    force_strict: bool,
) -> Result<LowerOutput, Error> {
    let tokens: TokenStream = source.parse()?;

    let strict_mode = force_strict || detect_strict_mode(&tokens);

    // Build the callee registry from the local signatures plus any
    // crate-wide extras the caller supplied.
    let registry = named_args::CalleeRegistry::collect_with_extras(&tokens, extras);

    let mut diagnostics = Vec::new();
    let tokens = named_args::rewrite(tokens, &registry, &mut diagnostics, strict_mode);
    let tokens = pipe::rewrite(tokens, &mut diagnostics);
    let tokens = preprocess::strip_strict_attrs(tokens);

    // Two views of the lowered output (RT-89): the linter must see the
    // `#[allow(trust::…)]` attributes (the allow map is built from them),
    // while the downstream compiler must NOT (stock rustc rejects the
    // `trust::` tool path — E0710).
    let lint_file: syn::File = syn::parse2(tokens.clone())?;
    let lint_source = prettyplease::unparse(&lint_file);

    let rustc_tokens = preprocess::strip_trust_allow_items(tokens);
    let file: syn::File = syn::parse2(rustc_tokens)?;
    let source = prettyplease::unparse(&file);

    Ok(LowerOutput {
        source,
        lint_source,
        diagnostics,
        strict_mode,
    })
}

/// RT-71: insert named arguments at positional call sites — the source-level
/// inverse of lowering. Turns vanilla Rust into strict named-arg form by
/// splicing `name: ` before each positional argument of a call to a callee
/// Trust can see (local `fn`s plus any `extras` dependency indices). Only the
/// names are inserted; every other byte of the original source — comments,
/// spacing, layout — is preserved. Used by `trust fix`.
///
/// Names are inserted exactly where R0042 would require them, so the result
/// passes `trust check` (modulo other lints) and round-trips back to the
/// original positional form under the lowering pass.
pub fn promote_named_args(source: &str, extras: &[(String, Vec<String>)]) -> Result<String, Error> {
    let tokens: TokenStream = source.parse()?;
    let registry = named_args::CalleeRegistry::collect_with_extras(&tokens, extras);
    let mut insertions = named_args::collect_name_insertions(&tokens, &registry);
    // Apply right-to-left so each insertion's byte offset stays valid as we go.
    insertions.sort_by_key(|(offset, _)| std::cmp::Reverse(*offset));
    let mut out = source.to_string();
    for (offset, text) in insertions {
        if offset <= out.len() && out.is_char_boundary(offset) {
            out.insert_str(offset, &text);
        }
    }
    Ok(out)
}

/// Scan a token stream for the per-file Trust activation marker: the
/// `#![strict]` inner attribute. Stock `rustc` rejects this attribute, so a
/// file carrying it only compiles through the Trust toolchain (`trust
/// check`/`build` for single files, the `trust-rustc` wrapper — i.e. `cargo
/// trust` — for cargo crates, which strips it during lowering).
///
/// Whole crates can skip the marker entirely and opt in at the project level
/// via `[package.metadata.trust] strict = true` — see `cargo-trust`, which
/// threads that through `lower_with_extra_callees_forced`.
///
/// Detection runs at the token level so it works on the *original* Trust
/// source before any pass strips the marker, and so it doesn't depend on syn
/// parsing (which would fail for source containing pipe / effect / named-arg
/// extensions).
fn detect_strict_mode(tokens: &TokenStream) -> bool {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    detect_strict_inner_attr(&trees)
}

/// Cheap source-level strict-mode detection. Used by the `trust-rustc`
/// cargo wrapper to decide whether a given input file needs lowering before
/// being handed to the real `rustc`. Returns `false` if the source can't be
/// tokenised at all (caller should fall through to passing the file
/// unchanged so the real rustc can produce a familiar diagnostic).
pub fn is_strict_source(source: &str) -> bool {
    let Ok(tokens) = source.parse::<TokenStream>() else {
        return false;
    };
    detect_strict_mode(&tokens)
}

fn detect_strict_inner_attr(trees: &[proc_macro2::TokenTree]) -> bool {
    let mut i = 0;
    while i < trees.len() {
        let proc_macro2::TokenTree::Punct(hash) = &trees[i] else {
            i += 1;
            continue;
        };
        if hash.as_char() != '#' {
            i += 1;
            continue;
        }
        let Some(proc_macro2::TokenTree::Punct(bang)) = trees.get(i + 1) else {
            i += 1;
            continue;
        };
        if bang.as_char() != '!' {
            i += 1;
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(g)) = trees.get(i + 2) else {
            i += 1;
            continue;
        };
        if g.delimiter() != proc_macro2::Delimiter::Bracket {
            i += 1;
            continue;
        }
        let inner: Vec<proc_macro2::TokenTree> = g.stream().into_iter().collect();
        if inner.len() == 1 {
            if let proc_macro2::TokenTree::Ident(id) = &inner[0] {
                if *id == "strict" {
                    return true;
                }
            }
        }
        i += 3;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_lowers_clean() {
        let out = lower("").expect("empty should lower");
        assert_eq!(out.source, "");
        assert!(out.diagnostics.is_empty());
    }

    #[test]
    fn vanilla_rust_passes_through() {
        let out = lower("fn main() { println!(\"hi\"); }").expect("hello should lower");
        assert!(out.source.contains("fn main"));
        assert!(out.diagnostics.is_empty());
    }

    // RT-89: trust:: allow items are linter-only; rustc rejects the tool
    // path (E0710), so lowering must strip them — wholly, or item-wise when
    // mixed with real rustc lints.
    #[test]
    fn trust_allow_items_are_stripped_from_lowered_output() {
        let src = "#![strict]\n#[allow(trust::R0017, reason = \"fixture\")]\npub fn f(a: u32, b: u32) {}\nfn main() {}";
        let out = lower(src).expect("should lower");
        assert!(
            !out.source.contains("trust ::") && !out.source.contains("trust::"),
            "trust:: must not survive lowering: {}",
            out.source
        );
        assert!(
            !out.source.contains("allow"),
            "all-trust allow drops entirely: {}",
            out.source
        );
        // ...but the lint-facing view keeps the attribute, so the allow map
        // can suppress the rule the user silenced.
        assert!(
            out.lint_source.contains("allow(trust::R0017"),
            "lint view keeps the trust allow: {}",
            out.lint_source
        );

        let mixed = "#![strict]\n#[allow(dead_code, trust::R0017, reason = \"x\")]\npub fn g(a: u32, b: u32) {}\nfn main() {}";
        let out = lower(mixed).expect("should lower");
        assert!(
            out.source.contains("allow(dead_code)"),
            "non-trust lint kept: {}",
            out.source
        );
        assert!(
            !out.source.contains("trust"),
            "trust item dropped: {}",
            out.source
        );

        let plain = "#![strict]\n#[allow(dead_code)]\nfn h() {}\nfn main() {}";
        let out = lower(plain).expect("should lower");
        assert!(
            out.source.contains("allow(dead_code)"),
            "plain allow untouched: {}",
            out.source
        );
    }

    #[test]
    fn detect_strict_accepts_inner_attr() {
        let src = "#![strict]\nfn main() {}";
        let out = lower(src).expect("should parse");
        assert!(out.strict_mode);
        // The marker must be stripped from the lowered output — stock rustc
        // rejects it.
        assert!(!out.source.contains("strict"));
    }

    // RT-82: the strict!{} macro marker was removed along with the
    // trust-attrs crate. A leftover invocation must NOT activate strict
    // mode — activation is #![strict] (per file) or
    // [package.metadata.trust] strict = true (per project) only.
    #[test]
    fn detect_strict_ignores_legacy_macro_marker() {
        for src in [
            "strict!{}\nfn main() {}",
            "trust_attrs::strict!{}\nfn main() {}",
            "trust::strict!{}\nfn main() {}",
        ] {
            let out = lower(src).expect("should parse");
            assert!(!out.strict_mode, "macro marker must be inert: {src}");
        }
    }

    #[test]
    fn forced_strict_activates_without_marker() {
        let src = "fn main() {}";
        let out = lower_with_extra_callees_forced(src, &[], true).expect("should parse");
        assert!(out.strict_mode);
    }
}
