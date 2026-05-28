//! Phase 2: pipe operator `|>`.
//!
//! Desugars `expr |> path(args)` to `path(expr, args)`. The leading `expr`
//! is any contiguous run of tokens between the previous statement
//! boundary (start-of-group, `;`, `{`, `}`, `=`, `=>`, or `,`) and the
//! `|>` punct pair, with parenthesised / bracketed / braced subexpressions
//! handled as atomic units.

use proc_macro2::{Delimiter, Group, Punct, Spacing, Span, TokenStream, TokenTree};
use rustricted_diag::Diagnostic;

use crate::preprocess::{from_vec, map_groups, to_vec};

pub fn rewrite(tokens: TokenStream, diagnostics: &mut Vec<Diagnostic>) -> TokenStream {
    let mut f = |_delim: Delimiter, inner: TokenStream| rewrite_stream(inner, diagnostics);
    let tokens = map_groups(tokens, &mut f);
    rewrite_stream(tokens, diagnostics)
}

/// Rewrite all top-level `|>` occurrences in a single stream (groups are
/// assumed to have already been recursively rewritten by the caller).
///
/// We loop until no `|>` remains so that chained pipes lower left-to-right:
/// after `x |> f() |> g()` becomes `f(x) |> g()`, the next pass rewrites it
/// to `g(f(x))`.
fn rewrite_stream(tokens: TokenStream, diagnostics: &mut Vec<Diagnostic>) -> TokenStream {
    let mut trees = to_vec(tokens);
    while let Some(pipe_idx) = find_pipe(&trees) {
        if !try_rewrite_at(&mut trees, pipe_idx, diagnostics) {
            // RHS didn't parse as `path(args)`; diagnostic already emitted.
            // Drop the `|>` so downstream syn parsing isn't poisoned by it.
            trees.remove(pipe_idx);
            trees.remove(pipe_idx);
        }
    }
    from_vec(trees)
}

/// Locate the first `|>` pair. We require `Spacing::Joint` on the `|`
/// because a closure header like `|x| x + 1` is also two `|` puncts but
/// each one is `Spacing::Alone`.
fn find_pipe(trees: &[TokenTree]) -> Option<usize> {
    for i in 0..trees.len().saturating_sub(1) {
        if let (TokenTree::Punct(p1), TokenTree::Punct(p2)) = (&trees[i], &trees[i + 1]) {
            if p1.as_char() == '|' && p1.spacing() == Spacing::Joint && p2.as_char() == '>' {
                return Some(i);
            }
        }
    }
    None
}

/// Try to rewrite a single `|>` at `pipe_idx` in place. Returns `false` if
/// the RHS couldn't be parsed as `path(args)` (a diagnostic is emitted).
fn try_rewrite_at(
    trees: &mut Vec<TokenTree>,
    pipe_idx: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let lhs_start = lhs_start(trees, pipe_idx);
    let lhs_len = pipe_idx - lhs_start;

    // RHS: optional `Ident (:: Ident)*` followed by a parenthesised Group.
    let rhs_start = pipe_idx + 2;
    let Some((path_end, args_group)) = parse_rhs(trees, rhs_start) else {
        let span = trees[pipe_idx].span();
        diagnostics.push(
            Diagnostic::error(
                crate::Rule::PipeRhsNotPathCall.code(),
                crate::Rule::PipeRhsNotPathCall.message_shape(),
                span_to_range(span),
            )
            .with_help("write `expr |> path(args)`"),
        );
        return false;
    };

    let lhs: Vec<TokenTree> = trees.drain(lhs_start..pipe_idx).collect();
    // After draining LHS, indices shift: `|>` is now at `lhs_start`.
    let pipe_now = lhs_start;
    // Remove `|>`.
    trees.drain(pipe_now..pipe_now + 2);
    // Path tokens now start at `pipe_now`. Length is `path_end - rhs_start`.
    let path_len = path_end - rhs_start;
    let path: Vec<TokenTree> = trees.drain(pipe_now..pipe_now + path_len).collect();
    // The args group is now at `pipe_now`. Remove it.
    let TokenTree::Group(args_group_owned) = trees.remove(pipe_now) else {
        // Should not happen — parse_rhs verified this.
        return false;
    };
    let _ = args_group; // silence unused warning; we use the drained owned copy.

    // Build new args: `(lhs, original_args...)`.
    let original_args = args_group_owned.stream();
    let mut new_args = TokenStream::new();
    new_args.extend(lhs);
    let has_existing_args = !original_args.is_empty();
    if has_existing_args {
        new_args.extend(std::iter::once(TokenTree::Punct(comma(
            args_group_owned.span(),
        ))));
        new_args.extend(original_args);
    }
    let mut new_group = Group::new(Delimiter::Parenthesis, new_args);
    new_group.set_span(args_group_owned.span());

    // Splice `path(new_args)` back in at `pipe_now`.
    let mut replacement: Vec<TokenTree> = Vec::with_capacity(path.len() + 1);
    replacement.extend(path);
    replacement.push(TokenTree::Group(new_group));

    // `trees.splice` inserts at the current position.
    trees.splice(pipe_now..pipe_now, replacement);

    // LHS length not strictly needed past here; suppress unused.
    let _ = lhs_len;
    true
}

