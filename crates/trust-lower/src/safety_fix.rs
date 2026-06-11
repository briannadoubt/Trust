//! Mechanical safety fixes for `trust fix` (RT-106).
//!
//! The first external adopter asked for `trust fix` to do more than named-arg
//! insertion — specifically, the obvious mechanical safety rewrite: turn
//! `.unwrap()` / `.expect(…)` into `?` inside functions that already return
//! `Result`. This is a *best-effort* (`MaybeIncorrect`) transform: we can't
//! prove the receiver's error type converts into the function's, so the result
//! should be reviewed and recompiled. But it removes the bulk of the manual
//! churn an R0001 cleanup backlog (the reviewer's ~22 findings) would require.
//!
//! Scoping rules, chosen so the rewrite never silently changes semantics:
//! - Only inside a function whose declared return type's last path segment is
//!   `Result` (so `?` propagates to the right place).
//! - Never inside a closure, an `async {}` block, or a nested `fn` — there `?`
//!   targets a *different* return type than the enclosing function.
//! - Never inside `#[cfg(test)]` / `#[test]` items — tests are exempt, matching
//!   the lint.
//!
//! The transform is purely textual once targets are found (byte-range splice),
//! so all surrounding formatting and comments are preserved.

use std::ops::Range;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// Rewrite `.unwrap()` / `.expect(…)` to `?` inside `Result`-returning
/// functions. Returns the rewritten source. Errors only if `source` does not
/// parse as Rust.
pub fn fix_unwrap_to_question(source: &str) -> syn::Result<String> {
    let file: syn::File = syn::parse_str(source)?;
    let mut ranges: Vec<Range<usize>> = Vec::new();
    collect_items(&file.items, &mut ranges);

    // Apply highest-offset-first so earlier byte ranges stay valid as we splice.
    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
    let mut out = source.to_string();
    for r in &ranges {
        // Guard against degenerate 0..0 spans (would mean span-locations is
        // off) and any range that somehow falls outside the source.
        if r.start < r.end && r.end <= out.len() {
            out.replace_range(r.clone(), "?");
        }
    }
    Ok(out)
}

/// `true` if `output`'s last path segment is `Result` (covers `Result`,
/// `io::Result`, `anyhow::Result`, `crate::Result`, …). Type aliases that hide
/// the `Result` are intentionally not matched — better to skip than mis-fix.
fn returns_result(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };
    let syn::Type::Path(tp) = ty.as_ref() else {
        return false;
    };
    tp.path.segments.last().is_some_and(|s| s.ident == "Result")
}

/// `true` if any attribute is `#[test]` or a `#[cfg(...)]` whose predicate
/// mentions `test`.
fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if a.path().is_ident("cfg") {
            if let syn::Meta::List(list) = &a.meta {
                return list.tokens.to_string().contains("test");
            }
        }
        false
    })
}

