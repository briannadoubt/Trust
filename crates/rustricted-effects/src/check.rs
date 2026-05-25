//! Effect inference + check.
//!
//! For each function with a declared effect set, walk its body, identify
//! direct calls (by simple name) to other functions and macros, accumulate
//! the union of their declared effects (from a seed table of `std` calls
//! plus the local crate table), and report any inferred effect missing
//! from the declared set as R4001.
//!
//! This is intentionally an intra-procedural, direct-call analysis — no
//! fixed-point closure, no aliasing. It's an honest prototype: covers the
//! common case (calling `println!` from a function that didn't declare
//! `io`) and stays predictable.

use crate::registry::{Effect, EffectSet, EffectTable};
use rustricted_diag::Diagnostic;
use std::collections::BTreeSet;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

#[derive(Debug, Default)]
pub struct EffectCheck {
    pub diagnostics: Vec<Diagnostic>,
}

/// Seeded effect declarations for stdlib calls + commonly-used macros.
/// Keyed by the simple name (last path segment, or macro name).
pub fn std_seed() -> EffectTable {
    let mut t = EffectTable::new();
    let io = EffectSet::from_names(["io"]);

    for name in [
        "println", "eprintln", "print", "eprint", "write", "writeln", "dbg",
    ] {
        t.insert(name, io.clone());
    }
    for name in [
        "read_to_string",
        "write_text",
        "write",
        "read",
        "create",
        "open",
    ] {
        t.insert(name, io.clone());
    }

    t
}

/// Run the effect check across every fn in `file`. `declared` is the table
/// produced by [`crate::strip_effect_annotations`] for this crate.
pub fn check(file: &syn::File, declared: &EffectTable) -> EffectCheck {
    let seed = std_seed();
    let mut visitor = EffectVisitor {
        declared,
        seed: &seed,
        diagnostics: Vec::new(),
    };
    visitor.visit_file(file);
    EffectCheck {
        diagnostics: visitor.diagnostics,
    }
}

struct EffectVisitor<'a> {
    declared: &'a EffectTable,
    seed: &'a EffectTable,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> EffectVisitor<'a> {
    fn lookup(&self, name: &str) -> Option<&'a EffectSet> {
        self.declared.get(name).or_else(|| self.seed.get(name))
    }

    fn check_fn(&mut self, fn_name: &str, body: &syn::Block) {
        let Some(declared) = self.declared.get(fn_name) else {
            return; // function has no declared effects — nothing to check
        };
        let mut inferred = EffectSet::default();
        let mut call_collector = CallCollector::default();
        call_collector.visit_block(body);
        for callee in &call_collector.callees {
            if let Some(effects) = self.lookup(callee) {
                inferred.union_with(effects);
            }
        }
        let missing: BTreeSet<Effect> = inferred.0.difference(&declared.0).cloned().collect();
        if !missing.is_empty() {
            let missing_str: Vec<&str> = missing.iter().map(|e| e.0.as_str()).collect();
            let diag = Diagnostic::error(
                "R4001",
                format!(
                    "`{fn_name}` is missing declared effect(s): {}",
                    missing_str.join(" + ")
                ),
                body.span().byte_range(),
            )
            .with_why(
                "every function must declare an effect set that is a superset of the effects it actually invokes"
                    .to_string(),
            )
            .with_help(format!(
                "add `effect {}` to the signature",
                missing_str.join(" + ")
            ));
            self.diagnostics.push(diag);
        }
    }
}

impl<'ast, 'a> Visit<'ast> for EffectVisitor<'a> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.check_fn(&node.sig.ident.to_string(), &node.block);
        visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.check_fn(&node.sig.ident.to_string(), &node.block);
        visit::visit_impl_item_fn(self, node);
    }
}

#[derive(Default)]
struct CallCollector {
    callees: Vec<String>,
}

impl<'ast> Visit<'ast> for CallCollector {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = node.func.as_ref() {
            if let Some(seg) = p.path.segments.last() {
                self.callees.push(seg.ident.to_string());
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.callees.push(node.method.to_string());
        visit::visit_expr_method_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if let Some(seg) = node.path.segments.last() {
            self.callees.push(seg.ident.to_string());
        }
        visit::visit_macro(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::EffectSet;

    fn parse(src: &str) -> syn::File {
        syn::parse_str(src).expect("test src must parse")
    }

    fn declared(pairs: &[(&str, &[&str])]) -> EffectTable {
        let mut t = EffectTable::new();
        for (name, effects) in pairs {
            t.insert(*name, EffectSet::from_names(effects.iter().copied()));
        }
        t
    }

    #[test]
    fn missing_io_effect_is_flagged() {
        let src = "fn f() {} fn naughty() { println!(\"hi\"); }";
        let file = parse(src);
        let table = declared(&[("naughty", &[])]);
        let report = check(&file, &table);
        assert!(report.diagnostics.iter().any(|d| d.rule == "R4001"));
    }

    #[test]
    fn declared_io_silences_check() {
        let src = "fn announce() { println!(\"hi\"); }";
        let file = parse(src);
        let table = declared(&[("announce", &["io"])]);
        let report = check(&file, &table);
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    }

    #[test]
    fn no_declaration_means_no_check() {
        // Functions without declared effects aren't audited (they're just
        // not in the table). Phase 4 prototype scope: enforcement only for
        // explicitly-annotated functions.
        let src = "fn just_does_io() { println!(\"hi\"); }";
        let file = parse(src);
        let table = declared(&[]);
        let report = check(&file, &table);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn calling_local_effectful_fn_propagates() {
        let src = "fn writer() { println!(\"hi\"); } fn caller() { writer(); }";
        let file = parse(src);
        let table = declared(&[("writer", &["io"]), ("caller", &[])]);
        let report = check(&file, &table);
        assert!(
            report.diagnostics.iter().any(|d| d.rule == "R4001"),
            "caller should be flagged: {:?}",
            report.diagnostics
        );
    }
}
