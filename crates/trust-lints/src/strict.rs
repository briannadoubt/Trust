//! Strict-mode activation + per-rule dispatch.
//!
//! Intentionally not `#![strict]`-marked: dogfooding (RT-31) surfaced
//! RT-41 (method calls match free-fn signatures by simple name). This file
//! has dozens of `visit::visit_X(this, node)` calls inside `Visit` impls
//! that the per-file registry mis-resolves to local `fn visit_X(&mut self,
//! node)` methods, producing arity-mismatch R0042 FPs. Fixing requires
//! either path-aware callee resolution or a per-callsite `#[allow]`
//! mechanism (neither exists yet).

use crate::Rule;
use proc_macro2::Span;
use std::ops::Range;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use trust_diag::Diagnostic;

/// Returns `true` if the file is in Trust strict mode: a `#![strict]` inner
/// attribute at the crate root. (The `strict!{}` macro marker was removed in
/// RT-82; project-level `[package.metadata.trust] strict = true` activation
/// never reaches this detector — `cargo trustc` threads it through as a
/// forced flag instead.)
pub fn detect_strict(file: &syn::File) -> bool {
    file.attrs.iter().any(|attr| attr.path().is_ident("strict"))
}

/// Run a single rule against the parsed file, appending diagnostics.
pub fn run_rule(rule: Rule, file: &syn::File, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    match rule {
        Rule::NoUnwrap => run_no_unwrap(file, diagnostics),
        Rule::EmptyExpect => run_empty_expect(file, diagnostics),
        Rule::NoAsCast => run_no_as_cast(file, diagnostics),
        Rule::NoGlobImport => run_no_glob_import(file, diagnostics),
        Rule::JustifyUnsafe => run_justify_unsafe(file, source, diagnostics),
        Rule::JustifyAllow => run_justify_allow(file, source, diagnostics),
        Rule::NoImplTraitReturn => run_no_impl_trait_return(file, diagnostics),
        Rule::NoUserMacros => run_no_user_macros(file, diagnostics),
        Rule::NoTodoMacro => run_no_todo_macro(file, diagnostics),
        Rule::NoPanic => run_no_panic(file, diagnostics),
        Rule::NoBoolParam => run_no_bool_param(file, diagnostics),
        Rule::NoBareIndex => run_no_bare_index(file, diagnostics),
        Rule::NoSameTypeParams => run_no_same_type_params(file, diagnostics),
        Rule::NoErrorContextDrop => run_no_error_context_drop(file, diagnostics),
        Rule::NoUncheckedLenArith => run_no_unchecked_len_arith(file, diagnostics),
        Rule::NoLockAcrossAwait => run_no_lock_across_await(file, diagnostics),
        Rule::NoCapacityAsLen => run_no_capacity_as_len(file, diagnostics),
        // R0015 / R0016 emission lives in `crate::allow::collect_allow_map`,
        // which the runner invokes before per-rule dispatch. The catalogue
        // entries stay here so SPEC.md and tooling can refer to the codes.
        Rule::AllowMissingReason | Rule::AllowUnknownCode => {}
        // R0042 emission lives in `trust_lower::named_args`, where the
        // pass can still see name-prefixed call args before they're stripped.
        // The catalogue entry stays here so SPEC.md and the docs can refer
        // to the rule by code.
        Rule::NoPositionalArgs => {}
    }
}

fn span_range(span: Span) -> Range<usize> {
    let r = span.byte_range();
    if r.start == 0 && r.end == 0 {
        0..0
    } else {
        r
    }
}

fn attrs_have_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let mut hit = false;
        // Best-effort: inspect the meta list for an ident `test`. Misses
        // `cfg(any(test, ...))` and similar — acceptable for the strict
        // preset, which encourages a single `#[cfg(test)]` per item.
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                hit = true;
            }
            Ok(())
        });
        hit
    })
}

struct NoUnwrapVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
}

impl<'a> NoUnwrapVisitor<'a> {
    fn with_test_scope<F: FnOnce(&mut Self)>(&mut self, is_test: bool, f: F) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }
}

impl<'ast, 'a> Visit<'ast> for NoUnwrapVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| visit::visit_item_fn(this, node));
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_impl(this, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| visit::visit_impl_item_fn(this, node));
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if self.in_test_depth == 0 && node.method == "unwrap" && node.args.is_empty() {
            let diag = Diagnostic::error(
                Rule::NoUnwrap.code(),
                "`.unwrap()` is banned in strict mode",
                span_range(node.method.span()),
            )
            .with_why(Rule::NoUnwrap.rationale().to_string())
            .with_help("use `?` or `.expect(\"reason\")` instead");
            self.diagnostics.push(diag);
        }
        visit::visit_expr_method_call(self, node);
    }
}

fn run_no_unwrap(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoUnwrapVisitor {
        diagnostics,
        in_test_depth: 0,
    };
    v.visit_file(file);
}

struct EmptyExpectVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast, 'a> Visit<'ast> for EmptyExpectVisitor<'a> {
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "expect" && node.args.len() == 1 {
            if let syn::Expr::Lit(lit) = &node.args[0] {
                if let syn::Lit::Str(s) = &lit.lit {
                    if s.value().is_empty() {
                        let diag = Diagnostic::error(
                            Rule::EmptyExpect.code(),
                            "`.expect(\"\")` defeats the point of `expect`",
                            span_range(lit.span()),
                        )
                        .with_why(Rule::EmptyExpect.rationale().to_string())
                        .with_help("use `.expect(\"explain why this can't fail\")`");
                        self.diagnostics.push(diag);
                    }
                }
            }
        }
        visit::visit_expr_method_call(self, node);
    }
}

fn run_empty_expect(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = EmptyExpectVisitor { diagnostics };
    v.visit_file(file);
}

struct NoAsCastVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast, 'a> Visit<'ast> for NoAsCastVisitor<'a> {
    fn visit_expr_cast(&mut self, node: &'ast syn::ExprCast) {
        let diag = Diagnostic::error(
            Rule::NoAsCast.code(),
            "`as` casts are banned in strict mode",
            span_range(node.as_token.span()),
        )
        .with_why(Rule::NoAsCast.rationale().to_string())
        .with_help(
            "use `TryFrom`/`try_into` for fallible casts or `From`/`into` for infallible ones",
        );
        self.diagnostics.push(diag);
        visit::visit_expr_cast(self, node);
    }
}

