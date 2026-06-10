//! RT-91 integration tests: the comment-window rules (R0005/R0006) must
//! find justifications in the ORIGINAL source even when the linted AST
//! comes from the lowered output (whose offsets drift because prettyplease
//! strips comments), and a justification longer than the legacy 200-byte
//! window must still count.

fn lint_through_pipeline(original: &str) -> Vec<String> {
    let out = trust_lower::lower_with_extra_callees_forced(original, &[], true)
        .expect("lowering should succeed");
    let file: syn::File = syn::parse_str(&out.lint_source).expect("lint_source parses");
    trust_lints::lint_strict(&file, original, true)
        .diagnostics
        .into_iter()
        .map(|d| d.rule.to_string())
        .collect()
}

#[test]
fn multi_line_reason_block_satisfies_justify_allow() {
    // The marker line sits more than 200 bytes above the attribute — the
    // legacy window alone can't see it (heck-strict's shouty_snake.rs).
    let src = "\
pub trait Loud: ToOwned {
    /// CONVERT THIS TYPE.
    // reason: the method name INTENTIONALLY_SCREAMS to match the output
    // convention of the case conversion this trait performs; the unusual
    // casing is part of the public API contract and renaming it would be
    // a semver-breaking change for every downstream consumer of the trait.
    #[allow(non_snake_case)]
    fn INTENTIONALLY_SCREAMS(&self) -> Self::Owned;
}
fn main() {}
";
    let rules = lint_through_pipeline(src);
    assert!(
        !rules.iter().any(|r| r == "R0006"),
        "multi-line reason block must justify the allow: {rules:?}"
    );
}

#[test]
fn unjustified_allow_still_fires_through_pipeline() {
    let src = "#[allow(dead_code)]\nfn quiet() {}\nfn main() {}\n";
    let rules = lint_through_pipeline(src);
    assert!(
        rules.iter().any(|r| r == "R0006"),
        "bare allow must fire: {rules:?}"
    );
}

#[test]
fn named_arg_syntax_does_not_break_site_discovery() {
    // The original parses only as TOKENS (named args are invalid syn) —
    // exactly the wrapper's situation. Justified unsafe + allow must pass.
    let src = "\
fn make_rect(width: u32, height: u32) -> u32 { width * height }
// reason: benchmark scaffolding, not part of the shipped library
#[allow(dead_code)]
fn bench() -> u32 { make_rect(width: 2, height: 3) }
// safety: the pointer is the address of a live local, checked above
fn touch() { unsafe { core::ptr::null::<u8>().read_volatile(); } }
fn main() {}
";
    let rules = lint_through_pipeline(src);
    assert!(
        !rules.iter().any(|r| r == "R0005" || r == "R0006"),
        "justified sites must pass through the lowering pipeline: {rules:?}"
    );
}
