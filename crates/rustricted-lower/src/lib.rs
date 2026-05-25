//! Lowering passes: Rustricted source → plain Rust source.
//!
//! Each pass is a token-level rewrite over `proc_macro2::TokenStream`:
//! - [`pipe`] desugars `e |> f(args)` to `f(e, args)`.
//! - [`named_args`] rewrites `f(name: value, ...)` to positional based on
//!   a per-crate callee registry built from local function signatures.
//! - [`effects`] strips `effect E + E` annotations from function signatures
//!   (the [`rustricted_effects`] crate enforces them before stripping).
//!
//! The driver wires these together via [`lower`].

use proc_macro2::TokenStream;
use rustricted_diag::Diagnostic;
use rustricted_effects::EffectTable;
use thiserror::Error;

pub mod named_args;
pub mod pipe;
pub mod preprocess;
pub mod rule;

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
    /// Effect declarations parsed out of `fn ... effect E + E` annotations.
    /// Empty for sources that don't use the `effect` keyword.
    pub effect_table: EffectTable,
}

/// Lower a Rustricted source file to plain Rust.
pub fn lower(source: &str) -> Result<LowerOutput, Error> {
    let tokens: TokenStream = source.parse()?;

    let strict_mode = detect_strict_mode(&tokens);

    // Strip `effect` clauses first — they live in fn signatures, so removing
    // them simplifies every later pass and guarantees syn can parse the result.
    let (tokens, effect_table) = rustricted_effects::strip_effect_annotations(tokens);

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
        effect_table,
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

/// Match `... :: strict ! GROUP` or `strict ! GROUP` at top level. Path
/// prefix is accepted but not inspected — any path that ends in `strict`
/// counts, so both `strict!{}` and `rustricted_attrs::strict!{}` work.
fn detect_strict_macro_call(trees: &[proc_macro2::TokenTree]) -> bool {
    for i in 0..trees.len() {
        let proc_macro2::TokenTree::Ident(id) = &trees[i] else {
            continue;
        };
        if *id != "strict" {
            continue;
        }
        let Some(proc_macro2::TokenTree::Punct(bang)) = trees.get(i + 1) else {
            continue;
        };
        if bang.as_char() != '!' {
            continue;
        }
        if matches!(trees.get(i + 2), Some(proc_macro2::TokenTree::Group(_))) {
            return true;
        }
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
}