fn run_no_as_cast(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoAsCastVisitor { diagnostics };
    v.visit_file(file);
}

fn tree_has_glob(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Glob(_) => true,
        syn::UseTree::Group(g) => g.items.iter().any(tree_has_glob),
        syn::UseTree::Path(p) => tree_has_glob(&p.tree),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) => false,
    }
}

struct NoGlobImportVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
}

impl<'a> NoGlobImportVisitor<'a> {
    fn with_test_scope<F: FnOnce(&mut Self)>(&mut self, is_test: bool, f: F) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }
}

impl<'ast, 'a> Visit<'ast> for NoGlobImportVisitor<'a> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if self.in_test_depth == 0 && tree_has_glob(&node.tree) {
            let diag = Diagnostic::error(
                Rule::NoGlobImport.code(),
                "glob imports (`use foo::*`) are banned in strict mode",
                span_range(node.span()),
            )
            .with_why(Rule::NoGlobImport.rationale().to_string())
            .with_help("import only the symbols you use, fully qualified");
            self.diagnostics.push(diag);
        }
        visit::visit_item_use(self, node);
    }
}

fn run_no_glob_import(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoGlobImportVisitor {
        diagnostics,
        in_test_depth: 0,
    };
    v.visit_file(file);
}

const JUSTIFICATION_WINDOW: usize = 200;

fn leading_window(source: &str, start: usize) -> &str {
    let begin = start.saturating_sub(JUSTIFICATION_WINDOW);
    let begin = clamp_to_char_boundary(source, begin);
    let end = start.min(source.len());
    &source[begin..end]
}

fn clamp_to_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx > s.len() {
        idx = s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Is `marker` present in the comment lines justifying the site at byte
/// `start`? Two accept paths (RT-91):
///
/// 1. The **contiguous comment block** directly above the site's line — every
///    line walking upward whose trimmed form starts with `//`. This is the
///    natural place for a justification, and unlike the byte window it can't
///    be defeated by writing a *thorough* multi-line comment whose marker
///    line scrolls past the window edge (how heck-strict's three-line
///    `// reason:` block managed to fail R0006).
/// 2. The legacy 200-byte window, for layouts the block walk misses.
fn justified_by_preceding_comments(source: &str, start: usize, marker: &str) -> bool {
    if window_contains_marker(leading_window(source, start), marker) {
        return true;
    }
    // Walk whole lines upward from the site's line.
    let start = clamp_to_char_boundary(source, start.min(source.len()));
    let site_line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let mut rest = &source[..site_line_start];
    for _ in 0..64 {
        // `rest` ends with the newline that terminated the previous line —
        // drop it so rfind locates the line's actual start.
        rest = rest.strip_suffix('\n').unwrap_or(rest);
        rest = rest.strip_suffix('\r').unwrap_or(rest);
        if rest.is_empty() {
            break;
        }
        let line_start = rest.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = rest[line_start..].trim_start();
        let body = if let Some(b) = line.strip_prefix("//") {
            b
        } else if let Some(b) = line.strip_prefix("/*") {
            b.trim_end_matches("*/")
        } else {
            break; // non-comment line ends the block
        };
        if body.to_ascii_lowercase().contains(marker) {
            return true;
        }
        if line_start == 0 {
            break;
        }
        rest = &rest[..line_start];
    }
    false
}

fn window_contains_marker(window: &str, marker: &str) -> bool {
    // We restrict the search to comment lines so a `safety:` appearing in
    // a string literal in nearby code doesn't satisfy the requirement.
    for line in window.lines() {
        let trimmed = line.trim_start();
        let body = if let Some(rest) = trimmed.strip_prefix("//") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("/*") {
            rest.trim_end_matches("*/")
        } else {
            continue;
        };
        if body.to_ascii_lowercase().contains(marker) {
            return true;
        }
    }
    false
}

/// Check whether any `#[doc = "..."]` / `///` attribute on an item mentions
/// a safety justification. Accepts both `safety:` (inline prose) and
/// `# safety` (standard rustdoc section header). Anyhow-style crates write
/// `// Safety:` paragraphs in the doc comment rather than as inline block
/// comments, so the 200-byte leading-window check misses them.
fn doc_attrs_contain_marker(attrs: &[syn::Attribute], marker: &str) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
            {
                let lower = s.value().to_ascii_lowercase();
                // Accept "safety:" (inline) and "# safety" (rustdoc section).
                let alt_marker = marker.trim_end_matches(':');
                if lower.contains(marker) || lower.contains(&format!("# {alt_marker}")) {
                    return true;
                }
            }
        }
    }
    false
}

struct JustifyUnsafeVisitor<'a, 'src> {
    diagnostics: &'a mut Vec<Diagnostic>,
    source: &'src str,
}

impl<'ast, 'a, 'src> Visit<'ast> for JustifyUnsafeVisitor<'a, 'src> {
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        let span = node.unsafe_token.span();
        let range = span_range(span);
        let window = leading_window(self.source, range.start);
        if !window_contains_marker(window, "safety:") {
            let diag = Diagnostic::error(
                Rule::JustifyUnsafe.code(),
                "`unsafe` block missing `// safety:` justification",
                range,
            )
            .with_why(Rule::JustifyUnsafe.rationale().to_string())
            .with_help("add a `// safety:` comment in the 200 bytes preceding this block");
            self.diagnostics.push(diag);
        }
        visit::visit_expr_unsafe(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.unsafety.is_some() {
            let span = node
                .sig
                .unsafety
                .as_ref()
                .map(|u| u.span())
                .unwrap_or_else(|| node.sig.span());
            let range = span_range(span);
            let window = leading_window(self.source, range.start);
            let justified = window_contains_marker(window, "safety:")
                || doc_attrs_contain_marker(&node.attrs, "safety:");
            if !justified {
                let diag = Diagnostic::error(
                    Rule::JustifyUnsafe.code(),
                    "`unsafe fn` missing `// safety:` justification",
                    range,
                )
                .with_why(Rule::JustifyUnsafe.rationale().to_string())
                .with_help("add a `// safety:` comment in the 200 bytes preceding this function, or in the function's doc comment");
                self.diagnostics.push(diag);
            }
        }
        visit::visit_item_fn(self, node);
    }
}

// ── RT-68/72/73/74: Tier 1/3 rules (eval validation still owed) ──────────
//
// These four rules were specified as eval-gated; they are implemented with
// deliberately narrow triggers and the cfg(test) exemption, and their eval
// validation is tracked on the tickets. Keep triggers narrow — widen only
// with eval evidence.

