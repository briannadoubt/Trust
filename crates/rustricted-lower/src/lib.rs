//! Lowering passes: Rustricted source → plain Rust source.
//!
//! Each pass is a token-level rewrite over `proc_macro2::TokenStream`:
//! - [`pipe`] desugars `e |> f(args)` to `f(e, args)`.
//! - [`named_args`] rewrites `f(name: value, ...)` to positional based on
//!   a per-crate callee registry built from local function signatures.
//!
//! The driver wires these together via [`lower`].

use proc_macro2::TokenStream;
use rustricted_diag::Diagnostic;
use thiserror::Error;

pub mod named_args;
pub mod pipe;
pub mod preprocess;
pub mod rule;
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
    /// The lowered, prettyprinted Rust source.
    pub source: String,
    /// Diagnostics emitted during lowering (e.g. unknown callee for named args).
    pub diagnostics: Vec<Diagnostic>,
    /// `true` if the original source had `#![strict]` at the crate root.
    /// Tracked here because the attribute is stripped during lowering
    /// (rustc doesn't recognise it) but downstream lints still need it.
    pub strict_mode: bool,
}

/// Lower a Rustricted source file to plain Rust.
pub fn lower(source: &str) -> Result<LowerOutput, Error> {
    let tokens: TokenStream = source.parse()?;

    let strict_mode = detect_strict_mode(&tokens);

    // Build the local callee registry from the cleaned signatures so named-args
    // can rewrite calls against the declared parameter list.
    let registry = named_args::CalleeRegistry::collect(&tokens);

    let mut diagnostics = Vec::new();
    let tokens = named_args::rewrite(tokens, &registry, &mut diagnostics, strict_mode);
    let tokens = pipe::rewrite(tokens, &mut diagnostics);
    let tokens = preprocess::strip_strict_attrs(tokens);

    let file: syn::File = syn::parse2(tokens)?;
    let source = prettyplease::unparse(&file);
    Ok(LowerOutput {
        source,
        diagnostics,
        strict_mode,
    })
}

/// Scan a token stream for Rustricted activation. Two forms are recognised:
///
/// 1. `#![strict]` inner attribute — used by single-file inputs sent through
///    `rustricted check`. Stock `rustc` rejects this attribute, so it cannot
///    appear in a file that will also be built by `cargo build`.
///
/// 2. `strict!{}` (or `rustricted_attrs::strict!{}`) function-like macro
///    invocation at item position — the cargo-friendly activation. The
///    `rustricted-attrs` crate exports the macro as a no-op for `rustc`.
///
/// Detection runs at the token level so it works on the *original* Rustricted
/// source before any pass strips the marker, and so it doesn't depend on syn
/// parsing (which would fail for source containing pipe / effect / named-arg
/// extensions).
fn detect_strict_mode(tokens: &TokenStream) -> bool {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    detect_strict_inner_attr(&trees) || detect_strict_macro_call(&trees)
}

/// Cheap source-level strict-mode detection. Used by the `rustricted-rustc`
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

/// Match `strict ! GROUP` (bare) or `<allowed-prefix> :: strict ! GROUP`
/// at top level. The path prefix must be either absent or one of the
/// known activation crates (`rustricted_attrs` or `rustricted`) — a
/// permissive match (any prefix) would let an unrelated `wibble::strict!{}`
/// silently activate the dialect, which the FP audit flagged as a real
/// concern.
fn detect_strict_macro_call(trees: &[proc_macro2::TokenTree]) -> bool {
    use proc_macro2::TokenTree;

    const ALLOWED_PREFIXES: &[&str] = &["rustricted_attrs", "rustricted"];

    for i in 0..trees.len() {
        let TokenTree::Ident(id) = &trees[i] else {
            continue;
        };
        if *id != "strict" {
            continue;
        }
        let Some(TokenTree::Punct(bang)) = trees.get(i + 1) else {
            continue;
        };
        if bang.as_char() != '!' {
            continue;
        }
        if !matches!(trees.get(i + 2), Some(TokenTree::Group(_))) {
            continue;
        }

        // Path-prefix check. Two cases:
        //  - bare: token immediately before `strict` is NOT `:` → accept.
        //  - qualified: the preceding tokens are `IDENT :: strict` → the
        //    ident must be in ALLOWED_PREFIXES.
        let preceded_by_colon = matches!(
            trees.get(i.wrapping_sub(1)),
            Some(TokenTree::Punct(p)) if i > 0 && p.as_char() == ':'
        );
        if !preceded_by_colon {
            return true;
        }
        if i >= 3 {
            if let (
                Some(TokenTree::Ident(prefix)),
                Some(TokenTree::Punct(c1)),
                Some(TokenTree::Punct(c2)),
            ) = (trees.get(i - 3), trees.get(i - 2), trees.get(i - 1))
            {
                if c1.as_char() == ':'
                    && c2.as_char() == ':'
                    && ALLOWED_PREFIXES.iter().any(|p| prefix == p)
                {
                    return true;
                }
            }
        }
        // Otherwise: qualified by something we don't recognise — keep
        // scanning. (Don't return false; another valid invocation may
        // appear later in the file.)
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

    // Codex feedback: detect_strict_macro_call used to accept any
    // `<anything>::strict!{}` path. That meant an unrelated crate's
    // `wibble::strict!{}` could silently activate the dialect.
    #[test]
    fn detect_strict_rejects_unrecognised_path_prefix() {
        let src = "wibble::strict!{}\nfn main() { let x: Option<u32> = None; x.unwrap(); }";
        let out = lower(src).expect("should still parse");
        assert!(
            !out.strict_mode,
            "wibble::strict!{{}} must NOT activate strict mode"
        );
    }

    #[test]
    fn detect_strict_accepts_bare_macro() {
        let src = "strict!{}\nfn main() {}";
        let out = lower(src).expect("should parse");
        assert!(out.strict_mode);
    }

    #[test]
    fn detect_strict_accepts_rustricted_attrs_macro() {
        let src = "rustricted_attrs::strict!{}\nfn main() {}";
        let out = lower(src).expect("should parse");
        assert!(out.strict_mode);
    }

    #[test]
    fn detect_strict_accepts_rustricted_macro() {
        // Short-form (`rustricted::strict`) is also on the allowlist for
        // crates that re-export the macro under the umbrella crate.
        let src = "rustricted::strict!{}\nfn main() {}";
        let out = lower(src).expect("should parse");
        assert!(out.strict_mode);
    }
}
