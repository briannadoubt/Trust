//! Strict-mode lint preset for Rustricted.
//!
//! Each lint is a visitor over the parsed Rust AST that emits a teaching
//! diagnostic for a specific footgun. Activated by `#![strict]` at the
//! crate root; individual lints can be silenced with `#[allow(...)]` if
//! accompanied by a `// reason:` justification comment.

mod rules;
mod runner;
mod strict;

pub use rules::Rule;
pub use runner::{lint, lint_strict, lint_with, LintReport};
pub use rustricted_diag::Diagnostic;

/// Returns the full Rustricted strict-mode lint set.
pub fn all_rules() -> Vec<Rule> {
    rules::ALL.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> syn::File {
        syn::parse_str(src).expect("test source must parse as Rust")
    }

    fn diags(src: &str) -> Vec<Diagnostic> {
        lint(&parse(src), src).diagnostics
    }

    fn diags_for(rule: Rule, src: &str) -> Vec<Diagnostic> {
        lint_with(&parse(src), src, vec![rule]).diagnostics
    }

    fn fires(rule: Rule, src: &str) -> bool {
        diags_for(rule, src).iter().any(|d| d.rule == rule.code())
    }

    #[test]
    fn clean_program_has_no_diagnostics() {
        let src = "#![strict]\nfn main() { let x: u32 = 1; println!(\"{x}\"); }";
        let d = diags(src);
        assert!(d.is_empty(), "expected no diagnostics, got {d:?}");
    }

    #[test]
    fn non_strict_program_skipped_even_with_violations() {
        let src = "fn main() { let x: Option<u32> = None; x.unwrap(); let n = 1u32 as u8; }";
        let d = diags(src);
        assert!(
            d.is_empty(),
            "non-strict programs must be silent, got {d:?}"
        );
    }

    #[test]
    fn report_records_strict_mode_flag() {
        let strict = "#![strict]\nfn main() {}";
        let plain = "fn main() {}";
        assert!(lint(&parse(strict), strict).strict_mode);
        assert!(!lint(&parse(plain), plain).strict_mode);
    }

    #[test]
    fn r0001_fires_on_unwrap() {
        let src = "#![strict]\nfn f() { let x: Option<u32> = None; x.unwrap(); }";
        assert!(fires(Rule::NoUnwrap, src));
    }

    #[test]
    fn r0001_allows_unwrap_in_cfg_test_fn() {
        let src = "#![strict]\n#[cfg(test)]\nfn t() { let x: Option<u32> = None; x.unwrap(); }";
        let d = diags_for(Rule::NoUnwrap, src);
        assert!(d.is_empty(), "expected no R0001 diag, got {d:?}");
    }

    #[test]
    fn r0001_allows_unwrap_in_test_fn() {
        let src = "#![strict]\n#[test]\nfn t() { let x: Option<u32> = None; x.unwrap(); }";
        let d = diags_for(Rule::NoUnwrap, src);
        assert!(d.is_empty(), "expected no R0001 diag, got {d:?}");
    }

    #[test]
    fn r0001_allows_unwrap_in_cfg_test_mod() {
        let src =
            "#![strict]\n#[cfg(test)]\nmod m { fn t() { let x: Option<u32> = None; x.unwrap(); } }";
        let d = diags_for(Rule::NoUnwrap, src);
        assert!(d.is_empty(), "expected no R0001 diag, got {d:?}");
    }

    #[test]
    fn r0001_silent_when_unwrap_has_arg() {
        let src = "#![strict]\nfn f() { let x: Option<u32> = None; x.unwrap_or(0); }";
        let d = diags_for(Rule::NoUnwrap, src);
        assert!(d.is_empty(), "expected no R0001 diag, got {d:?}");
    }

    #[test]
    fn r0002_fires_on_empty_expect() {
        let src = "#![strict]\nfn f() { let x: Option<u32> = None; x.expect(\"\"); }";
        assert!(fires(Rule::EmptyExpect, src));
    }

    #[test]
    fn r0002_silent_on_nonempty_expect() {
        let src = "#![strict]\nfn f() { let x: Option<u32> = None; x.expect(\"must exist\"); }";
        let d = diags_for(Rule::EmptyExpect, src);
        assert!(d.is_empty(), "expected no R0002 diag, got {d:?}");
    }

    #[test]
    fn r0003_fires_on_as_cast() {
        let src = "#![strict]\nfn f() { let n: u32 = 1; let _ = n as u8; }";
        assert!(fires(Rule::NoAsCast, src));
    }

    #[test]
    fn r0003_silent_without_as_cast() {
        let src =
            "#![strict]\nfn f() { let n: u32 = 1; let _: u8 = u8::try_from(n).unwrap_or(0); }";
        let d = diags_for(Rule::NoAsCast, src);
        assert!(d.is_empty(), "expected no R0003 diag, got {d:?}");
    }

    #[test]
    fn r0004_fires_on_glob_import() {
        let src = "#![strict]\nuse std::collections::*;\nfn f() {}";
        assert!(fires(Rule::NoGlobImport, src));
    }

    #[test]
    fn r0004_fires_on_nested_glob() {
        let src = "#![strict]\nuse std::{collections::*, fmt};\nfn f() {}";
        assert!(fires(Rule::NoGlobImport, src));
    }

    #[test]
    fn r0004_silent_on_explicit_import() {
        let src = "#![strict]\nuse std::collections::HashMap;\nfn f() {}";
        let d = diags_for(Rule::NoGlobImport, src);
        assert!(d.is_empty(), "expected no R0004 diag, got {d:?}");
    }

    #[test]
    fn r0004_silent_on_super_glob_in_cfg_test_mod() {
        let src = "#![strict]\nfn production() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn it_works() {}\n}";
        let d = diags_for(Rule::NoGlobImport, src);
        assert!(
            d.is_empty(),
            "expected no R0004 for use super::* in cfg(test), got {d:?}"
        );
    }

    #[test]
    fn r0004_still_fires_outside_cfg_test() {
        let src = "#![strict]\nuse super::*;\nfn f() {}";
        assert!(fires(Rule::NoGlobImport, src));
    }

    #[test]
    fn r0005_fires_on_unjustified_unsafe_block() {
        let src = "#![strict]\nfn f() { unsafe { let _ = 1; } }";
        assert!(fires(Rule::JustifyUnsafe, src));
    }

    #[test]
    fn r0005_silent_with_safety_comment() {
        let src = "#![strict]\nfn f() {\n    // safety: this is a no-op in tests\n    unsafe { let _ = 1; }\n}";
        let d = diags_for(Rule::JustifyUnsafe, src);
        assert!(d.is_empty(), "expected no R0005 diag, got {d:?}");
    }

    #[test]
    fn r0005_fires_on_unjustified_unsafe_fn() {
        let src = "#![strict]\nunsafe fn f() {}";
        assert!(fires(Rule::JustifyUnsafe, src));
    }

    #[test]
    fn r0005_silent_on_justified_unsafe_fn() {
        let src = "#![strict]\n// safety: pointer is checked by caller\nunsafe fn f() {}";
        let d = diags_for(Rule::JustifyUnsafe, src);
        assert!(d.is_empty(), "expected no R0005 diag, got {d:?}");
    }

    #[test]
    fn r0005_silent_on_doc_comment_safety_in_unsafe_fn() {
        // anyhow-style: Safety: paragraph lives in the doc comment, not an
        // inline block comment within 200 bytes of the unsafe keyword.
        let src = "#![strict]\n/// Does something.\n///\n/// # Safety\n///\n/// The pointer must be valid.\nunsafe fn f() {}";
        let d = diags_for(Rule::JustifyUnsafe, src);
        assert!(
            d.is_empty(),
            "expected no R0005 for Safety: in doc comment, got {d:?}"
        );
    }

    #[test]
    fn r0006_fires_on_unjustified_allow() {
        let src = "#![strict]\n#[allow(dead_code)]\nfn f() {}";
        assert!(fires(Rule::JustifyAllow, src));
    }

    #[test]
    fn r0006_silent_with_reason_comment() {
        let src = "#![strict]\n// reason: kept for future use\n#[allow(dead_code)]\nfn f() {}";
        let d = diags_for(Rule::JustifyAllow, src);
        assert!(d.is_empty(), "expected no R0006 diag, got {d:?}");
    }

    #[test]
    fn r0008_fires_on_macro_rules() {
        let src = "#![strict]\nmacro_rules! m { () => {} }\nfn f() {}";
        assert!(fires(Rule::NoUserMacros, src));
    }

    #[test]
    fn r0008_silent_with_macros_ok_on_item() {
        let src = "#![strict]\n#[strict::macros_ok]\nmacro_rules! m { () => {} }\nfn f() {}";
        let d = diags_for(Rule::NoUserMacros, src);
        assert!(d.is_empty(), "expected no R0008 diag, got {d:?}");
    }

    #[test]
    fn r0008_silent_with_macros_ok_on_enclosing_mod() {
        let src =
            "#![strict]\n#[strict::macros_ok]\nmod m { macro_rules! m { () => {} } }\nfn f() {}";
        let d = diags_for(Rule::NoUserMacros, src);
        assert!(d.is_empty(), "expected no R0008 diag, got {d:?}");
    }

    #[test]
    fn r0008_silent_on_builtin_macro_invocation() {
        let src = "#![strict]\nfn f() { println!(\"hi\"); }";
        let d = diags_for(Rule::NoUserMacros, src);
        assert!(d.is_empty(), "expected no R0008 diag, got {d:?}");
    }

    #[test]
    fn r0008_silent_in_cfg_test_mod() {
        // Heck-style: test helper macros inside #[cfg(test)] mod must not fire R0008.
        let src = "#![strict]\n#[cfg(test)]\nmod tests {\n    macro_rules! t {\n        ($s:expr) => { $s.to_string() }\n    }\n    #[test]\n    fn it_works() { assert_eq!(t!(\"hi\"), \"hi\"); }\n}";
        let d = diags_for(Rule::NoUserMacros, src);
        assert!(d.is_empty(), "expected no R0008 in cfg(test), got {d:?}");
    }

    #[test]
    fn r0007_fires_on_impl_trait_return() {
        let src = "#![strict]\nfn xs() -> impl Iterator<Item = u32> { [1u32].into_iter() }";
        assert!(fires(Rule::NoImplTraitReturn, src));
    }

    #[test]
    fn r0007_silent_on_named_return_type() {
        let src = "#![strict]\nfn xs() -> Vec<u32> { vec![1] }";
        let d = diags_for(Rule::NoImplTraitReturn, src);
        assert!(d.is_empty(), "expected no R0007 diag, got {d:?}");
    }

    #[test]
    fn r0007_silent_on_arg_position_impl_trait() {
        let src = "#![strict]\nfn xs(it: impl Iterator<Item = u32>) -> u32 { it.sum() }";
        let d = diags_for(Rule::NoImplTraitReturn, src);
        assert!(d.is_empty(), "expected no R0007 diag, got {d:?}");
    }

    #[test]
    fn r0010_fires_on_todo() {
        let src = "#![strict]\nfn f() -> u32 { todo!() }";
        let d = diags_for(Rule::NoTodoMacro, src);
        assert!(
            d.iter().any(|x| x.rule == "R0010"),
            "expected R0010 emission, got {d:?}"
        );
    }

    #[test]
    fn r0010_fires_on_unimplemented() {
        let src = "#![strict]\nfn f() -> u32 { unimplemented!() }";
        let d = diags_for(Rule::NoTodoMacro, src);
        assert!(
            d.iter().any(|x| x.rule == "R0010"),
            "expected R0010 emission, got {d:?}"
        );
    }

    #[test]
    fn r0010_silent_in_cfg_test_mod() {
        let src = "#![strict]\n#[cfg(test)]\nmod m { fn t() { let _ = todo!(); } }";
        let d = diags_for(Rule::NoTodoMacro, src);
        assert!(d.is_empty(), "expected no R0010 diag, got {d:?}");
    }

    #[test]
    fn r0011_fires_on_panic() {
        let src = "#![strict]\nfn f() { panic!(\"boom\"); }";
        let d = diags_for(Rule::NoPanic, src);
        assert!(
            d.iter().any(|x| x.rule == "R0011"),
            "expected R0011 emission, got {d:?}"
        );
    }

    #[test]
    fn r0011_silent_in_cfg_test_fn() {
        let src = "#![strict]\n#[cfg(test)]\nfn t() { panic!(\"boom\"); }";
        let d = diags_for(Rule::NoPanic, src);
        assert!(d.is_empty(), "expected no R0011 diag, got {d:?}");
    }

    #[test]
    fn r0012_fires_on_pub_bool_param() {
        let src = "#![strict]\npub fn f(detached: bool) {}";
        assert!(fires(Rule::NoBoolParam, src));
    }

    #[test]
    fn r0012_fires_on_pub_crate_bool_param() {
        let src = "#![strict]\npub(crate) fn f(detached: bool) {}";
        assert!(fires(Rule::NoBoolParam, src));
    }

    #[test]
    fn r0012_silent_on_private_fn() {
        let src = "#![strict]\nfn f(detached: bool) {}";
        let d = diags_for(Rule::NoBoolParam, src);
        assert!(d.is_empty(), "expected no R0012 diag, got {d:?}");
    }

    #[test]
    fn r0012_silent_in_test_mod() {
        let src = "#![strict]\n#[cfg(test)]\nmod m { pub fn f(x: bool) {} }";
        let d = diags_for(Rule::NoBoolParam, src);
        assert!(d.is_empty(), "expected no R0012 diag, got {d:?}");
    }

    #[test]
    fn r0014_fires_on_variable_index() {
        let src = "#![strict]\nfn f(v: &[u32], i: usize) -> u32 { v[i] }";
        assert!(fires(Rule::NoBareIndex, src));
    }

    #[test]
    fn r0014_silent_on_literal_index() {
        let src = "#![strict]\nfn f(v: &[u32]) -> u32 { v[0] }";
        let d = diags_for(Rule::NoBareIndex, src);
        assert!(d.is_empty(), "expected no R0014 diag, got {d:?}");
    }

    #[test]
    fn r0014_silent_in_test_fn() {
        let src = "#![strict]\n#[cfg(test)]\nfn t(v: &[u32], i: usize) -> u32 { v[i] }";
        let d = diags_for(Rule::NoBareIndex, src);
        assert!(d.is_empty(), "expected no R0014 diag, got {d:?}");
    }

    // Per eval/false-positives/REPORT.md: R0014 fired on every `v[a..b]`
    // slice expression in real code (30.4% FP rate, the worst rule). The
    // fix is to treat ranges as non-firing the same way literal ints are.
    #[test]
    fn r0014_silent_on_range_slice_bounded() {
        let src = "#![strict]\nfn f(v: &[u32]) -> &[u32] { &v[0..5] }";
        let d = diags_for(Rule::NoBareIndex, src);
        assert!(d.is_empty(), "expected no R0014 diag, got {d:?}");
    }

    #[test]
    fn r0014_silent_on_range_slice_open_end() {
        let src = "#![strict]\nfn f(v: &[u32], n: usize) -> &[u32] { &v[..n] }";
        let d = diags_for(Rule::NoBareIndex, src);
        assert!(d.is_empty(), "expected no R0014 diag, got {d:?}");
    }

    #[test]
    fn r0014_silent_on_range_slice_open_start() {
        let src = "#![strict]\nfn f(v: &[u32], n: usize) -> &[u32] { &v[n..] }";
        let d = diags_for(Rule::NoBareIndex, src);
        assert!(d.is_empty(), "expected no R0014 diag, got {d:?}");
    }

    #[test]
    fn r0014_silent_on_full_range_slice() {
        let src = "#![strict]\nfn f(v: &[u32]) -> &[u32] { &v[..] }";
        let d = diags_for(Rule::NoBareIndex, src);
        assert!(d.is_empty(), "expected no R0014 diag, got {d:?}");
    }
}
