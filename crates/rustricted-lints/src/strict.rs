//! Strict-mode activation + per-rule dispatch.

use crate::Rule;
use proc_macro2::Span;
use rustricted_diag::Diagnostic;
use std::ops::Range;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// Returns `true` if the file has the inner attribute `#![strict]`.
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
        // R0042 emission lives in `rustricted_lower::named_args`, where the
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
        let is_test = attrs_have_cfg_test(&node.attrs);
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
}

impl<'ast, 'a> Visit<'ast> for NoGlobImportVisitor<'a> {
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        if tree_has_glob(&node.tree) {
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
    let mut v = NoGlobImportVisitor { diagnostics };
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
            if !window_contains_marker(window, "safety:") {
                let diag = Diagnostic::error(
                    Rule::JustifyUnsafe.code(),
                    "`unsafe fn` missing `// safety:` justification",
                    range,
                )
                .with_why(Rule::JustifyUnsafe.rationale().to_string())
                .with_help("add a `// safety:` comment in the 200 bytes preceding this function");
                self.diagnostics.push(diag);
            }
        }
        visit::visit_item_fn(self, node);
    }
}

fn run_justify_unsafe(file: &syn::File, source: &str, diagnostics: &mut Vec<Diagnostic>) {
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

fn run_justify_allow(file: &syn::File, source: &str, diagnostics: &mut Vec<Diagnostic>) {
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
}

impl<'ast, 'a> Visit<'ast> for NoUserMacrosVisitor<'a> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let opt_out = item_has_macros_ok(&node.attrs);
        if opt_out {
            self.mod_opt_out_depth += 1;
        }
        visit::visit_item_mod(self, node);
        if opt_out {
            self.mod_opt_out_depth -= 1;
        }
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if node.mac.path.is_ident("macro_rules")
            && self.mod_opt_out_depth == 0
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

fn index_is_int_literal(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(lit) => matches!(lit.lit, syn::Lit::Int(_)),
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
        if self.in_test_depth == 0 && !index_is_int_literal(&node.index) {
            let diag = Diagnostic::error(
                Rule::NoBareIndex.code(),
                "bare indexing `v[idx]` with a non-literal index is banned in strict mode",
                span_range(node.index.span()),
            )
            .with_why(Rule::NoBareIndex.rationale().to_string())
            .with_help("use `.get(idx)` and handle the `Option`");
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
