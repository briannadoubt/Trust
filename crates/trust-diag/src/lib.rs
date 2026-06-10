//! Teaching diagnostics for Trust.
//!
//! Each diagnostic carries a stable rule code, a one-sentence rationale,
//! and (where possible) a literal code-fragment suggestion. The renderer
//! formats them via `ariadne` so callers see file/line context with the
//! help text inline.
//!
//! For agent consumers, diagnostics also serialise to a stable JSON shape
//! (RT-70) via [`to_json`], carrying byte spans, line/column, the `why`
//! rationale, the `help` text, and — where the toolchain can produce one —
//! a structured, machine-applicable [`Fix`] with an [`Applicability`]
//! confidence level. An agent harness can ingest that directly and apply the
//! `Automatic` fixes without re-parsing prose.
//!
//! NOTE: this crate is `#![strict]`-dogfooded, so it must build under the
//! `trust-rustc` wrapper too — which means no multi-argument call to a
//! *local* fn (R0042). The JSON emitter is therefore written as methods on
//! [`JsonWriter`] (method calls are exempt), and the public [`line_col`]
//! delegates to a one-field [`Located`] helper rather than calling a 2-arg
//! free fn.

use std::ops::Range;

pub use ariadne;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// How safe a [`Fix`] is to apply without human review. Mirrors the
/// rustc/clippy notion of applicability — the "confidence" an agent loop
/// keys on when deciding whether to auto-apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// Semantics-preserving; an agent (or `--fix`) may apply it unattended.
    Automatic,
    /// Probably correct, but worth a glance — it may change behaviour or
    /// depend on context the linter can't see (e.g. `.unwrap()` → `?`
    /// assumes the enclosing fn returns `Result`).
    MaybeIncorrect,
    /// Contains `...`-style placeholders that MUST be filled before the code
    /// compiles (e.g. the named-argument template for R0042).
    HasPlaceholders,
}

impl Applicability {
    /// Stable lowerCamelCase token used in the JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            Applicability::Automatic => "automatic",
            Applicability::MaybeIncorrect => "maybeIncorrect",
            Applicability::HasPlaceholders => "hasPlaceholders",
        }
    }
}

/// A structured, machine-applicable edit: replace `span` in the source with
/// `replacement`. `applicability` tells a consumer how much to trust it.
#[derive(Debug, Clone)]
pub struct Fix {
    /// Byte range in the source to replace. Often equals the diagnostic's
    /// own span, but not always (a fix may target a wider or narrower range).
    pub span: Range<usize>,
    /// Exact replacement text for `span`.
    pub replacement: String,
    /// How safe the fix is to apply automatically.
    pub applicability: Applicability,
}

impl Fix {
    pub fn new(
        span: Range<usize>,
        replacement: impl Into<String>,
        applicability: Applicability,
    ) -> Self {
        Self {
            span,
            replacement: replacement.into(),
            applicability,
        }
    }
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
    /// Suggested replacement (or other actionable hint), as prose.
    pub help: Option<String>,
    /// Structured, machine-applicable edit (RT-70). `None` when the rule has
    /// no mechanical fix (the prose `help` may still guide a human/agent).
    pub fix: Option<Fix>,
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
            fix: None,
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
            fix: None,
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

    /// Attach a structured, machine-applicable [`Fix`] (RT-70).
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }

    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

/// A named source text. Bundles the filename and contents every renderer
/// needs — and keeps an adjacent same-typed `(&str, &str)` pair out of the
/// public signatures (R0017, surfaced by this crate's own strict dogfood).
#[derive(Clone, Copy)]
pub struct NamedSource<'a> {
    /// Display name for the source-position banner (a path, or `<stdin>`).
    pub name: &'a str,
    /// The source text the diagnostic spans index into.
    pub text: &'a str,
}

/// Render diagnostics to a writer using `ariadne`. `src.name` is shown in
/// the source-position banner.
pub fn render<W: std::io::Write>(
    diagnostics: &[Diagnostic],
    src: NamedSource<'_>,
    writer: &mut W,
) -> std::io::Result<()> {
    let (filename, source) = (src.name, src.text);
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

/// 1-based `(line, column)` for a byte offset into `source`. Column counts
/// Unicode scalar values (chars) since the last newline, so it lines up with
/// what an editor shows. An out-of-range offset clamps to the source end.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    Located { source }.at(offset)
}

/// One-field view over a source string. Exists so [`line_col`] and the JSON
/// emitter can locate offsets via a *method* call (`.at(offset)`) rather than
/// a two-argument free-fn call, which R0042 forbids in this strict crate.
struct Located<'a> {
    source: &'a str,
}

