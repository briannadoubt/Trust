//! Per-callsite `#[allow(trust::Rxxxx, reason = "…")]` escape hatch.
//!
//! Walks the file once and produces an [`AllowMap`]: a list of
//! `(scope_range, suppressed_rules)` entries, one per item/statement that
//! carries an `#[allow(trust::...)]` attribute.
//!
//! Validation diagnostics (R0015 missing-reason, R0016 unknown-code) are
//! emitted during the walk. A malformed allow does *not* suppress the rules
//! it lists — the user has to fix the attribute first.
//!
//! Non-trust allows (`#[allow(dead_code)]`, `#[allow(clippy::xxx)]`)
//! are ignored entirely — R0006 still requires a leading `// reason:`
//! comment for those (the comment justification and the inline `reason =`
//! justification are orthogonal mechanisms, kept separate to avoid breaking
//! every existing R0006 escape hatch).

use crate::Rule;
use proc_macro2::Span;
use trust_diag::Diagnostic;
use std::collections::HashSet;
use std::ops::Range;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

/// A single allow-scope: while the cursor is inside `range`, every rule in
/// `rules` is suppressed for that span.
#[derive(Debug, Clone)]
pub(crate) struct AllowScope {
    pub range: Range<usize>,
    pub rules: HashSet<Rule>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct AllowMap {
    scopes: Vec<AllowScope>,
}

impl AllowMap {
    /// `true` if `(rule, span)` is suppressed by an enclosing
    /// `#[allow(trust::<rule>)]` attribute.
    pub fn is_suppressed(&self, rule: Rule, span: &Range<usize>) -> bool {
        // Crate-level `#![allow(...)]` produces a `range` of `0..file_len`,
        // so the zero-zero `span` of a diagnostic with a synthetic span is
        // still covered. For real spans we require strict containment of
        // the diag start inside the scope range.
        self.scopes.iter().any(|s| {
            s.rules.contains(&rule) && (span.start >= s.range.start && span.start < s.range.end)
        })
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

/// Parse a `#[allow(...)]` attribute. Returns:
/// - `None` if this attribute isn't an `#[allow(trust::...)]` form
///   (e.g. `#[allow(dead_code)]`, `#[allow(clippy::xxx)]`) — caller ignores.
/// - `Some(parsed)` with the rules listed, the reason (if any), and any
///   validation diagnostics that should be emitted.
fn parse_trust_allow(attr: &syn::Attribute) -> Option<ParsedAllow> {
    if !attr.path().is_ident("allow") && !attr.path().is_ident("expect") {
        return None;
    }
    let syn::Meta::List(_) = &attr.meta else {
        return None;
    };

    let mut rules: HashSet<Rule> = HashSet::new();
    let mut unknown_codes: Vec<(String, Range<usize>)> = Vec::new();
    let mut reason: Option<String> = None;
    let mut saw_trust = false;

    let parse_result = attr.parse_nested_meta(|m| {
        // `reason = "..."` argument — standard Rust attribute syntax.
        if m.path.is_ident("reason") {
            let val: syn::LitStr = m.value()?.parse()?;
            reason = Some(val.value());
            return Ok(());
        }
        // Two-segment path `trust::Rxxxx`.
        if m.path.segments.len() == 2 && m.path.segments[0].ident == "trust" {
            saw_trust = true;
            let code_ident = &m.path.segments[1].ident;
            let code = code_ident.to_string();
            match Rule::from_code(&code) {
                Some(r) => {
                    rules.insert(r);
                }
                None => {
                    unknown_codes.push((code, span_range(code_ident.span())));
                }
            }
            return Ok(());
        }
        // Non-trust path (e.g. `dead_code`, `clippy::xxx`) — accept and
        // skip. `attr.parse_nested_meta` doesn't require us to consume any
        // value; the inner-meta parser handles `path = expr` and `path(...)`
        // forms implicitly.
        if m.input.peek(syn::Token![=]) {
            let _: syn::Expr = m.value()?.parse()?;
        }
        Ok(())
    });

    // If the attribute didn't parse at all (very rare — malformed source),
    // we silently bail; rustc will catch it later.
    if parse_result.is_err() {
        return None;
    }
    if !saw_trust {
        return None;
    }

    Some(ParsedAllow {
        attr_range: span_range(attr.span()),
        rules,
        unknown_codes,
        reason,
    })
}

struct ParsedAllow {
    attr_range: Range<usize>,
    rules: HashSet<Rule>,
    unknown_codes: Vec<(String, Range<usize>)>,
    reason: Option<String>,
}

fn known_codes_help() -> String {
    let codes: Vec<&'static str> = crate::rules::ALL.iter().map(|r| r.code()).collect();
    format!("known rule codes: {}", codes.join(", "))
}

fn process_attrs(
    attrs: &[syn::Attribute],
    scope: Range<usize>,
    map: &mut AllowMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut accumulated: HashSet<Rule> = HashSet::new();
    for attr in attrs {
        let Some(parsed) = parse_trust_allow(attr) else {
            continue;
        };
        // R0016: unknown codes — always emitted, regardless of reason.
        for (code, range) in &parsed.unknown_codes {
            let diag = Diagnostic::error(
                Rule::AllowUnknownCode.code(),
                format!("`#[allow(trust::{code})]` references an unknown rule code"),
                range.clone(),
            )
            .with_why(Rule::AllowUnknownCode.rationale().to_string())
            .with_help(known_codes_help());
            diagnostics.push(diag);
        }
        // R0015: missing or empty `reason = "..."`.
        let reason_ok = parsed
            .reason
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if !reason_ok {
            let diag = Diagnostic::error(
                Rule::AllowMissingReason.code(),
                "`#[allow(trust::...)]` missing `reason = \"...\"` justification",
                parsed.attr_range.clone(),
            )
            .with_why(Rule::AllowMissingReason.rationale().to_string())
            .with_help(
                "add a `reason = \"...\"` argument explaining why this rule is suppressed here",
            );
            diagnostics.push(diag);
            // A malformed allow does NOT suppress. Force the listed rules
            // to keep firing so the user can't accidentally silence work.
            continue;
        }
        accumulated.extend(parsed.rules.iter().copied());
    }
    if !accumulated.is_empty() {
        map.scopes.push(AllowScope {
            range: scope,
            rules: accumulated,
        });
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

fn stmt_attrs(stmt: &syn::Stmt) -> Option<&[syn::Attribute]> {
    match stmt {
        syn::Stmt::Local(l) => Some(&l.attrs),
        syn::Stmt::Item(i) => item_attrs(i).map(|v| v.as_slice()),
        syn::Stmt::Expr(e, _) => expr_attrs(e),
        syn::Stmt::Macro(m) => Some(&m.attrs),
    }
}

fn expr_attrs(expr: &syn::Expr) -> Option<&[syn::Attribute]> {
    match expr {
        syn::Expr::Array(e) => Some(&e.attrs),
        syn::Expr::Assign(e) => Some(&e.attrs),
        syn::Expr::Async(e) => Some(&e.attrs),
        syn::Expr::Await(e) => Some(&e.attrs),
        syn::Expr::Binary(e) => Some(&e.attrs),
        syn::Expr::Block(e) => Some(&e.attrs),
        syn::Expr::Break(e) => Some(&e.attrs),
        syn::Expr::Call(e) => Some(&e.attrs),
        syn::Expr::Cast(e) => Some(&e.attrs),
        syn::Expr::Closure(e) => Some(&e.attrs),
        syn::Expr::Const(e) => Some(&e.attrs),
        syn::Expr::Continue(e) => Some(&e.attrs),
        syn::Expr::Field(e) => Some(&e.attrs),
        syn::Expr::ForLoop(e) => Some(&e.attrs),
        syn::Expr::Group(e) => Some(&e.attrs),
        syn::Expr::If(e) => Some(&e.attrs),
        syn::Expr::Index(e) => Some(&e.attrs),
        syn::Expr::Infer(e) => Some(&e.attrs),
        syn::Expr::Let(e) => Some(&e.attrs),
        syn::Expr::Lit(e) => Some(&e.attrs),
        syn::Expr::Loop(e) => Some(&e.attrs),
        syn::Expr::Macro(e) => Some(&e.attrs),
        syn::Expr::Match(e) => Some(&e.attrs),
        syn::Expr::MethodCall(e) => Some(&e.attrs),
        syn::Expr::Paren(e) => Some(&e.attrs),
        syn::Expr::Path(e) => Some(&e.attrs),
        syn::Expr::Range(e) => Some(&e.attrs),
        syn::Expr::Reference(e) => Some(&e.attrs),
        syn::Expr::Repeat(e) => Some(&e.attrs),
        syn::Expr::Return(e) => Some(&e.attrs),
        syn::Expr::Struct(e) => Some(&e.attrs),
        syn::Expr::Try(e) => Some(&e.attrs),
        syn::Expr::TryBlock(e) => Some(&e.attrs),
        syn::Expr::Tuple(e) => Some(&e.attrs),
        syn::Expr::Unary(e) => Some(&e.attrs),
        syn::Expr::Unsafe(e) => Some(&e.attrs),
        syn::Expr::While(e) => Some(&e.attrs),
        syn::Expr::Yield(e) => Some(&e.attrs),
        _ => None,
    }
}

struct AllowCollector<'a> {
    map: &'a mut AllowMap,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'ast, 'a> Visit<'ast> for AllowCollector<'a> {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        if let Some(attrs) = item_attrs(node) {
            process_attrs(attrs, span_range(node.span()), self.map, self.diagnostics);
        }
        visit::visit_item(self, node);
    }

    fn visit_impl_item(&mut self, node: &'ast syn::ImplItem) {
        let attrs: Option<&[syn::Attribute]> = match node {
            syn::ImplItem::Const(c) => Some(&c.attrs),
            syn::ImplItem::Fn(f) => Some(&f.attrs),
            syn::ImplItem::Type(t) => Some(&t.attrs),
            syn::ImplItem::Macro(m) => Some(&m.attrs),
            _ => None,
        };
        if let Some(a) = attrs {
            process_attrs(a, span_range(node.span()), self.map, self.diagnostics);
        }
        visit::visit_impl_item(self, node);
    }

    fn visit_trait_item(&mut self, node: &'ast syn::TraitItem) {
        let attrs: Option<&[syn::Attribute]> = match node {
            syn::TraitItem::Const(c) => Some(&c.attrs),
            syn::TraitItem::Fn(f) => Some(&f.attrs),
            syn::TraitItem::Type(t) => Some(&t.attrs),
            syn::TraitItem::Macro(m) => Some(&m.attrs),
            _ => None,
        };
        if let Some(a) = attrs {
            process_attrs(a, span_range(node.span()), self.map, self.diagnostics);
        }
        visit::visit_trait_item(self, node);
    }

    fn visit_stmt(&mut self, node: &'ast syn::Stmt) {
        if let Some(attrs) = stmt_attrs(node) {
            process_attrs(attrs, span_range(node.span()), self.map, self.diagnostics);
        }
        visit::visit_stmt(self, node);
    }
}

/// Collect every `#[allow(trust::...)]` scope in the file. Also emits
/// R0015 (missing reason) and R0016 (unknown code) diagnostics for malformed
/// allow attributes.
pub(crate) fn collect_allow_map(
    file: &syn::File,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> AllowMap {
    let mut map = AllowMap::default();
    // Crate-level `#![allow(...)]` covers the whole file.
    process_attrs(&file.attrs, 0..source.len(), &mut map, diagnostics);
    let mut v = AllowCollector {
        map: &mut map,
        diagnostics,
    };
    v.visit_file(file);
    map
}