/// Walk items, descending into impls/mods/traits, and collect fix ranges from
/// every `Result`-returning function body that isn't test-gated.
fn collect_items(items: &[syn::Item], out: &mut Vec<Range<usize>>) {
    for item in items {
        match item {
            syn::Item::Fn(f) if !is_cfg_test(&f.attrs) && returns_result(&f.sig.output) => {
                collect_in_block(&f.block, out);
            }
            syn::Item::Impl(im) if !is_cfg_test(&im.attrs) => {
                for ii in &im.items {
                    if let syn::ImplItem::Fn(m) = ii {
                        if !is_cfg_test(&m.attrs) && returns_result(&m.sig.output) {
                            collect_in_block(&m.block, out);
                        }
                    }
                }
            }
            syn::Item::Mod(m) if !is_cfg_test(&m.attrs) => {
                if let Some((_, items)) = &m.content {
                    collect_items(items, out);
                }
            }
            syn::Item::Trait(t) if !is_cfg_test(&t.attrs) => {
                for ti in &t.items {
                    if let syn::TraitItem::Fn(m) = ti {
                        if let Some(block) = &m.default {
                            if !is_cfg_test(&m.attrs) && returns_result(&m.sig.output) {
                                collect_in_block(block, out);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_in_block(block: &syn::Block, out: &mut Vec<Range<usize>>) {
    Collector { out }.visit_block(block);
}

/// Collects the byte ranges of `.unwrap()` / `.expect(…)` calls to replace with
/// `?`, but refuses to descend where `?` would target a different return type.
struct Collector<'a> {
    out: &'a mut Vec<Range<usize>>,
}

impl<'ast> Visit<'ast> for Collector<'_> {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        let is_unwrap = method == "unwrap" && call.args.is_empty();
        let is_expect = method == "expect" && call.args.len() == 1;
        if is_unwrap || is_expect {
            // Replace `.unwrap()` / `.expect(msg)` — from the `.` through the
            // closing `)` — with `?`, leaving the receiver intact.
            let start = call.dot_token.span().byte_range().start;
            let end = call.paren_token.span.close().byte_range().end;
            self.out.push(start..end);
            // Recurse into the receiver only (catch chained `a.unwrap().b()` →
            // the inner unwrap), but not the `expect` message expression.
            self.visit_expr(&call.receiver);
        } else {
            // Ordinary call: recurse normally so unwraps in the receiver AND in
            // the arguments are found.
            visit::visit_expr_method_call(self, call);
        }
    }

    // `?` inside these targets a different return type than the enclosing fn.
    fn visit_expr_closure(&mut self, _: &'ast syn::ExprClosure) {}
    fn visit_expr_async(&mut self, _: &'ast syn::ExprAsync) {}
    fn visit_item_fn(&mut self, _: &'ast syn::ItemFn) {}
    fn visit_item_impl(&mut self, _: &'ast syn::ItemImpl) {}
    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {}
}

#[cfg(test)]
mod tests {
    use super::fix_unwrap_to_question;

    fn fix(src: &str) -> String {
        fix_unwrap_to_question(src).expect("input parses")
    }

    #[test]
    fn unwrap_in_result_fn_becomes_question() {
        let src = "fn f() -> Result<u8, E> {\n    let x = g().unwrap();\n    Ok(x)\n}\n";
        let out = fix(src);
        assert!(out.contains("let x = g()?;"), "{out}");
        assert!(!out.contains("unwrap"));
    }

    #[test]
    fn expect_becomes_question() {
        let src = "fn f() -> Result<u8, E> {\n    let x = g().expect(\"why\");\n    Ok(x)\n}\n";
        let out = fix(src);
        assert!(out.contains("let x = g()?;"), "{out}");
    }

    #[test]
    fn non_result_fn_is_untouched() {
        let src = "fn f() -> u8 {\n    g().unwrap()\n}\n";
        assert_eq!(fix(src), src, "non-Result fn must be left alone");
    }

    #[test]
    fn closure_is_skipped() {
        // The fn returns Result, but the unwrap is inside a closure whose `?`
        // would target the closure, not f — so it must be left alone.
        let src = "fn f() -> Result<(), E> {\n    let c = || h().unwrap();\n    c();\n    Ok(())\n}\n";
        let out = fix(src);
        assert!(out.contains("|| h().unwrap()"), "closure unwrap kept: {out}");
    }

    #[test]
    fn chained_unwraps_both_fixed() {
        let src = "fn f() -> Result<u8, E> {\n    Ok(a().unwrap().b().unwrap())\n}\n";
        let out = fix(src);
        assert!(out.contains("a()?.b()?"), "{out}");
    }

    #[test]
    fn unwrap_in_argument_is_fixed() {
        let src = "fn f() -> Result<(), E> {\n    use_it(parse().unwrap());\n    Ok(())\n}\n";
        let out = fix(src);
        assert!(out.contains("use_it(parse()?)"), "{out}");
    }

    #[test]
    fn cfg_test_module_is_skipped() {
        let src = "#[cfg(test)]\nmod t {\n    fn f() -> Result<(), E> { g().unwrap(); Ok(()) }\n}\n";
        assert_eq!(fix(src), src, "cfg(test) items are exempt");
    }
}
