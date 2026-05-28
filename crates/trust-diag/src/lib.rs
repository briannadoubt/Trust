//! Teaching diagnostics for Trust.
//!
//! Each diagnostic carries a stable rule code, a one-sentence rationale,
//! and (where possible) a literal code-fragment suggestion. The renderer
//! formats them via `ariadne` so callers see file/line context with the
//! help text inline.

trust_attrs::strict! {}

use std::ops::Range;

pub use ariadne;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Stable rule code, e.g. `"R0001"`.
    pub rule: &'static str,
    /// Severity.
    pub severity: Severity,
    /// Primary message (what the author did wrong, in one short sentence).
    pub message: String,
    /// Byte range in the source file the diagnostic refers to.
    pub span: Range<usize>,
    /// One-sentence explanation of why the rule exists.
    pub why: Option<String>,
    /// Suggested replacement (or other actionable hint).
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(rule: &'static str, message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            rule,
            severity: Severity::Error,
            message: message.into(),
            span,
            why: None,
            help: None,
        }
    }

    pub fn warning(rule: &'static str, message: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            rule,
            severity: Severity::Warning,
            message: message.into(),
            span,
            why: None,
            help: None,
        }
    }

    pub fn with_why(mut self, why: impl Into<String>) -> Self {
        self.why = Some(why.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

/// Render diagnostics to a writer using `ariadne`. `filename` is shown in
/// the source-position banner.
pub fn render<W: std::io::Write>(
    diagnostics: &[Diagnostic],
    filename: &str,
    source: &str,
    writer: &mut W,
) -> std::io::Result<()> {
    use ariadne::{Color, Label, Report, ReportKind, Source};

    for diag in diagnostics {
        let kind = match diag.severity {
            Severity::Error => ReportKind::Error,
            Severity::Warning => ReportKind::Warning,
        };

        let mut report = Report::build(kind, filename, diag.span.start)
            .with_code(diag.rule)
            .with_message(&diag.message);

        let label_color = match diag.severity {
            Severity::Error => Color::Red,
            Severity::Warning => Color::Yellow,
        };

        report = report.with_label(
            Label::new((filename, diag.span.clone()))
                .with_color(label_color)
                .with_message(&diag.message),
        );

        if let Some(why) = &diag.why {
            report = report.with_note(format!("why: {why}"));
        }
        if let Some(help) = &diag.help {
            report = report.with_help(help.clone());
        }

        report
            .finish()
            .write((filename, Source::from(source)), &mut *writer)?;
    }

    Ok(())
}