/// Shared cfg(test)-scoped expression visitor driver: walks the file, tracks
/// test scopes like NoUnwrapVisitor, and calls `check` on every method call.
struct TestScopedVisitor<'a, F: FnMut(&syn::ExprMethodCall, &mut Vec<Diagnostic>)> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
    check: F,
}

impl<'a, F: FnMut(&syn::ExprMethodCall, &mut Vec<Diagnostic>)> TestScopedVisitor<'a, F> {
    fn with_test_scope<G: FnOnce(&mut Self)>(&mut self, is_test: bool, f: G) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }
}

impl<'ast, 'a, F: FnMut(&syn::ExprMethodCall, &mut Vec<Diagnostic>)> Visit<'ast>
    for TestScopedVisitor<'a, F>
{
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| visit::visit_item_fn(this, node));
    }
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if self.in_test_depth == 0 {
            (self.check)(node, self.diagnostics);
        }
        visit::visit_expr_method_call(self, node);
    }
}

/// R0018: `.map_err(|_| …)` and `.ok().expect(…)` discard the source error.
fn run_no_error_context_drop(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = TestScopedVisitor {
        diagnostics,
        in_test_depth: 0,
        check: |node: &syn::ExprMethodCall, diags: &mut Vec<Diagnostic>| {
            let flag = |diags: &mut Vec<Diagnostic>, span: proc_macro2::Span, what: &str| {
                diags.push(
                    Diagnostic::error(Rule::NoErrorContextDrop.code(), what, span_range(span))
                        .with_why(Rule::NoErrorContextDrop.rationale().to_string())
                        .with_help(Rule::NoErrorContextDrop.instead().to_string()),
                );
            };
            if node.method == "map_err" && node.args.len() == 1 {
                if let syn::Expr::Closure(c) = &node.args[0] {
                    if c.inputs.len() == 1 && matches!(c.inputs.first(), Some(syn::Pat::Wild(_))) {
                        flag(
                            diags,
                            node.method.span(),
                            "`.map_err(|_| …)` discards the source error",
                        );
                    }
                }
            }
            if node.method == "expect" {
                if let syn::Expr::MethodCall(inner) = &*node.receiver {
                    if inner.method == "ok" && inner.args.is_empty() {
                        flag(
                            diags,
                            node.method.span(),
                            "`.ok().expect(…)` throws away the original error before panicking",
                        );
                    }
                }
            }
        },
    };
    v.visit_file(file);
}

/// Does this expression (stripping parens/refs) end in a `.len()` call?
fn is_len_call(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(m) => m.method == "len" && m.args.is_empty(),
        syn::Expr::Paren(p) => is_len_call(&p.expr),
        syn::Expr::Reference(r) => is_len_call(&r.expr),
        _ => false,
    }
}

/// R0019: bare `+`/`-`/`*` with a `.len()` operand.
fn run_no_unchecked_len_arith(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    struct V<'a> {
        diagnostics: &'a mut Vec<Diagnostic>,
        in_test_depth: usize,
    }
    impl<'ast, 'a> Visit<'ast> for V<'a> {
        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            let is_test = attrs_have_cfg_test(&node.attrs)
                || node.attrs.iter().any(|a| a.path().is_ident("test"));
            if is_test {
                self.in_test_depth += 1;
            }
            visit::visit_item_fn(self, node);
            if is_test {
                self.in_test_depth -= 1;
            }
        }
        fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
            let is_test = attrs_have_cfg_test(&node.attrs);
            if is_test {
                self.in_test_depth += 1;
            }
            visit::visit_item_mod(self, node);
            if is_test {
                self.in_test_depth -= 1;
            }
        }
        fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
            use syn::BinOp;
            let arith = matches!(node.op, BinOp::Add(_) | BinOp::Sub(_) | BinOp::Mul(_));
            if self.in_test_depth == 0
                && arith
                && (is_len_call(&node.left) || is_len_call(&node.right))
            {
                self.diagnostics.push(
                    Diagnostic::error(
                        Rule::NoUncheckedLenArith.code(),
                        "bare arithmetic on a `.len()` value — debug panics, release wraps",
                        span_range(node.op.span()),
                    )
                    .with_why(Rule::NoUncheckedLenArith.rationale().to_string())
                    .with_help(Rule::NoUncheckedLenArith.instead().to_string()),
                );
            }
            visit::visit_expr_binary(self, node);
        }
    }
    let mut v = V {
        diagnostics,
        in_test_depth: 0,
    };
    v.visit_file(file);
}

/// Does this expression contain a *sync* guard acquisition — a
/// `.lock()`/`.read()`/`.write()` immediately unwrapped with
/// `.unwrap()`/`.expect(…)`? (An async lock would be `.lock().await`, so the
/// unwrap/expect discriminates std::sync from tokio::sync at the syntax
/// level.)
fn contains_sync_guard_acquisition(expr: &syn::Expr) -> bool {
    struct Finder(bool);
    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            if node.method == "unwrap" || node.method == "expect" {
                if let syn::Expr::MethodCall(inner) = &*node.receiver {
                    if inner.method == "lock" || inner.method == "read" || inner.method == "write" {
                        self.0 = true;
                    }
                }
            }
            visit::visit_expr_method_call(self, node);
        }
    }
    let mut f = Finder(false);
    f.visit_expr(expr);
    f.0
}

fn contains_await(expr: &syn::Expr) -> bool {
    struct Finder(bool);
    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_await(&mut self, _: &'ast syn::ExprAwait) {
            self.0 = true;
        }
    }
    let mut f = Finder(false);
    f.visit_expr(expr);
    f.0
}

