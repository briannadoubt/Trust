//! RT-69: lightweight `requires!(…)` precondition contracts.
//!
//! A strict-mode file may open a function body with precondition
//! statements:
//!
//! ```text
//! fn withdraw(balance: u64, amount: u64) -> u64 {
//!     requires!(amount <= balance);
//!     balance - amount
//! }
//! ```
//!
//! Lowering rewrites each bare `requires!(COND)` invocation into
//!
//! ```text
//! debug_assert!(COND, "requires violated: COND");
//! ```
//!
//! so the stated intent becomes a checkable assertion in debug builds and
//! disappears in release — no solver, no dataflow, no new type machinery.
//! That scope boundary is deliberate (the ticket's own warning): the moment
//! a contract feature wants more than a lowering, it has left this design.
//!
//! `ensures!` (postconditions) is intentionally NOT implemented: it requires
//! wrapping every return path, which is a transformation with real semantic
//! surface (early returns, `?`, tail expressions). Cut per the ticket.
//!
//! The rewrite is strict-mode-only and only touches *bare* `requires!`
//! invocations — a path-qualified `some_crate::requires!(…)` belongs to that
//! crate and passes through untouched, as does everything in non-strict
//! files (where `requires!` may be a user macro).

use proc_macro2::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenStream, TokenTree};

/// Rewrite bare `requires!(COND)` invocations to
/// `debug_assert!(COND, "requires violated: …")`. No-op unless
/// `strict_mode`.
pub fn rewrite(tokens: TokenStream, strict_mode: bool) -> TokenStream {
    if !strict_mode {
        return tokens;
    }
    rewrite_inner(tokens)
}

fn rewrite_inner(tokens: TokenStream) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(trees.len());
    let mut i = 0;
    while let Some(tree) = trees.get(i) {
        // Bare `requires ! ( ... )` — the preceding token must not be `:`
        // (which would make it path-qualified) or `.` (a method position).
        if let TokenTree::Ident(id) = tree {
            if *id == "requires" && !preceded_by_path_or_dot(&out) {
                if let (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(args))) =
                    (trees.get(i + 1), trees.get(i + 2))
                {
                    if bang.as_char() == '!' && args.delimiter() == Delimiter::Parenthesis {
                        out.extend(build_debug_assert(args, id.span()));
                        i += 3;
                        continue;
                    }
                }
            }
        }
        match tree {
            TokenTree::Group(g) => {
                let mut new = Group::new(g.delimiter(), rewrite_inner(g.stream()));
                new.set_span(g.span());
                out.push(TokenTree::Group(new));
            }
            other => out.push(other.clone()),
        }
        i += 1;
    }
    out.into_iter().collect()
}

fn preceded_by_path_or_dot(out: &[TokenTree]) -> bool {
    matches!(
        out.last(),
        Some(TokenTree::Punct(p)) if p.as_char() == ':' || p.as_char() == '.'
    )
}

/// `debug_assert ! ( COND , "requires violated: COND" )`, spans pointing at
/// the original `requires` ident so diagnostics land on the contract.
fn build_debug_assert(args: &Group, span: Span) -> Vec<TokenTree> {
    let cond = args.stream();
    let cond_text = cond.to_string();
    let mut inner: Vec<TokenTree> = cond.into_iter().collect();
    inner.push(TokenTree::Punct(Punct::new(',', Spacing::Alone)));
    inner.push(TokenTree::Literal(proc_macro2::Literal::string(&format!(
        "requires violated: {cond_text}"
    ))));

    let mut group = Group::new(Delimiter::Parenthesis, inner.into_iter().collect());
    group.set_span(args.span());
    vec![
        TokenTree::Ident(Ident::new("debug_assert", span)),
        TokenTree::Punct(Punct::new('!', Spacing::Alone)),
        TokenTree::Group(group),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(src: &str, strict: bool) -> String {
        rewrite(src.parse().expect("tokenizes"), strict).to_string()
    }

    #[test]
    fn requires_lowers_to_debug_assert_in_strict() {
        let out = lower("fn f(x: u32) { requires!(x > 0); }", true);
        assert!(out.contains("debug_assert"), "{out}");
        assert!(out.contains("requires violated: x > 0"), "{out}");
        assert!(!out.contains("requires !"), "{out}");
    }

    #[test]
    fn requires_untouched_outside_strict() {
        let out = lower("fn f(x: u32) { requires!(x > 0); }", false);
        assert!(
            out.contains("requires !"),
            "non-strict must pass through: {out}"
        );
        assert!(!out.contains("debug_assert"), "{out}");
    }

    #[test]
    fn qualified_and_method_forms_pass_through() {
        let q = lower("fn f() { contracts::requires!(true); }", true);
        assert!(q.contains("requires !"), "path-qualified is not ours: {q}");
        let m = lower("fn f(c: C) { c.requires!(true); }", true);
        assert!(
            m.contains("requires !"),
            "method-ish position is not ours: {m}"
        );
    }
}