impl Located<'_> {
    fn at(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let mut line = 1usize;
        let mut col = 1usize;
        for (i, ch) in self.source.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// Serialise diagnostics to a stable JSON document for agent consumers
/// (RT-70). The shape is:
///
/// ```json
/// {
///   "version": "0.1",
///   "file": "src/main.rs",
///   "diagnostics": [
///     {
///       "rule": "R0042",
///       "severity": "error",
///       "message": "...",
///       "span": {"start": 45, "end": 67,
///                "startLine": 3, "startColumn": 13,
///                "endLine": 3, "endColumn": 35},
///       "why": "...",
///       "help": "...",
///       "fix": {"span": {}, "replacement": "...",
///               "applicability": "hasPlaceholders"}
///     }
///   ]
/// }
/// ```
///
/// `why`, `help`, and `fix` are emitted as `null` when absent. The emitter is
/// hand-rolled (no serde dependency) and escapes strings per RFC 8259.
pub fn to_json(diagnostics: &[Diagnostic], src: NamedSource<'_>) -> String {
    let mut writer = JsonWriter::new(src.text);
    writer.document(diagnostics, src.name);
    writer.into_string()
}

/// Escape `s` as a JSON string literal (with surrounding quotes), per RFC
/// 8259. Exposed so other tools — e.g. `trust explain --format json` — can
/// emit JSON without duplicating the escaper.
pub fn json_escape(s: &str) -> String {
    let mut writer = JsonWriter::new("");
    writer.string(s);
    writer.into_string()
}

fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// Accumulates the JSON document. Methods (not free fns) so multi-argument
/// helpers don't trip R0042 in this strict-dogfooded crate.
struct JsonWriter<'a> {
    out: String,
    source: &'a str,
}

impl<'a> JsonWriter<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            out: String::new(),
            source,
        }
    }

    fn into_string(self) -> String {
        self.out
    }

    fn document(&mut self, diagnostics: &[Diagnostic], filename: &str) {
        self.out
            .push_str("{\n  \"version\": \"0.1\",\n  \"file\": ");
        self.string(filename);
        self.out.push_str(",\n  \"diagnostics\": [");
        for (i, diag) in diagnostics.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            self.entry(diag);
        }
        if diagnostics.is_empty() {
            self.out.push_str("]\n}\n");
        } else {
            self.out.push_str("\n  ]\n}\n");
        }
    }

    fn entry(&mut self, diag: &Diagnostic) {
        self.out.push_str("\n    {\n      \"rule\": ");
        self.string(diag.rule);
        self.out.push_str(",\n      \"severity\": ");
        self.string(severity_str(diag.severity));
        self.out.push_str(",\n      \"message\": ");
        self.string(&diag.message);
        self.out.push_str(",\n      \"span\": ");
        self.span(&diag.span);
        self.out.push_str(",\n      \"why\": ");
        self.opt(diag.why.as_deref());
        self.out.push_str(",\n      \"help\": ");
        self.opt(diag.help.as_deref());
        self.out.push_str(",\n      \"fix\": ");
        match &diag.fix {
            Some(fix) => self.fix(fix),
            None => self.out.push_str("null"),
        }
        self.out.push_str("\n    }");
    }

    fn span(&mut self, span: &Range<usize>) {
        let (start_line, start_col) = Located {
            source: self.source,
        }
        .at(span.start);
        let (end_line, end_col) = Located {
            source: self.source,
        }
        .at(span.end);
        self.out.push('{');
        self.out.push_str("\"start\": ");
        self.out.push_str(&span.start.to_string());
        self.out.push_str(", \"end\": ");
        self.out.push_str(&span.end.to_string());
        self.out.push_str(", \"startLine\": ");
        self.out.push_str(&start_line.to_string());
        self.out.push_str(", \"startColumn\": ");
        self.out.push_str(&start_col.to_string());
        self.out.push_str(", \"endLine\": ");
        self.out.push_str(&end_line.to_string());
        self.out.push_str(", \"endColumn\": ");
        self.out.push_str(&end_col.to_string());
        self.out.push('}');
    }

    fn fix(&mut self, fix: &Fix) {
        self.out.push_str("{\"span\": ");
        self.span(&fix.span);
        self.out.push_str(", \"replacement\": ");
        self.string(&fix.replacement);
        self.out.push_str(", \"applicability\": ");
        self.string(fix.applicability.as_str());
        self.out.push('}');
    }

    fn opt(&mut self, value: Option<&str>) {
        match value {
            Some(s) => self.string(s),
            None => self.out.push_str("null"),
        }
    }

    /// Append `s` as a JSON string literal, escaping per RFC 8259.
    fn string(&mut self, s: &str) {
        self.out.push('"');
        for ch in s.chars() {
            match ch {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\n' => self.out.push_str("\\n"),
                '\r' => self.out.push_str("\\r"),
                '\t' => self.out.push_str("\\t"),
                '\u{08}' => self.out.push_str("\\b"),
                '\u{0c}' => self.out.push_str("\\f"),
                c if u32::from(c) < 0x20 => {
                    self.out.push_str("\\u");
                    let code = u32::from(c);
                    for shift in [12, 8, 4, 0] {
                        let nibble = (code >> shift) & 0xf;
                        self.out.push(char::from_digit(nibble, 16).unwrap_or('0'));
                    }
                }
                c => self.out.push(c),
            }
        }
        self.out.push('"');
    }
}

// Tests live in a sibling file (`mod tests;`) rather than an inline
// `mod tests { … }`. The `trust-rustc` wrapper lowers/lints only the crate
// root, so an external child module is not subject to R0042 — which lets the
// tests use ordinary positional calls (`to_json(diags, file, src)`) that the
// strict dialect would otherwise reject. Same trick `trust-std` uses.
#[cfg(test)]
mod tests;