/// R0020: a sync guard bound by a `let` with an `.await` later in the same
/// async block. Statement-level, no dataflow: a guard *dropped* before the
/// await (scoped in its own block) never surfaces as a flagged `let`.
fn run_no_lock_across_await(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    fn check_async_block(block: &syn::Block, diagnostics: &mut Vec<Diagnostic>) {
        for (i, stmt) in block.stmts.iter().enumerate() {
            let syn::Stmt::Local(local) = stmt else {
                continue;
            };
            let Some(init) = &local.init else { continue };
            if !contains_sync_guard_acquisition(&init.expr) {
                continue;
            }
            let awaited_later = block.stmts.iter().skip(i + 1).any(|later| {
                let expr = match later {
                    syn::Stmt::Expr(e, _) => Some(e),
                    syn::Stmt::Local(l) => l.init.as_ref().map(|init| &*init.expr),
                    _ => None,
                };
                expr.map(contains_await).unwrap_or(false)
            });
            if awaited_later {
                diagnostics.push(
                    Diagnostic::error(
                        Rule::NoLockAcrossAwait.code(),
                        "sync lock guard held across a later `.await` in this block",
                        span_range(local.let_token.span),
                    )
                    .with_why(Rule::NoLockAcrossAwait.rationale().to_string())
                    .with_help(Rule::NoLockAcrossAwait.instead().to_string()),
                );
            }
        }
    }

    struct V<'a> {
        diagnostics: &'a mut Vec<Diagnostic>,
    }
    impl<'ast, 'a> Visit<'ast> for V<'a> {
        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            if node.sig.asyncness.is_some() {
                check_async_block(&node.block, self.diagnostics);
            }
            visit::visit_item_fn(self, node);
        }
        fn visit_expr_async(&mut self, node: &'ast syn::ExprAsync) {
            check_async_block(&node.block, self.diagnostics);
            visit::visit_expr_async(self, node);
        }
    }
    let mut v = V { diagnostics };
    v.visit_file(file);
}

fn contains_capacity_call(expr: &syn::Expr) -> bool {
    struct Finder(bool);
    impl<'ast> Visit<'ast> for Finder {
        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            if node.method == "capacity" && node.args.is_empty() {
                self.0 = true;
            }
            visit::visit_expr_method_call(self, node);
        }
    }
    let mut f = Finder(false);
    f.visit_expr(expr);
    f.0
}

/// R0021: `.capacity()` as an index or range bound.
fn run_no_capacity_as_len(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    struct V<'a> {
        diagnostics: &'a mut Vec<Diagnostic>,
    }
    impl<'a> V<'a> {
        fn flag(&mut self, span: proc_macro2::Span, what: &str) {
            self.diagnostics.push(
                Diagnostic::error(Rule::NoCapacityAsLen.code(), what, span_range(span))
                    .with_why(Rule::NoCapacityAsLen.rationale().to_string())
                    .with_help(Rule::NoCapacityAsLen.instead().to_string()),
            );
        }
    }
    impl<'ast, 'a> Visit<'ast> for V<'a> {
        fn visit_expr_index(&mut self, node: &'ast syn::ExprIndex) {
            if contains_capacity_call(&node.index) {
                self.flag(
                    node.bracket_token.span.join(),
                    "`.capacity()` used as an index — element count is `.len()`",
                );
            }
            visit::visit_expr_index(self, node);
        }
        fn visit_expr_range(&mut self, node: &'ast syn::ExprRange) {
            let in_bound = node
                .start
                .as_deref()
                .map(contains_capacity_call)
                .unwrap_or(false)
                || node
                    .end
                    .as_deref()
                    .map(contains_capacity_call)
                    .unwrap_or(false);
            if in_bound {
                self.flag(
                    node.limits.span(),
                    "`.capacity()` used as a range bound — element count is `.len()`",
                );
            }
            visit::visit_expr_range(self, node);
        }
    }
    let mut v = V { diagnostics };
    v.visit_file(file);
}

/// RT-91: token-level site discovery for the comment-window rules.
///
/// R0005/R0006 check the 200 bytes of *original* source preceding a site for
/// a justification comment. The AST visitors below get their spans from the
/// **lowered** parse, and prettyplease strips comments during lowering — so
/// lowered offsets drift against the original text and the window check
/// misses justifications that are plainly there (or finds ones that aren't).
///
/// The fix: discover sites by tokenizing the ORIGINAL source. proc-macro2
/// happily lexes Trust syntax (named args, pipe — they're valid tokens), and
/// its spans index the original string exactly. The lowering rewrites never
/// add or remove `unsafe` tokens or `#[allow]` attributes, so the token walk
/// sees the same sites the AST would. `macro_rules!` bodies are skipped —
/// template code isn't checked by the AST path either.
mod window_sites {
    use proc_macro2::{Delimiter, TokenStream, TokenTree};
    use std::ops::Range;

    pub struct UnsafeSite {
        pub range: Range<usize>,
        pub is_fn: bool,
        /// Doc-comment text gathered from `#[doc = "..."]` attrs directly
        /// preceding an `unsafe fn` (rustdoc `/// # Safety` sections count
        /// as justification).
        pub doc_text: String,
    }

    pub struct AllowSite {
        pub range: Range<usize>,
        pub has_inline_reason: bool,
    }

    pub fn scan(source: &str) -> Option<(Vec<UnsafeSite>, Vec<AllowSite>)> {
        let tokens: TokenStream = source.parse().ok()?;
        let mut unsafes = Vec::new();
        let mut allows = Vec::new();
        walk(&tokens, &mut unsafes, &mut allows);
        // Sites surface in token order within each level but nested groups
        // append after their siblings; sort so windows are deterministic.
        unsafes.sort_by_key(|s| s.range.start);
        allows.sort_by_key(|s| s.range.start);
        Some((unsafes, allows))
    }

