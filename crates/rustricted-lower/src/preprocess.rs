//! Shared token-stream walking utilities for the lowering passes.
//!
//! All passes follow the same shape: recursively walk a `TokenStream`,
//! recurse into `Group`s (so nested calls and blocks are visited), and
//! rewrite specific token patterns in place. The helpers here keep that
//! boilerplate out of each pass.

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};

/// Recursively map every `Group` in `tokens` through `f`, which receives
/// the inner stream and returns a (possibly rewritten) inner stream.
///
/// The outer walk preserves the delimiter and span of each group.
pub fn map_groups<F>(tokens: TokenStream, f: &mut F) -> TokenStream
where
    F: FnMut(Delimiter, TokenStream) -> TokenStream,
{
    let mut out = TokenStream::new();
    for tt in tokens {
        match tt {
            TokenTree::Group(g) => {
                let delim = g.delimiter();
                let inner = map_groups(g.stream(), f);
                let inner = f(delim, inner);
                let mut new_group = Group::new(delim, inner);
                new_group.set_span(g.span());
                out.extend(std::iter::once(TokenTree::Group(new_group)));
            }
            other => out.extend(std::iter::once(other)),
        }
    }
    out
}

/// Convert a `TokenStream` to a `Vec<TokenTree>` for index-based scanning.
pub fn to_vec(tokens: TokenStream) -> Vec<TokenTree> {
    tokens.into_iter().collect()
}

/// Collect a `Vec<TokenTree>` back into a `TokenStream`.
pub fn from_vec(trees: Vec<TokenTree>) -> TokenStream {
    trees.into_iter().collect()
}

/// Strip any attribute whose first path segment is `strict` — both inner
/// (`#![strict]`, `#![strict::macros_ok]`) and outer (`#[strict::macros_ok]`)
/// variants. These attributes are recognised by Rustricted lints but unknown
/// to `rustc`, so they must be removed before the lowered source goes to the
/// downstream compiler.
pub fn strip_strict_attrs(tokens: TokenStream) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(trees.len());
    let mut i = 0;
    while i < trees.len() {
        if let Some(consumed) = try_strip_at(&trees, i) {
            i += consumed;
            continue;
        }
        match &trees[i] {
            TokenTree::Group(g) => {
                let mut new = Group::new(g.delimiter(), strip_strict_attrs(g.stream()));
                new.set_span(g.span());
                out.push(TokenTree::Group(new));
            }
            other => out.push(other.clone()),
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// If `trees[i..]` starts with a `#[strict...]` or `#![strict...]` attribute,
/// return how many tokens it consumed. Otherwise return `None`.
fn try_strip_at(trees: &[TokenTree], i: usize) -> Option<usize> {
    let TokenTree::Punct(hash) = trees.get(i)? else {
        return None;
    };
    if hash.as_char() != '#' {
        return None;
    }
    let mut j = i + 1;
    if let Some(TokenTree::Punct(bang)) = trees.get(j) {
        if bang.as_char() == '!' {
            j += 1;
        }
    }
    let TokenTree::Group(g) = trees.get(j)? else {
        return None;
    };
    if g.delimiter() != Delimiter::Bracket {
        return None;
    }
    let inner: Vec<TokenTree> = g.stream().into_iter().collect();
    let first = inner.first()?;
    let TokenTree::Ident(id) = first else {
        return None;
    };
    if *id == "strict" {
        Some(j + 1 - i)
    } else {
        None
    }
}
