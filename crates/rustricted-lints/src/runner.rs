//! Lint runner: drives visitors over a parsed `syn::File` and collects
//! diagnostics. Phase 0 stub returns an empty report; Phase 1 subagent
//! fills in `visit::Visit` impls per rule.

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

    for rule in rules {
        crate::strict::run_rule(rule, file, source, &mut report.diagnostics);
    }

    report
}