    fn walk(tokens: &TokenStream, unsafes: &mut Vec<UnsafeSite>, allows: &mut Vec<AllowSite>) {
        let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
        for (i, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Ident(id) if *id == "unsafe" => {
                    match trees.get(i + 1) {
                        Some(TokenTree::Ident(next)) if *next == "fn" => {
                            unsafes.push(UnsafeSite {
                                range: byte_range(tree),
                                is_fn: true,
                                doc_text: preceding_doc_text(&trees, i),
                            });
                        }
                        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
                            unsafes.push(UnsafeSite {
                                range: byte_range(tree),
                                is_fn: false,
                                doc_text: String::new(),
                            });
                        }
                        // `unsafe impl` / `unsafe trait` / `unsafe extern`:
                        // not flagged by R0005 (parity with the AST rule).
                        _ => {}
                    }
                }
                TokenTree::Punct(p) if p.as_char() == '#' => {
                    if let Some(site) = allow_site_at(&trees, i) {
                        allows.push(site);
                    }
                }
                _ => {}
            }
        }
        for (i, tree) in trees.iter().enumerate() {
            if let TokenTree::Group(g) = tree {
                if g.delimiter() == Delimiter::Brace && is_macro_rules_body(&trees, i) {
                    continue;
                }
                walk(&g.stream(), unsafes, allows);
            }
        }
    }

    fn byte_range(tree: &TokenTree) -> Range<usize> {
        tree.span().byte_range()
    }

    /// `trees[..i]` ends with `macro_rules ! IDENT`, making `trees[i]` a
    /// macro definition body.
    fn is_macro_rules_body(trees: &[TokenTree], i: usize) -> bool {
        if i < 3 {
            return false;
        }
        matches!(
            (&trees[i - 3], &trees[i - 2], &trees[i - 1]),
            (TokenTree::Ident(kw), TokenTree::Punct(bang), TokenTree::Ident(_))
                if *kw == "macro_rules" && bang.as_char() == '!'
        )
    }

    /// Gather `#[doc = "..."]` string contents from the attribute run
    /// directly preceding `trees[i]`, skipping visibility / qualifier tokens
    /// (`pub`, `pub(crate)`, `const`, `async`, `extern "C"`).
    fn preceding_doc_text(trees: &[TokenTree], i: usize) -> String {
        let mut out = String::new();
        let mut j = i;
        while j > 0 {
            j -= 1;
            let Some(tree) = trees.get(j) else { break };
            match tree {
                TokenTree::Ident(id)
                    if *id == "pub" || *id == "const" || *id == "async" || *id == "extern" => {}
                TokenTree::Literal(_) => {} // the "C" in extern "C"
                TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis => {} // pub(crate)
                TokenTree::Group(g) if g.delimiter() == Delimiter::Bracket => {
                    // Possible attribute body — collect doc strings.
                    if j == 0 || !matches!(&trees[j - 1], TokenTree::Punct(p) if p.as_char() == '#')
                    {
                        break;
                    }
                    let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                    if let [TokenTree::Ident(name), TokenTree::Punct(eq), TokenTree::Literal(lit)] =
                        inner.as_slice()
                    {
                        if *name == "doc" && eq.as_char() == '=' {
                            out.push_str(&lit.to_string());
                            out.push('\n');
                        }
                    }
                    j -= 1; // consume the `#`
                }
                _ => break,
            }
        }
        out
    }

    /// If `trees[i..]` is `# [!]? [ (allow|expect) ( ... ) ]`, build the site.
    fn allow_site_at(trees: &[TokenTree], i: usize) -> Option<AllowSite> {
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
        if (*name != "allow" && *name != "expect") || list.delimiter() != Delimiter::Parenthesis {
            return None;
        }
        // Top-level `reason` ident followed by `=` marks an inline reason.
        let items: Vec<TokenTree> = list.stream().into_iter().collect();
        let has_inline_reason = items.windows(2).any(|w| {
            matches!(
                (&w[0], &w[1]),
                (TokenTree::Ident(id), TokenTree::Punct(eq))
                    if *id == "reason" && eq.as_char() == '='
            )
        });
        Some(AllowSite {
            range: trees.get(i)?.span().byte_range(),
            has_inline_reason,
        })
    }
}

fn run_justify_unsafe(file: &syn::File, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    // Token-accurate path (RT-91): sites + spans from the original source.
    if let Some((unsafes, _)) = window_sites::scan(source) {
        for site in unsafes {
            let mut justified =
                justified_by_preceding_comments(source, site.range.start, "safety:");
            if !justified && site.is_fn {
                let lower = site.doc_text.to_ascii_lowercase();
                justified = lower.contains("safety:") || lower.contains("# safety");
            }
            if !justified {
                let (what, help) = if site.is_fn {
                    (
                        "`unsafe fn` missing `// safety:` justification",
                        "add a `// safety:` comment in the 200 bytes preceding this function, or in the function's doc comment",
                    )
                } else {
                    (
                        "`unsafe` block missing `// safety:` justification",
                        "add a `// safety:` comment in the 200 bytes preceding this block",
                    )
                };
                diagnostics.push(
                    Diagnostic::error(Rule::JustifyUnsafe.code(), what, site.range)
                        .with_why(Rule::JustifyUnsafe.rationale().to_string())
                        .with_help(help),
                );
            }
        }
        return;
    }
    // Fallback (source failed to tokenize — shouldn't happen for inputs that
    // reached the linter): the lowered-AST visitor.
    let mut v = JustifyUnsafeVisitor {
        diagnostics,
        source,
    };
    v.visit_file(file);
}

struct JustifyAllowVisitor<'a, 'src> {
    diagnostics: &'a mut Vec<Diagnostic>,
    source: &'src str,
}

impl<'a, 'src> JustifyAllowVisitor<'a, 'src> {
    fn check_attrs(&mut self, attrs: &[syn::Attribute]) {
        for attr in attrs {
            if !attr.path().is_ident("allow") {
                continue;
            }
            // An allow that carries its own `reason = "..."` argument (the
            // `#[allow(trust::Rxxxx, reason = "…")]` form from RT-46) is
            // self-justifying — demanding a `// reason:` comment on top of it
            // would be redundant, and the comment-window check cannot work
            // through the lowering pipeline anyway (prettyplease strips
            // comments, so lowered-AST offsets drift against the original
            // source — RT-91).
            if attr_has_inline_reason(attr) {
                continue;
            }
            let span = attr.span();
            let range = span_range(span);
            let window = leading_window(self.source, range.start);
            if !window_contains_marker(window, "reason:") {
                let diag = Diagnostic::error(
                    Rule::JustifyAllow.code(),
                    "`#[allow(...)]` missing `// reason:` justification",
                    range,
                )
                .with_why(Rule::JustifyAllow.rationale().to_string())
                .with_help("add a `// reason:` comment in the 200 bytes preceding this attribute");
                self.diagnostics.push(diag);
            }
        }
    }
}

impl<'ast, 'a, 'src> Visit<'ast> for JustifyAllowVisitor<'a, 'src> {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        if let Some(attrs) = item_attrs(node) {
            self.check_attrs(attrs);
        }
        visit::visit_item(self, node);
    }

    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        let attrs = match node {
            syn::ImplItem::Const(c) => Some(&c.attrs),
            syn::ImplItem::Fn(f) => Some(&f.attrs),
            syn::ImplItem::Type(t) => Some(&t.attrs),
            syn::ImplItem::Macro(m) => Some(&m.attrs),
            _ => None,
        };
        if let Some(a) = attrs {
            self.check_attrs(a);
        }
        visit::visit_impl_item(self, node);
    }

    fn visit_trait_item(&mut self, node: &'ast syn::TraitItem) {
        let attrs = match node {
            syn::TraitItem::Const(c) => Some(&c.attrs),
            syn::TraitItem::Fn(f) => Some(&f.attrs),
            syn::TraitItem::Type(t) => Some(&t.attrs),
            syn::TraitItem::Macro(m) => Some(&m.attrs),
            _ => None,
        };
        if let Some(a) = attrs {
            self.check_attrs(a);
        }
        visit::visit_trait_item(self, node);
    }

    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if let syn::Stmt::Local(local) = node {
            self.check_attrs(&local.attrs);
        }
        visit::visit_stmt(self, node);
    }
}

