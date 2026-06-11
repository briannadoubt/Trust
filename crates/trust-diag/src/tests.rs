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
    let json = to_json(
        &[],
        NamedSource {
            name: "f.rs",
            text: "",
        },
    );
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
    let json = to_json(
        std::slice::from_ref(&diag),
        NamedSource {
            name: "src/main.rs",
            text: src,
        },
    );

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
    let json = to_json(
        std::slice::from_ref(&diag),
        NamedSource {
            name: "f.rs",
            text: "xy",
        },
    );
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

// RT-107: SARIF 2.1.0 for GitHub code-scanning.
#[test]
fn sarif_emits_valid_shape() {
    let src = "fn f() { let x = a.unwrap(); }\n";
    let diag = Diagnostic::error("R0001", "`.unwrap()` is banned", 17..23).with_why("panics");
    let sarif = to_sarif(&[FileDiagnostics {
        name: "src/a.rs",
        text: src,
        diagnostics: std::slice::from_ref(&diag),
    }]);
    assert!(sarif.contains("\"version\": \"2.1.0\""));
    assert!(sarif.contains("\"name\": \"trust\""));
    assert!(sarif.contains("\"ruleId\": \"R0001\""));
    assert!(sarif.contains("\"level\": \"error\""));
    assert!(sarif.contains("\"uri\": \"src/a.rs\""));
    assert!(sarif.contains("\"startLine\": 1"));
    // Distinct rule is listed in driver.rules carrying its `why`.
    assert!(sarif.contains("\"id\": \"R0001\""));
    assert!(sarif.contains("panics"));
}

#[test]
fn sarif_aggregates_multiple_files_in_one_run() {
    let a = Diagnostic::error("R0001", "x", 0..1);
    let b = Diagnostic::warning("R0003", "y", 0..1);
    let sarif = to_sarif(&[
        FileDiagnostics {
            name: "a.rs",
            text: "ab",
            diagnostics: std::slice::from_ref(&a),
        },
        FileDiagnostics {
            name: "b.rs",
            text: "cd",
            diagnostics: std::slice::from_ref(&b),
        },
    ]);
    // Exactly one run; both files' findings present with their level.
    assert_eq!(sarif.matches("\"runs\":").count(), 1);
    assert!(sarif.contains("\"uri\": \"a.rs\""));
    assert!(sarif.contains("\"uri\": \"b.rs\""));
    assert!(sarif.contains("\"level\": \"warning\""));
}

#[test]
fn sarif_empty_is_well_formed() {
    let sarif = to_sarif(&[FileDiagnostics {
        name: "a.rs",
        text: "",
        diagnostics: &[],
    }]);
    assert!(sarif.contains("\"results\": []"));
    assert!(sarif.contains("\"rules\": []"));
}
