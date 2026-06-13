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
/// variants. These attributes are recognised by Trust lints but unknown
/// to `rustc`, so they must be removed before the lowered source goes to the
/// downstream compiler.
pub fn strip_strict_attrs(tokens: TokenStream) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(trees.len());
    let mut i = 0;
    while let Some(tree) = trees.get(i) {
        if let Some(consumed) = try_strip_at(&trees, i) {
            i += consumed;
            continue;
        }
        match tree {
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

/// Strip `trust::Rxxxx` items out of `#[allow(...)]` / `#[expect(...)]`
/// attributes (RT-89). The `#[allow(trust::R0017, reason = "…")]` escape
/// hatch is consumed by the Trust linter, but stock rustc rejects the
/// `trust::` tool path (E0710 unknown tool name), so it must not survive
/// lowering. Non-trust lint paths in the same attribute are kept; if nothing
/// but trust items (and their `reason`) remain, the whole attribute is
/// dropped.
pub fn strip_trust_allow_items(tokens: TokenStream) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(trees.len());
    let mut i = 0;
    while let Some(tree) = trees.get(i) {
        if let Some((replacement, consumed)) = try_rewrite_allow_at(&trees, i) {
            out.extend(replacement);
            i += consumed;
            continue;
        }
        match tree {
            TokenTree::Group(g) => {
                let mut new = Group::new(g.delimiter(), strip_trust_allow_items(g.stream()));
                new.set_span(g.span());
                out.push(TokenTree::Group(new));
            }
            other => out.push(other.clone()),
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// If `trees[i..]` starts with an `#[allow(...)]`-shaped attribute containing
/// a `trust::` item, return the rewritten attribute tokens (possibly empty)
/// and how many input tokens were consumed.
fn try_rewrite_allow_at(trees: &[TokenTree], i: usize) -> Option<(Vec<TokenTree>, usize)> {
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
    let TokenTree::Group(bracket) = trees.get(j)? else {
        return None;
    };
    if bracket.delimiter() != Delimiter::Bracket {
        return None;
    }
    let inner: Vec<TokenTree> = bracket.stream().into_iter().collect();
    let [TokenTree::Ident(name), TokenTree::Group(list)] = inner.as_slice() else {
        return None;
    };
    if *name != "allow" && *name != "expect" {
        return None;
    }
    if list.delimiter() != Delimiter::Parenthesis {
        return None;
    }

    // Split the list on top-level commas; keep items that are not
    // `trust::...` and not `reason = ...`.
    let items = split_top_commas(list.stream());
    let has_trust = items.iter().any(|seg| starts_with_trust_path(seg));
    if !has_trust {
        return None; // plain rustc/clippy allow — leave untouched
    }
    let kept: Vec<Vec<TokenTree>> = items
        .into_iter()
        .filter(|seg| !starts_with_trust_path(seg) && !is_reason_item(seg))
        .collect();

    if kept.is_empty() {
        // Nothing rustc-meaningful left: drop the whole attribute.
        return Some((Vec::new(), j + 1 - i));
    }

    // Rebuild `#[allow(<kept...>)]` preserving the original shape.
    let mut list_tokens: Vec<TokenTree> = Vec::new();
    for (idx, seg) in kept.into_iter().enumerate() {
        if idx > 0 {
            list_tokens.push(TokenTree::Punct(proc_macro2::Punct::new(
                ',',
                proc_macro2::Spacing::Alone,
            )));
        }
        list_tokens.extend(seg);
    }
    let mut new_list = Group::new(Delimiter::Parenthesis, list_tokens.into_iter().collect());
    new_list.set_span(list.span());
    let mut new_bracket = Group::new(
        Delimiter::Bracket,
        vec![TokenTree::Ident(name.clone()), TokenTree::Group(new_list)]
            .into_iter()
            .collect(),
    );
    new_bracket.set_span(bracket.span());

    let mut replacement: Vec<TokenTree> = trees[i..j].to_vec(); // `#` and optional `!`
    replacement.push(TokenTree::Group(new_bracket));
    Some((replacement, j + 1 - i))
}

fn split_top_commas(tokens: TokenStream) -> Vec<Vec<TokenTree>> {
    let mut out: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for tt in tokens {
        if let TokenTree::Punct(p) = &tt {
            if p.as_char() == ',' {
                out.push(Vec::new());
                continue;
            }
        }
        out.last_mut().expect("non-empty").push(tt);
    }
    out.retain(|seg| !seg.is_empty());
    out
}

fn starts_with_trust_path(seg: &[TokenTree]) -> bool {
    matches!(seg.first(), Some(TokenTree::Ident(id)) if *id == "trust")
}

fn is_reason_item(seg: &[TokenTree]) -> bool {
    matches!(seg.first(), Some(TokenTree::Ident(id)) if *id == "reason")
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