fn item_attrs(item: &syn::Item) -> Option<&Vec<syn::Attribute>> {
    match item {
        syn::Item::Const(i) => Some(&i.attrs),
        syn::Item::Enum(i) => Some(&i.attrs),
        syn::Item::ExternCrate(i) => Some(&i.attrs),
        syn::Item::Fn(i) => Some(&i.attrs),
        syn::Item::ForeignMod(i) => Some(&i.attrs),
        syn::Item::Impl(i) => Some(&i.attrs),
        syn::Item::Macro(i) => Some(&i.attrs),
        syn::Item::Mod(i) => Some(&i.attrs),
        syn::Item::Static(i) => Some(&i.attrs),
        syn::Item::Struct(i) => Some(&i.attrs),
        syn::Item::Trait(i) => Some(&i.attrs),
        syn::Item::TraitAlias(i) => Some(&i.attrs),
        syn::Item::Type(i) => Some(&i.attrs),
        syn::Item::Union(i) => Some(&i.attrs),
        syn::Item::Use(i) => Some(&i.attrs),
        _ => None,
    }
}

/// Does this `#[allow(...)]` attribute carry a `reason = "..."` argument?
fn attr_has_inline_reason(attr: &syn::Attribute) -> bool {
    let mut found = false;
    let _ = attr.parse_nested_meta(|m| {
        if m.path.is_ident("reason") {
            let _: syn::LitStr = m.value()?.parse()?;
            found = true;
        } else if m.input.peek(syn::Token![=]) {
            let _: syn::Expr = m.value()?.parse()?;
        }
        Ok(())
    });
    found
}

fn run_justify_allow(file: &syn::File, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    // Token-accurate path (RT-91): sites + spans from the original source.
    if let Some((_, allows)) = window_sites::scan(source) {
        for site in allows {
            if site.has_inline_reason {
                // `#[allow(..., reason = "…")]` is self-justifying (RT-89).
                continue;
            }
            if !justified_by_preceding_comments(source, site.range.start, "reason:") {
                diagnostics.push(
                    Diagnostic::error(
                        Rule::JustifyAllow.code(),
                        "`#[allow(...)]` missing `// reason:` justification",
                        site.range,
                    )
                    .with_why(Rule::JustifyAllow.rationale().to_string())
                    .with_help(
                        "add a `// reason:` comment in the 200 bytes preceding this attribute",
                    ),
                );
            }
        }
        return;
    }
    // Fallback: the lowered-AST visitor.
    let mut v = JustifyAllowVisitor {
        diagnostics,
        source,
    };
    // Crate-level `#![allow(...)]` attributes live on the file itself.
    v.check_attrs(&file.attrs);
    v.visit_file(file);
}

fn item_has_macros_ok(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let segments: Vec<String> = attr
            .path()
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        segments == ["strict", "macros_ok"]
    })
}

struct NoUserMacrosVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    mod_opt_out_depth: usize,
    in_test_depth: usize,
}

impl<'a> NoUserMacrosVisitor<'a> {
    fn with_scope<F: FnOnce(&mut Self)>(&mut self, opt_out: bool, is_test: bool, f: F) {
        if opt_out {
            self.mod_opt_out_depth += 1;
        }
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if opt_out {
            self.mod_opt_out_depth -= 1;
        }
        if is_test {
            self.in_test_depth -= 1;
        }
    }
}

impl<'ast, 'a> Visit<'ast> for NoUserMacrosVisitor<'a> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let opt_out = item_has_macros_ok(&node.attrs);
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_scope(opt_out, is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if node.mac.path.is_ident("macro_rules")
            && self.mod_opt_out_depth == 0
            && self.in_test_depth == 0
            && !item_has_macros_ok(&node.attrs)
        {
            let diag = Diagnostic::error(
                Rule::NoUserMacros.code(),
                "user-defined `macro_rules!` requires `#[strict::macros_ok]` opt-in",
                span_range(node.span()),
            )
            .with_why(Rule::NoUserMacros.rationale().to_string())
            .with_help("add `#[strict::macros_ok]` to this item or its enclosing module to opt in");
            self.diagnostics.push(diag);
        }
        visit::visit_item_macro(self, node);
    }
}

fn run_no_user_macros(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoUserMacrosVisitor {
        diagnostics,
        mod_opt_out_depth: 0,
        in_test_depth: 0,
    };
    v.visit_file(file);
}

// ----------------------------------------------------------------------------
// R0007 — no-impl-trait-return
// ----------------------------------------------------------------------------

fn return_is_impl_trait(sig: &syn::Signature) -> Option<&syn::TypeImplTrait> {
    let syn::ReturnType::Type(_, ty) = &sig.output else {
        return None;
    };
    let syn::Type::ImplTrait(it) = ty.as_ref() else {
        return None;
    };
    Some(it)
}

struct NoImplTraitReturnVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast, 'a> Visit<'ast> for NoImplTraitReturnVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if let Some(it) = return_is_impl_trait(&node.sig) {
            self.emit(it.span());
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if let Some(it) = return_is_impl_trait(&node.sig) {
            self.emit(it.span());
        }
        visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if let Some(it) = return_is_impl_trait(&node.sig) {
            self.emit(it.span());
        }
        visit::visit_trait_item_fn(self, node);
    }
}

impl<'a> NoImplTraitReturnVisitor<'a> {
    fn emit(&mut self, span: Span) {
        let diag = Diagnostic::error(
            Rule::NoImplTraitReturn.code(),
            "`impl Trait` in return position is banned in strict mode",
            span_range(span),
        )
        .with_why(Rule::NoImplTraitReturn.rationale().to_string())
        .with_help("name the type with a `type Alias = ...;` and return the alias");
        self.diagnostics.push(diag);
    }
}

fn run_no_impl_trait_return(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoImplTraitReturnVisitor { diagnostics };
    v.visit_file(file);
}

