//! Lint runner: drives visitors over a parsed `syn::File` and collects
//! diagnostics. Phase 0 stub returns an empty report; Phase 1 subagent
//! fills in `visit::Visit` impls per rule.
//!
//! Intentionally not `#![strict]`-marked: calls cross-file fns
//! `crate::strict::run_rule` and `crate::strict::detect_strict` whose
//! signatures aren't visible to this file's per-file callee registry
//! (RT-40). Without cross-file resolution, named-arg lowering can't strip
//! the names, and rustc rejects them.

use crate::strict::detect_strict;
use crate::Rule;
use rustricted_diag::Diagnostic;

#[derive(Debug, Default)]
pub struct LintReport {
    pub diagnostics: Vec<Diagnostic>,
    pub strict_mode: bool,
}

impl LintReport {
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.is_error()).count()
    }
}

/// Run every rule against `file`, auto-detecting strict mode from the file's
/// inner attributes. `source` is the original source text, used by individual
/// rules to extract justification comments and to produce `ariadne` spans.
pub fn lint(file: &syn::File, source: &str) -> LintReport {
    lint_with(file, source, crate::all_rules())
}

/// Like [`lint`] but with an explicit strict-mode flag. Use this when the
/// driver has already detected `#![strict]` at the token level (e.g. before
/// the attribute was stripped during lowering).
pub fn lint_strict(file: &syn::File, source: &str, strict_mode: bool) -> LintReport {
    run(file, source, crate::all_rules(), strict_mode)
}

/// Run only the given subset of rules. Useful for tests and for crates
/// that opt into stricter-than-default subsets.
pub fn lint_with(file: &syn::File, source: &str, rules: Vec<Rule>) -> LintReport {
    let strict_mode = detect_strict(file);
    run(file, source, rules, strict_mode)
}

fn run(file: &syn::File, source: &str, rules: Vec<Rule>, strict_mode: bool) -> LintReport {
    let mut report = LintReport {
        diagnostics: Vec::new(),
        strict_mode,
    };

    if !strict_mode {
        return report;
    }

    // First pass: collect every `#[allow(rustricted::...)]` scope, and emit
    // R0015 / R0016 diagnostics for malformed allow attributes. A malformed
    // allow does NOT suppress, so the rules it lists keep firing below.
    let allow_map = crate::allow::collect_allow_map(file, source, &mut report.diagnostics);

    for rule in &rules {
        crate::strict::run_rule(*rule, file, source, &mut report.diagnostics);
    }

    // Drop diagnostics suppressed by an enclosing `#[allow(rustricted::Rxxxx,
    // reason = "...")]`. R0015 / R0016 themselves cannot be suppressed —
    // that would let a malformed allow silence its own validation diag.
    report.diagnostics.retain(|d| {
        if d.rule == Rule::AllowMissingReason.code() || d.rule == Rule::AllowUnknownCode.code() {
            return true;
        }
        let Some(rule) = Rule::from_code(d.rule) else {
            return true;
        };
        !allow_map.is_suppressed(rule, &d.span)
    });

    report
}
