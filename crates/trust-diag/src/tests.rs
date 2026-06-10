//! Tests for `trust-diag`. Kept in a sibling file (not an inline
//! `mod tests {}`) so the `trust-rustc` wrapper — which lints only the crate
//! root — doesn't apply R0042 to the positional helper calls below.

use super::*;

#[test]
fn line_col_basic() {
    let src = "ab\ncde\nf";
    assert_eq!(line_col(src, 0), (1, 1));
    assert_eq!(line_col(src, 1), (1, 2));
    assert_eq!(line_col(src, 3), (2, 1)); // first char of line 2
    assert_eq!(line_col(src, 4), (2, 2));
    assert_eq!(line_col(src, 7), (3, 1)); // 'f'
                                          // Out-of-range clamps to end.
    assert_eq!(line_col(src, 999), (3, 2));
}

#[test]
fn line_col_counts_chars_not_bytes() {
    // 'é' is two UTF-8 bytes; the column after it should be 2, not 3.
    let src = "é=x";
    // byte offset of '=' is 2; it's the 2nd char on the line.
    assert_eq!(line_col(src, 2), (1, 2));
}

#[test]
fn json_escapes_strings() {
    let mut w = JsonWriter::new("");
    w.string("a\"b\\c\nd\te");
    assert_eq!(w.into_string(), "\"a\\\"b\\\\c\\nd\\te\"");
}

#[test]
fn json_empty_diagnostics() {
    let json = to_json(&[], "f.rs", "");
    assert!(json.contains("\"diagnostics\": []"));
    assert!(json.contains("\"file\": \"f.rs\""));
}

#[test]
fn json_full_diagnostic_with_fix() {
    let src = "fn main() {\n    let _ = area(1, 2);\n}\n";
    let span = 20..30;
    let diag = Diagnostic::error(
        "R0042",
        "call to `area` must use named arguments",
        span.clone(),
    )
    .with_why("positional ordering is the largest LLM bug class")
    .with_help("rewrite as `area(width: ..., height: ...)`")
    .with_fix(Fix::new(
        span,
        "area(width: ..., height: ...)",
        Applicability::HasPlaceholders,
    ));
    let json = to_json(std::slice::from_ref(&diag), "src/main.rs", src);

    // Spot-check the structured fields an agent would key on.
    assert!(json.contains("\"rule\": \"R0042\""));
    assert!(json.contains("\"severity\": \"error\""));
    assert!(json.contains("\"applicability\": \"hasPlaceholders\""));
    assert!(json.contains("\"replacement\": \"area(width: ..., height: ...)\""));
    assert!(json.contains("\"startLine\": 2"));
    assert!(json.contains("\"why\":"));
    assert!(json.contains("\"help\":"));
}

#[test]
fn json_null_fields_when_absent() {
    let diag = Diagnostic::warning("R0000", "x", 0..1);
    let json = to_json(std::slice::from_ref(&diag), "f.rs", "xy");
    assert!(json.contains("\"why\": null"));
    assert!(json.contains("\"help\": null"));
    assert!(json.contains("\"fix\": null"));
}

#[test]
fn applicability_tokens_are_stable() {
    assert_eq!(Applicability::Automatic.as_str(), "automatic");
    assert_eq!(Applicability::MaybeIncorrect.as_str(), "maybeIncorrect");
    assert_eq!(Applicability::HasPlaceholders.as_str(), "hasPlaceholders");
}