// ----------------------------------------------------------------------------
// R0010 — no-todo-macro
// R0011 — no-panic
// ----------------------------------------------------------------------------

/// Shared scaffolding for macro-name lints that should ignore `#[cfg(test)]`
/// scopes. The `targets` slice lists macro identifiers (last path segment)
/// that should produce a diagnostic.
struct MacroBanVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
    rule: Rule,
    targets: &'static [&'static str],
    help: &'static str,
}

impl<'a> MacroBanVisitor<'a> {
    fn with_test_scope<F: FnOnce(&mut Self)>(&mut self, is_test: bool, f: F) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }
}

impl<'ast, 'a> Visit<'ast> for MacroBanVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| visit::visit_item_fn(this, node));
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_impl(this, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_impl_item_fn(this, node));
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if self.in_test_depth == 0 {
            if let Some(seg) = node.path.segments.last() {
                let name = seg.ident.to_string();
                if self.targets.iter().any(|t| *t == name) {
                    let diag = Diagnostic::error(
                        self.rule.code(),
                        format!("`{name}!` is banned outside `#[cfg(test)]` in strict mode"),
                        span_range(node.path.span()),
                    )
                    .with_why(self.rule.rationale().to_string())
                    .with_help(self.help);
                    self.diagnostics.push(diag);
                }
            }
        }
        visit::visit_macro(self, node);
    }
}

fn run_no_todo_macro(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = MacroBanVisitor {
        diagnostics,
        in_test_depth: 0,
        rule: Rule::NoTodoMacro,
        targets: &["todo", "unimplemented"],
        help: "implement the function or return a typed `Err`",
    };
    v.visit_file(file);
}

fn run_no_panic(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = MacroBanVisitor {
        diagnostics,
        in_test_depth: 0,
        rule: Rule::NoPanic,
        targets: &["panic"],
        help: "return a typed `Err` and let the caller decide whether to abort",
    };
    v.visit_file(file);
}

// ----------------------------------------------------------------------------
// R0012 — no-bool-param
// ----------------------------------------------------------------------------

fn is_visible(vis: &syn::Visibility) -> bool {
    !matches!(vis, syn::Visibility::Inherited)
}

fn ty_is_bool(ty: &syn::Type) -> bool {
    let syn::Type::Path(tp) = ty else {
        return false;
    };
    if tp.qself.is_some() {
        return false;
    }
    let last = match tp.path.segments.last() {
        Some(seg) => seg,
        None => return false,
    };
    last.ident == "bool" && tp.path.segments.len() == 1
}

struct NoBoolParamVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
}

impl<'a> NoBoolParamVisitor<'a> {
    fn with_test_scope<F: FnOnce(&mut Self)>(&mut self, is_test: bool, f: F) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }

    fn check_sig(&mut self, sig: &syn::Signature, vis_visible: bool) {
        if self.in_test_depth > 0 || !vis_visible {
            return;
        }
        for input in &sig.inputs {
            let syn::FnArg::Typed(pat_ty) = input else {
                continue;
            };
            if ty_is_bool(&pat_ty.ty) {
                let pat_name = match pat_ty.pat.as_ref() {
                    syn::Pat::Ident(p) => p.ident.to_string(),
                    _ => "<param>".to_string(),
                };
                let diag = Diagnostic::error(
                    Rule::NoBoolParam.code(),
                    format!(
                        "visible function `{}` takes `bool` parameter `{pat_name}`",
                        sig.ident
                    ),
                    span_range(pat_ty.ty.span()),
                )
                .with_why(Rule::NoBoolParam.rationale().to_string())
                .with_help("replace with a named enum (e.g. `enum Mode { On, Off }`)");
                self.diagnostics.push(diag);
            }
        }
    }
}

impl<'ast, 'a> Visit<'ast> for NoBoolParamVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| {
            this.check_sig(&node.sig, is_visible(&node.vis));
            visit::visit_item_fn(this, node);
        });
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        // Methods inside inherent impls inherit visibility from the impl
        // block's own visibility — but inherent impls don't have an outer
        // visibility token, so we treat them as visible iff the method itself
        // has a visibility modifier. Trait impls expose all methods publicly.
        self.with_test_scope(is_test, |this| visit::visit_item_impl(this, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        // Trait-impl methods are always exposed at the visibility of the
        // trait itself; treat them as visible. Inherent impl methods use
        // their own visibility token.
        self.check_sig(&node.sig, is_visible(&node.vis));
        visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        // Trait methods are visible at the trait's visibility, which is
        // always at least as wide as the surrounding scope.
        self.check_sig(&node.sig, true);
        visit::visit_trait_item_fn(self, node);
    }
}

fn run_no_bool_param(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoBoolParamVisitor {
        diagnostics,
        in_test_depth: 0,
    };
    v.visit_file(file);
}

// ----------------------------------------------------------------------------
// R0014 — no-bare-index
// ----------------------------------------------------------------------------

/// `true` for index expressions the lint should leave alone:
/// - integer literals (`v[0]`, `v[7]`) — intentional const access
/// - range expressions (`v[0..5]`, `v[..n]`, `v[i..]`, `v[..]`) — slice
///   operations have different ergonomics from single-element indexing;
///   without this exemption the lint fires on every `&s[..n]` truncation
///   pattern. Range bounds can still panic on overflow, but flagging them
///   produces too many false positives on real code (see
///   eval/false-positives/REPORT.md, R0014 30.4% FP rate before this fix).
fn index_is_int_literal(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(_),
            ..
        }) | syn::Expr::Range(_)
    )
}

/// RT-43: heuristic — does this index expression *look* like a `usize` /
/// integer position, as opposed to a key into a `Slab`/`IndexMap`?
///
/// We don't have type information at lint time, so this is purely
/// syntactic. The heuristic fires on:
/// - bare identifiers commonly used as numeric indices: `i`, `j`, `k`,
///   `n`, `idx`, `index`
/// - identifiers ending in `_idx`, `_index`, `_i` (e.g. `child_idx`)
/// - `.len()`-derived arithmetic: `xs.len() - 1`, `xs.len() / 2`
///
/// Everything else (key-shaped identifiers like `key`, `node_key`,
/// `entity_id`, or method calls returning unknown types) is treated as
/// a key-style access and is not flagged. Users who want the lint to
/// fire on those callsites can — but the inverse case (R0014 false
/// positive on arena types) is the more common failure mode and the
/// one this heuristic targets. Per-callsite suppression remains
/// available via `#[allow(trust::R0014, reason = "...")]` (RT-46).
fn index_looks_usize(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(p) if p.qself.is_none() => {
            let Some(ident) = p.path.get_ident() else {
                return false;
            };
            let name = ident.to_string();
            matches!(name.as_str(), "i" | "j" | "k" | "n" | "idx" | "index")
                || name.ends_with("_idx")
                || name.ends_with("_index")
                || name.ends_with("_i")
        }
        syn::Expr::Binary(b) => index_looks_usize(&b.left) || index_looks_usize(&b.right),
        syn::Expr::Paren(p) => index_looks_usize(&p.expr),
        syn::Expr::MethodCall(m) => m.method == "len",
        _ => false,
    }
}