/// Walk left from `pipe_idx` to find the first statement-boundary token.
/// The LHS is everything strictly between that boundary and `pipe_idx`.
/// Boundaries: `;`, `{`, `}`, `=`, `=>`, `,`, or start-of-stream.
/// Groups (parens, brackets, braces) are atomic — they never split the LHS.
fn lhs_start(trees: &[TokenTree], pipe_idx: usize) -> usize {
    let mut i = pipe_idx;
    while i > 0 {
        let prev = &trees[i - 1];
        if is_boundary(trees, i - 1) {
            break;
        }
        // `=>` is two puncts: `=` (Joint) then `>` (Alone). We've already
        // treated the trailing `>` as a boundary via `is_boundary`, so the
        // `=` would also trip the `=` arm. That's fine — both anchor here.
        let _ = prev;
        i -= 1;
    }
    i
}

fn is_boundary(trees: &[TokenTree], idx: usize) -> bool {
    match &trees[idx] {
        TokenTree::Punct(p) => {
            let c = p.as_char();
            if c == ';' || c == ',' {
                return true;
            }
            if c == '=' {
                // `==`, `=>`, `=` all anchor (they end any preceding expr).
                return true;
            }
            if c == '>' && idx > 0 {
                // `=>` arrow: previous token was `=` joint.
                if let TokenTree::Punct(prev) = &trees[idx - 1] {
                    if prev.as_char() == '=' && prev.spacing() == Spacing::Joint {
                        return true;
                    }
                }
            }
            false
        }
        TokenTree::Group(g) => matches!(g.delimiter(), Delimiter::Brace),
        _ => false,
    }
}

/// Try to parse `Ident (:: Ident)* (...)` starting at `start`. Returns
/// `(index of the paren group, the group)` on success.
fn parse_rhs(trees: &[TokenTree], start: usize) -> Option<(usize, Group)> {
    let mut i = start;
    if i >= trees.len() {
        return None;
    }
    // First ident.
    if !matches!(trees.get(i), Some(TokenTree::Ident(_))) {
        return None;
    }
    i += 1;
    // Zero or more `:: Ident` segments. `::` is two joined `:` puncts.
    loop {
        let p1 = trees.get(i);
        let p2 = trees.get(i + 1);
        let ident = trees.get(i + 2);
        let is_colon_colon = matches!(
            (p1, p2),
            (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
                if a.as_char() == ':' && a.spacing() == Spacing::Joint && b.as_char() == ':'
        );
        if is_colon_colon && matches!(ident, Some(TokenTree::Ident(_))) {
            i += 3;
        } else {
            break;
        }
    }
    // Now expect a parenthesised group.
    match trees.get(i) {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
            Some((i, g.clone()))
        }
        _ => None,
    }
}

fn comma(span: Span) -> Punct {
    let mut p = Punct::new(',', Spacing::Alone);
    p.set_span(span);
    p
}

/// Convert a `proc_macro2::Span` to a byte range into the original source.
/// Requires the `span-locations` feature on `proc-macro2` (enabled in this
/// crate's Cargo.toml) — without it, `byte_range()` returns `0..0` for
/// every span, which collapses every diagnostic to line 1 col 1 (RT-42).
fn span_to_range(span: Span) -> std::ops::Range<usize> {
    span.byte_range()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(src: &str) -> (String, Vec<Diagnostic>) {
        let tokens: TokenStream = src.parse().unwrap();
        let mut diags = Vec::new();
        let out = rewrite(tokens, &mut diags);
        (out.to_string(), diags)
    }

    #[test]
    fn pipe_simple() {
        let (out, diags) = lower("x |> f()");
        assert!(out.contains("f (x)") || out.contains("f(x)"), "got: {out}");
        assert!(diags.is_empty());
    }

    #[test]
    fn pipe_with_args() {
        let (out, _) = lower("x |> f(y)");
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("f(x,y)"), "got: {out}");
    }

    #[test]
    fn pipe_chain() {
        let (out, _) = lower("x |> f() |> g()");
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("g(f(x))"), "got: {out}");
    }

    #[test]
    fn pipe_with_path() {
        let (out, _) = lower("x |> std::iter::once()");
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("std::iter::once(x)"), "got: {out}");
    }

    #[test]
    fn pipe_after_semi() {
        let (out, _) = lower("let z = a; b |> f()");
        let collapsed = out.replace(' ', "");
        // `a` must be untouched on the LHS of `;`, and `b` is the pipe LHS.
        assert!(collapsed.contains("letz=a;"), "got: {out}");
        assert!(collapsed.contains("f(b)"), "got: {out}");
    }

    #[test]
    fn pipe_inside_args() {
        let (out, _) = lower("outer(inner |> f())");
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("outer(f(inner))"), "got: {out}");
    }

    #[test]
    fn pipe_paren_lhs() {
        let (out, _) = lower("(a + b) |> double()");
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("double((a+b))"), "got: {out}");
    }

    #[test]
    fn no_pipe_passthrough() {
        let src = "fn main() { let x = 1 + 2; }";
        let (out, diags) = lower(src);
        // Token rendering normalises whitespace, but content survives.
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("letx=1+2"), "got: {out}");
        assert!(diags.is_empty());
    }

    #[test]
    fn bare_pipe_in_closure_unaffected() {
        let src = "let c = |x| x + 1;";
        let (out, diags) = lower(src);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        let collapsed = out.replace(' ', "");
        // `|x|` survives.
        assert!(collapsed.contains("|x|"), "got: {out}");
    }

    #[test]
    fn pipe_inside_let() {
        let (out, _) = lower("let z = a |> f();");
        let collapsed = out.replace(' ', "");
        assert!(collapsed.contains("letz=f(a);"), "got: {out}");
    }

    #[test]
    fn pipe_bad_rhs_emits_diagnostic() {
        let (_out, diags) = lower("x |> 5");
        assert!(diags.iter().any(|d| d.rule == "R2001"));
    }
}