struct NoBareIndexVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
}

impl<'a> NoBareIndexVisitor<'a> {
    fn with_test_scope<F: FnOnce(&mut Self)>(&mut self, is_test: bool, f: F) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }
}

impl<'ast, 'a> Visit<'ast> for NoBareIndexVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| visit::visit_item_fn(this, node));
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_impl(this, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_impl_item_fn(this, node));
    }

    fn visit_expr_index(&mut self, node: &'ast syn::ExprIndex) {
        // RT-43: only fire when the index expression *looks like* a usize
        // position. Bare-key indexing into `Slab`/`IndexMap`-style arena
        // types (where the index type is a `Key` newtype) no longer trips
        // R0014. Users wanting to ban every `expr[idx]` can still do so
        // via `#[deny]` or by tightening this heuristic locally.
        if self.in_test_depth == 0
            && !index_is_int_literal(&node.index)
            && index_looks_usize(&node.index)
        {
            let diag = Diagnostic::error(
                Rule::NoBareIndex.code(),
                "bare indexing `v[i]` with a usize-typed index is banned in strict mode",
                span_range(node.index.span()),
            )
            .with_why(Rule::NoBareIndex.rationale().to_string())
            .with_help(
                "use `.get(idx)` and handle the `Option`, or `#[allow(trust::R0014, reason = \"…\")]` if this is a key-style index",
            );
            self.diagnostics.push(diag);
        }
        visit::visit_expr_index(self, node);
    }
}

fn run_no_bare_index(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoBareIndexVisitor {
        diagnostics,
        in_test_depth: 0,
    };
    v.visit_file(file);
}

// ----------------------------------------------------------------------------
// R0017 — no-same-type-params
// ----------------------------------------------------------------------------

/// `true` if `ty` is exactly one of the function's own declared generic type
/// parameters (`fn f<T>(a: T, b: T)`). Two values of the same generic type
/// are usually intentional (`max`, `min`, `swap`), so the rule exempts them —
/// it targets *concrete* same-typed params (`u32`, `u64`, `&str`, ID types).
fn ty_is_generic_param(ty: &syn::Type, generics: &[String]) -> bool {
    let syn::Type::Path(tp) = ty else {
        return false;
    };
    if tp.qself.is_some() || tp.path.segments.len() != 1 {
        return false;
    }
    let seg = &tp.path.segments[0];
    seg.arguments.is_empty() && generics.iter().any(|g| seg.ident == g.as_str())
}

fn param_name(pat: &syn::Pat) -> String {
    match pat {
        syn::Pat::Ident(p) => p.ident.to_string(),
        _ => "<param>".to_string(),
    }
}

struct NoSameTypeParamsVisitor<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
    in_test_depth: usize,
}

impl<'a> NoSameTypeParamsVisitor<'a> {
    fn with_test_scope<F: FnOnce(&mut Self)>(&mut self, is_test: bool, f: F) {
        if is_test {
            self.in_test_depth += 1;
        }
        f(self);
        if is_test {
            self.in_test_depth -= 1;
        }
    }

    fn check_sig(&mut self, sig: &syn::Signature, vis_visible: bool) {
        if self.in_test_depth > 0 || !vis_visible {
            return;
        }
        let generics: Vec<String> = sig
            .generics
            .type_params()
            .map(|tp| tp.ident.to_string())
            .collect();
        // Typed params in declaration order (a `self` receiver is not a
        // `Typed` arg, so it's naturally excluded).
        let typed: Vec<&syn::PatType> = sig
            .inputs
            .iter()
            .filter_map(|a| match a {
                syn::FnArg::Typed(pt) => Some(pt),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        // Compare each adjacent pair. `syn::Type: PartialEq` (the
        // `extra-traits` feature) gives a structural, whitespace-insensitive
        // comparison — no type inference needed, this is purely syntactic.
        for pair in typed.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.ty == b.ty && !ty_is_generic_param(&a.ty, &generics) {
                let na = param_name(&a.pat);
                let nb = param_name(&b.pat);
                let diag = Diagnostic::error(
                    Rule::NoSameTypeParams.code(),
                    format!(
                        "visible function `{}` has adjacent same-type parameters `{na}` and `{nb}`",
                        sig.ident
                    ),
                    span_range(b.ty.span()),
                )
                .with_why(Rule::NoSameTypeParams.rationale().to_string())
                .with_help(
                    "give each a distinct newtype — `trust_std::newtype!(pub Width(u32));` makes that a one-liner — so a swap is a type error; or `#[allow(trust::R0017, reason = \"…\")]` if the two are genuinely interchangeable",
                );
                self.diagnostics.push(diag);
            }
        }
    }
}

impl<'ast, 'a> Visit<'ast> for NoSameTypeParamsVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let is_test = attrs_have_cfg_test(&node.attrs)
            || node.attrs.iter().any(|a| a.path().is_ident("test"));
        self.with_test_scope(is_test, |this| {
            this.check_sig(&node.sig, is_visible(&node.vis));
            visit::visit_item_fn(this, node);
        });
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_mod(this, node));
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let is_test = attrs_have_cfg_test(&node.attrs);
        self.with_test_scope(is_test, |this| visit::visit_item_impl(this, node));
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.check_sig(&node.sig, is_visible(&node.vis));
        visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        self.check_sig(&node.sig, true);
        visit::visit_trait_item_fn(self, node);
    }
}

fn run_no_same_type_params(file: &syn::File, diagnostics: &mut Vec<Diagnostic>) {
    let mut v = NoSameTypeParamsVisitor {
        diagnostics,
        in_test_depth: 0,
    };
    v.visit_file(file);
}
