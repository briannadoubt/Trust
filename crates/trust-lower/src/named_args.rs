//! Phase 3: named arguments.
//!
//! At call sites `f(name: value, name: value)`, look up `f` in the local
//! [`CalleeRegistry`] (built from `fn` definitions in this file). If
//! registered, validate that supplied names exist in the declared params
//! and rewrite to positional in declaration order. If not registered (or
//! when called as a method or via a qualified path), strip the `name:`
//! prefix and trust the caller's argument order.
//!
//! Lowering target: every named call becomes a plain positional Rust call,
//! so syn and rustc can take it from there.

use proc_macro2::{Delimiter, Group, Punct, Spacing, Span, TokenStream, TokenTree};
use trust_diag::Diagnostic;
use std::collections::{HashMap, HashSet};

use crate::preprocess::from_vec;
use crate::std_signatures::STD_SIGNATURES;

#[derive(Debug, Default)]
pub struct CalleeRegistry {
    /// Map from function name → declared parameter names in order
    /// (excluding `self`).
    pub fns: HashMap<String, Vec<String>>,
}

impl CalleeRegistry {
    /// Walk a token stream recursively, recording every `fn NAME(PARAMS)`
    /// definition (free function, method, trait method).
    ///
    /// **Conflict handling:** if the same name appears twice with
    /// *different* parameter lists (e.g. `mod a { fn f(x: u32) }` and
    /// `mod b { fn f(y: String) }`), the name is marked ambiguous and
    /// excluded from the registry entirely. R0042 then falls back to
    /// cross-crate behavior (positional silently accepted) rather than
    /// guessing which signature the caller meant. The same name with
    /// identical params is silently de-duplicated.
    ///
    /// **Cross-crate seeding (RT-32):** after collecting the local fns,
    /// the registry is seeded from `STD_SIGNATURES` — a build-time index
    /// of every `pub fn` in `trust-std`. Local definitions take
    /// precedence; std entries fill the gaps so call sites like
    /// `trust_std::fs::write_text(path: p, contents: c)` get
    /// reordered/stripped to the real `(path, contents)` order. If a
    /// std name collides with a local fn of a *different* signature, the
    /// local one wins (the closer scope is more likely what the caller
    /// meant).
    pub fn collect(tokens: &TokenStream) -> Self {
        Self::collect_with_extras(tokens, &[])
    }

    /// Like [`collect`], but also seeds the registry from an externally
    /// supplied list of `(name, params)` entries (e.g. a crate-wide index
    /// gathered by `trust-rustc` from sibling files in the same
    /// `src/` tree — see RT-40).
    ///
    /// Precedence (highest to lowest):
    ///   1. Locally defined fns in this file (this is the closest scope).
    ///   2. Cross-file (crate-wide) extras passed in here.
    ///   3. Bundled `STD_SIGNATURES` (trust-std).
    ///
    /// Names that are *locally ambiguous* (two definitions in this file
    /// with different param lists) are excluded from every layer — we'd
    /// rather fall back to cross-crate behaviour than guess.
    pub fn collect_with_extras(
        tokens: &TokenStream,
        extras: &[(String, Vec<String>)],
    ) -> Self {
        let mut fns: HashMap<String, Vec<String>> = HashMap::new();
        let mut ambiguous: HashSet<String> = HashSet::new();
        walk_for_fns(tokens.clone(), &mut fns, &mut ambiguous);
        // Layer 2: crate-wide extras. Local fns win on conflict.
        for (name, params) in extras {
            if ambiguous.contains(name) || fns.contains_key(name) {
                continue;
            }
            fns.insert(name.clone(), params.clone());
        }
        // Layer 3: bundled std index. Local fns / extras win on conflict.
        for (name, params) in STD_SIGNATURES {
            let name_string = (*name).to_string();
            if ambiguous.contains(&name_string) || fns.contains_key(&name_string) {
                continue;
            }
            fns.insert(
                name_string,
                params.iter().map(|s| (*s).to_string()).collect(),
            );
        }
        CalleeRegistry { fns }
    }
}

fn walk_for_fns(
    tokens: TokenStream,
    fns: &mut HashMap<String, Vec<String>>,
    ambiguous: &mut HashSet<String>,
) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for i in 0..trees.len() {
        if let TokenTree::Ident(id) = &trees[i] {
            if *id == "fn" {
                if let (Some(TokenTree::Ident(name)), Some(rest)) =
                    (trees.get(i + 1), trees.get(i + 2))
                {
                    let params_group = match rest {
                        TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis => {
                            Some(g.clone())
                        }
                        _ => find_first_paren_after(&trees, i + 2),
                    };
                    if let Some(g) = params_group {
                        let params = parse_param_names(&g.stream());
                        let name_str = name.to_string();
                        if ambiguous.contains(&name_str) {
                            continue;
                        }
                        match fns.get(&name_str) {
                            Some(existing) if existing != &params => {
                                fns.remove(&name_str);
                                ambiguous.insert(name_str);
                            }
                            Some(_) => {
                                // identical re-declaration, silently de-dup
                            }
                            None => {
                                fns.insert(name_str, params);
                            }
                        }
                    }
                }
            }
        }
    }
    for tree in &trees {
        if let TokenTree::Group(g) = tree {
            walk_for_fns(g.stream(), fns, ambiguous);
        }
    }
}

fn find_first_paren_after(trees: &[TokenTree], start: usize) -> Option<Group> {
    for tree in &trees[start..] {
        if let TokenTree::Group(g) = tree {
            if g.delimiter() == Delimiter::Parenthesis {
                return Some(g.clone());
            }
        }
    }
    None
}

fn parse_param_names(params: &TokenStream) -> Vec<String> {
    split_by_top_comma(params.clone())
        .into_iter()
        .filter_map(extract_param_name)
        .collect()
}

fn extract_param_name(segment: TokenStream) -> Option<String> {
    let trees: Vec<TokenTree> = segment.into_iter().collect();
    let mut i = 0;

    // Skip leading attributes like `#[foo]`.
    while i < trees.len() {
        if let TokenTree::Punct(p) = &trees[i] {
            if p.as_char() == '#' {
                let mut j = i + 1;
                if matches!(trees.get(j), Some(TokenTree::Punct(p2)) if p2.as_char() == '!') {
                    j += 1;
                }
                if matches!(
                    trees.get(j),
                    Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket
                ) {
                    i = j + 1;
                    continue;
                }
            }
        }
        break;
    }

    // Skip `mut` / `ref` modifiers.
    while let Some(TokenTree::Ident(id)) = trees.get(i) {
        let s = id.to_string();
        if s == "mut" || s == "ref" {
            i += 1;
        } else {
            break;
        }
    }

    // Skip `&` and lifetime prefixes for `self` patterns like `&self`, `&mut self`.
    if let Some(TokenTree::Punct(p)) = trees.get(i) {
        if p.as_char() == '&' {
            i += 1;
            // Skip lifetime '...
            if matches!(trees.get(i), Some(TokenTree::Punct(p)) if p.as_char() == '\'') {
                i += 2; // '<lifetime ident>
            }
            // Skip mut after &
            if matches!(trees.get(i), Some(TokenTree::Ident(id)) if id == "mut") {
                i += 1;
            }
        }
    }

    if let Some(TokenTree::Ident(id)) = trees.get(i) {
        let s = id.to_string();
        if s == "self" {
            return None;
        }
        return Some(s);
    }
    None
}

pub fn rewrite(
    tokens: TokenStream,
    registry: &CalleeRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    strict_mode: bool,
) -> TokenStream {
    rewrite_stream(tokens, registry, diagnostics, strict_mode)
}

fn rewrite_stream(
    tokens: TokenStream,
    registry: &CalleeRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    strict_mode: bool,
) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let param_groups = find_param_group_indices(&trees);
    let mut out: Vec<TokenTree> = Vec::with_capacity(trees.len());
    for (i, tree) in trees.into_iter().enumerate() {
        match tree {
            TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis => {
                let recursed = rewrite_stream(g.stream(), registry, diagnostics, strict_mode);
                if param_groups.contains(&i) {
                    // Function parameter list — leave intact. The inner stream
                    // is still recursed in case a default-value expression (or
                    // a nested closure) contains rewritable calls.
                    let mut new_group = Group::new(Delimiter::Parenthesis, recursed);
                    new_group.set_span(g.span());
                    out.push(TokenTree::Group(new_group));
                } else {
                    let callee = preceding_ident(&out);
                    let new_inner = rewrite_call_args(
                        recursed,
                        callee.as_deref(),
                        g.span(),
                        registry,
                        diagnostics,
                        strict_mode,
                    );
                    let mut new_group = Group::new(Delimiter::Parenthesis, new_inner);
                    new_group.set_span(g.span());
                    out.push(TokenTree::Group(new_group));
                }
            }
            TokenTree::Group(g) => {
                let recursed = rewrite_stream(g.stream(), registry, diagnostics, strict_mode);
                let mut new_group = Group::new(g.delimiter(), recursed);
                new_group.set_span(g.span());
                out.push(TokenTree::Group(new_group));
            }
            other => out.push(other),
        }
    }
    from_vec(out)
}

/// Mark which paren groups in `trees` are function parameter lists
/// (`fn NAME [<generics>] (params)`). Those must NOT be treated as call
/// sites — stripping `name:` from a parameter declaration would corrupt
/// the function signature.
fn find_param_group_indices(trees: &[TokenTree]) -> HashSet<usize> {
    let mut params = HashSet::new();
    for i in 0..trees.len() {
        let TokenTree::Ident(id) = &trees[i] else {
            continue;
        };
        if *id != "fn" {
            continue;
        }
        let mut j = i + 1;
        // The fn name (or `fn` followed by generics if anonymous; rare).
        if matches!(trees.get(j), Some(TokenTree::Ident(_))) {
            j += 1;
        }
        // Optional generic parameter list `<...>`. Track depth to handle
        // nested angle brackets like `<Vec<T>>`.
        if matches!(trees.get(j), Some(TokenTree::Punct(p)) if p.as_char() == '<') {
            let mut depth = 1;
            j += 1;
            while j < trees.len() && depth > 0 {
                if let TokenTree::Punct(p) = &trees[j] {
                    match p.as_char() {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        _ => {}
                    }
                }
                j += 1;
            }
        }
        // First paren group after the (possibly-generic) name is the params.
        while j < trees.len() {
            if let TokenTree::Group(g) = &trees[j] {
                if g.delimiter() == Delimiter::Parenthesis {
                    params.insert(j);
                    break;
                }
            }
            j += 1;
        }
    }
    params
}

/// The callee, if the paren group's preceding token is an identifier.
/// For paths like `foo::bar(...)` this returns `"bar"` — the final segment.
///
/// **RT-41:** Returns `None` for method calls (`x.foo(...)`). The leading
/// `.` separates the receiver expression from the method ident, and a
/// method's signature has no relation to a free fn of the same simple
/// name. Matching `xs.insert(k, v)` against `trust_std::*::insert`
/// (or any local free fn named `insert`) would either fire a spurious
/// R0042 or reorder by the wrong param list. Best to fall through to the
/// "unknown callee" branch (strip names, keep order).
fn preceding_ident(out: &[TokenTree]) -> Option<String> {
    match out.last() {
        Some(TokenTree::Ident(id)) => {
            // Method-call detection: an ident immediately preceded by a
            // `.` punct is a method name, not a free-fn callee.
            if out.len() >= 2 {
                if let Some(TokenTree::Punct(p)) = out.get(out.len() - 2) {
                    if p.as_char() == '.' {
                        return None;
                    }
                }
            }
            Some(id.to_string())
        }
        _ => None,
    }
}

fn rewrite_call_args(
    args: TokenStream,
    callee: Option<&str>,
    call_span: Span,
    registry: &CalleeRegistry,
    diagnostics: &mut Vec<Diagnostic>,
    strict_mode: bool,
) -> TokenStream {
    let segments = split_by_top_comma(args);
    if segments.is_empty() {
        return TokenStream::new();
    }

    let parsed: Vec<(Option<(String, Span)>, TokenStream)> =
        segments.into_iter().map(extract_named).collect();
    let any_named = parsed.iter().any(|(n, _)| n.is_some());
    let all_named = parsed.iter().all(|(n, _)| n.is_some());

    // R0042: in strict mode, calls to locally-defined functions with
    // arity > 1 must use named arguments. This is the dialect's main
    // bug-prevention rule — it's why named arguments exist.
    //
    // RT-42: the diagnostic's span is the call's opening paren — without
    // this, every R0042 collapsed to line 1 col 1 (the strict marker).
    if strict_mode && parsed.len() > 1 && !all_named {
        if let Some(name) = callee {
            if let Some(declared) = registry.fns.get(name) {
                let suggestion = declared
                    .iter()
                    .map(|p| format!("{p}: ..."))
                    .collect::<Vec<_>>()
                    .join(", ");
                diagnostics.push(
                    Diagnostic::error(
                        "R0042",
                        format!(
                            "call to `{name}` must use named arguments (arity {})",
                            declared.len()
                        ),
                        span_to_range(call_span),
                    )
                    .with_why(
                        "positional argument ordering is the largest LLM-authored bug class in Rust; named args eliminate it".to_string(),
                    )
                    .with_help(format!("rewrite as `{name}({suggestion})`")),
                );
            }
        }
    }

    if !any_named {
        return reconstruct(parsed);
    }

    // Validate + reorder against the local registry when possible.
    if let Some(name) = callee {
        if let Some(declared) = registry.fns.get(name) {
            if all_named {
                for (n, _) in &parsed {
                    let Some((supplied, supplied_span)) = n else {
                        continue;
                    };
                    if !declared.iter().any(|d| d == supplied) {
                        diagnostics.push(
                            Diagnostic::error(
                                crate::Rule::NamedArgUnknownParam.code(),
                                format!(
                                    "`{name}` has no parameter named `{supplied}`"
                                ),
                                span_to_range(*supplied_span),
                            )
                            .with_why(
                                "named arguments are validated against the callee's declared parameter list".to_string(),
                            )
                            .with_help(format!(
                                "declared parameters: {}",
                                declared.join(", ")
                            )),
                        );
                    }
                }

                let mut by_name: HashMap<String, TokenStream> = HashMap::new();
                for (n, v) in parsed {
                    if let Some((name, _span)) = n {
                        by_name.insert(name, v);
                    }
                }
                let reordered: Vec<(Option<(String, Span)>, TokenStream)> = declared
                    .iter()
                    .filter_map(|d| by_name.remove(d).map(|v| (None, v)))
                    .collect();
                return reconstruct(reordered);
            }
        }
    }

    // Fallback: strip names, keep order.
    reconstruct(parsed)
}

fn extract_named(segment: TokenStream) -> (Option<(String, Span)>, TokenStream) {
    let trees: Vec<TokenTree> = segment.into_iter().collect();
    // Pattern: IDENT ':' (Alone spacing — distinguishes from `::`) <rest>
    if trees.len() >= 2 {
        if let (TokenTree::Ident(name), TokenTree::Punct(colon)) = (&trees[0], &trees[1]) {
            if colon.as_char() == ':' && colon.spacing() == Spacing::Alone {
                let value: TokenStream = trees[2..].iter().cloned().collect();
                return (Some((name.to_string(), name.span())), value);
            }
        }
    }
    (None, from_vec(trees))
}

/// Convert a `proc_macro2::Span` to a byte range into the original source.
/// Requires the `span-locations` feature on `proc-macro2` — without it,
/// `byte_range()` returns `0..0` for every span, collapsing diagnostics to
/// line 1 col 1 (the regression RT-42 fixes).
fn span_to_range(span: Span) -> std::ops::Range<usize> {
    span.byte_range()
}

fn reconstruct(parsed: Vec<(Option<(String, Span)>, TokenStream)>) -> TokenStream {
    let mut out = TokenStream::new();
    for (i, (_, value)) in parsed.into_iter().enumerate() {
        if i > 0 {
            out.extend(std::iter::once(TokenTree::Punct(Punct::new(
                ',',
                Spacing::Alone,
            ))));
        }
        out.extend(value);
    }
    out
}

/// Split a token stream at top-level commas, excluding commas inside
/// closure-parameter lists (`|x, y|`).
///
/// **Closure tracking.** A `|` token toggles a "we are inside closure
/// params" flag; while the flag is set, commas are part of the closure's
/// parameter list and must not split the surrounding call. The two-token
/// sequence `||` (logical OR, or an empty-param closure) toggles the
/// flag twice and so leaves splitting behaviour unchanged either way.
///
/// This was R0042's worst pre-fix bug: a single-argument call like
/// `each(|x, y| x + y)` was split into two segments at the closure's
/// internal comma, so R0042 fired on an arity-1 call. Surfaced by a
/// dogfood subagent on `crates/trust/src/main.rs`.
fn split_by_top_comma(tokens: TokenStream) -> Vec<TokenStream> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut segments: Vec<Vec<TokenTree>> = Vec::new();
    let mut current: Vec<TokenTree> = Vec::new();
    let mut in_closure_params = false;
    // Depth of unmatched `<` we're currently inside. Commas inside generic
    // argument lists (e.g. `HashMap<K, V>`, or turbofish `foo::<A, B>(..)`)
    // must NOT split the surrounding arg/param list. RT-39: without this,
    // a param like `map: &mut HashMap<K, V>` was split into two segments at
    // the comma between `K` and `V`, inflating reported arity. We only
    // count `<`/`>` when they appear as `Alone`-spacing punct, to avoid
    // tangling with operators like `<<`, `<=`, `->`, `=>`. This is heuristic
    // but matches every well-formed Rust signature.
    let mut angle_depth: i32 = 0;
    for tree in trees {
        if let TokenTree::Punct(p) = &tree {
            match p.as_char() {
                '|' => {
                    in_closure_params = !in_closure_params;
                    current.push(tree);
                    continue;
                }
                '<' if p.spacing() == Spacing::Alone => {
                    angle_depth += 1;
                    current.push(tree);
                    continue;
                }
                '>' if p.spacing() == Spacing::Alone && angle_depth > 0 => {
                    angle_depth -= 1;
                    current.push(tree);
                    continue;
                }
                ',' if !in_closure_params && angle_depth == 0 => {
                    if !current.is_empty() {
                        segments.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                _ => {}
            }
        }
        current.push(tree);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments.into_iter().map(from_vec).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lower_str(input: &str) -> (String, Vec<Diagnostic>) {
        lower_with_mode(input, false)
    }

    fn lower_strict(input: &str) -> (String, Vec<Diagnostic>) {
        lower_with_mode(input, true)
    }

    fn lower_with_mode(input: &str, strict: bool) -> (String, Vec<Diagnostic>) {
        let tokens: TokenStream = input.parse().expect("test input must tokenize");
        let registry = CalleeRegistry::collect(&tokens);
        let mut diags = Vec::new();
        let out = rewrite(tokens, &registry, &mut diags, strict);
        (out.to_string(), diags)
    }

    #[test]
    fn registry_finds_local_fns() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }";
        let tokens: TokenStream = src.parse().expect("test input must tokenize");
        let reg = CalleeRegistry::collect(&tokens);
        let params = reg.fns.get("area").expect("area should be in registry");
        assert_eq!(params, &vec!["width".to_string(), "height".to_string()]);
    }

    #[test]
    fn registry_skips_self_parameter() {
        let src = "impl S { fn foo(&self, x: u32, y: u32) {} }";
        let tokens: TokenStream = src.parse().expect("test input must tokenize");
        let reg = CalleeRegistry::collect(&tokens);
        let params = reg.fns.get("foo").expect("foo should be in registry");
        assert_eq!(params, &vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn rewrite_named_call_strips_names() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(width: 4, height: 6); }";
        let (out, diags) = lower_str(src);
        assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
        // The definition must keep `width:` / `height:` — only the call site
        // should be stripped.
        assert!(
            out.contains("area (width : u32 , height : u32)"),
            "definition signature must be preserved: {out}"
        );
        assert!(
            out.contains("area (4 , 6)") || out.contains("area(4 , 6)"),
            "expected positional area(4, 6) at the call site: {out}"
        );
    }

    #[test]
    fn rewrite_reorders_named_args_against_registry() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(height: 6, width: 4); }";
        let (out, diags) = lower_str(src);
        assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
        // After reorder, declared order is width, height — so 4 should come before 6.
        let area_pos = out.find("area").expect("expected area call");
        let four_pos = out[area_pos..].find('4').expect("expected 4");
        let six_pos = out[area_pos..].find('6').expect("expected 6");
        assert!(
            four_pos < six_pos,
            "expected 4 before 6 after reorder: {out}"
        );
    }

    #[test]
    fn rewrite_emits_diagnostic_on_unknown_name() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(wodth: 4, height: 6); }";
        let (_out, diags) = lower_str(src);
        assert!(
            diags.iter().any(|d| d.rule == "R3001"),
            "expected R3001 diagnostic, got {diags:?}"
        );
    }

    #[test]
    fn rewrite_strips_names_for_unknown_callee() {
        let src = "fn main() { let _ = upstream::area(width: 4, height: 6); }";
        let (out, diags) = lower_str(src);
        assert!(
            diags.is_empty(),
            "should silently strip for unknown: {diags:?}"
        );
        assert!(
            !out.contains("width :") && !out.contains("width:"),
            "names should be stripped: {out}"
        );
    }

    #[test]
    fn rewrite_passes_through_positional_calls() {
        let src = "fn f(a: u32, b: u32) {} fn main() { f(1, 2); }";
        let (out, diags) = lower_str(src);
        assert!(diags.is_empty(), "no diags: {diags:?}");
        assert!(
            out.contains("f (1 , 2)") || out.contains("f(1 , 2)") || out.contains("f (1, 2)"),
            "expected positional preserved: {out}"
        );
    }

    #[test]
    fn rewrite_handles_nested_named_calls() {
        let src = "fn add(a: u32, b: u32) -> u32 { a + b }\nfn main() { let _ = add(a: add(a: 1, b: 2), b: 3); }";
        let (out, diags) = lower_str(src);
        assert!(diags.is_empty(), "no diags: {diags:?}");
        // Outer add gets (add(1, 2), 3); inner gets (1, 2).
        let add_pos = out.find("add").expect("expected add");
        assert!(out[add_pos..].contains("add"));
    }

    #[test]
    fn double_colon_is_not_a_named_arg() {
        let src = "fn main() { let _ = std::cmp::max(1, 2); }";
        let (out, _) = lower_str(src);
        // `std::cmp::max` should not be touched (no `name: value` syntax)
        assert!(out.contains("std :: cmp :: max") || out.contains("std::cmp::max"));
    }

    #[test]
    fn method_call_with_named_args_strips_silently() {
        let src = "fn main() { let s = String::new(); s.split_at(at: 5); }";
        let (out, diags) = lower_str(src);
        assert!(
            diags.is_empty(),
            "method calls without registry entries should be silent"
        );
        assert!(
            !out.contains("at :") && !out.contains("at:"),
            "name should be stripped: {out}"
        );
    }

    #[test]
    fn r0042_fires_on_positional_call_in_strict_mode() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(4, 6); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            diags.iter().any(|d| d.rule == "R0042"),
            "expected R0042 for positional call to local fn, got {diags:?}"
        );
    }

    #[test]
    fn r0042_silent_outside_strict_mode() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(4, 6); }";
        let (_out, diags) = lower_str(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "non-strict source must be silent for R0042: {diags:?}"
        );
    }

    #[test]
    fn r0042_silent_on_named_call() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(width: 4, height: 6); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "named call must not fire R0042: {diags:?}"
        );
    }

    #[test]
    fn r0042_silent_on_arity_one() {
        let src = "fn double(x: u32) -> u32 { x * 2 }\nfn main() { let _ = double(5); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "arity-1 calls don't need names: {diags:?}"
        );
    }

    #[test]
    fn r0042_silent_on_unregistered_callee() {
        let src = "fn main() { let _ = upstream::area(4, 6); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "cross-crate calls fall back to positional: {diags:?}"
        );
    }

    #[test]
    fn r0042_fires_on_mixed_positional_named() {
        let src = "fn area(width: u32, height: u32) -> u32 { width * height }\nfn main() { let _ = area(4, height: 6); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            diags.iter().any(|d| d.rule == "R0042"),
            "mixed positional + named should fire R0042: {diags:?}"
        );
    }

    // Codex/FP audit follow-up: same name with different signatures across
    // modules ("first-wins collision") used to silently mis-register. Now
    // the name is dropped from the registry entirely and R0042 falls back
    // to cross-crate behaviour (silent on positional, doesn't pretend to
    // know the param list).
    #[test]
    fn registry_drops_ambiguous_local_fn_names() {
        let src = "mod a { pub fn make_point(x: i32, y: i32) {} }\n\
                   mod b { pub fn make_point(name: String) {} }\n\
                   fn main() { a::make_point(1, 2); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "R0042 must not fire on ambiguous names — registry should drop: {diags:?}"
        );
    }

    // Surfaced by the trust CLI dogfood subagent: R0042 used to fire
    // on `f(|x, y| body)` because the closure's internal comma got
    // counted as a top-level arg separator, inflating the call's
    // perceived arity from 1 to 2.
    #[test]
    fn r0042_silent_on_closure_arg_with_param_comma() {
        let src = "fn each(f: fn(i32, i32) -> i32) {}\n\
                   fn main() { each(|x, y| x + y); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "R0042 must not fire on a single closure arg: {diags:?}"
        );
    }

    #[test]
    fn r0042_silent_on_closure_body_with_or() {
        // `||` (logical OR) toggles the closure-tracker twice, leaving
        // it in the same state — no spurious split or arg-count change.
        let src = "fn each(f: fn(bool) -> bool) {}\n\
                   fn main() { each(|x| x || true); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "R0042 must not trip on || in a closure body: {diags:?}"
        );
    }

    #[test]
    fn r0042_still_fires_on_multi_closure_args() {
        // Two closure arguments separated by a real top-level comma —
        // arity is 2, R0042 should fire.
        let src = "fn either(a: fn(i32) -> i32, b: fn(i32) -> i32) {}\n\
                   fn main() { either(|x| x + 1, |y| y - 1); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            diags.iter().any(|d| d.rule == "R0042"),
            "R0042 should still fire on two closure args: {diags:?}"
        );
    }

    // RT-32: cross-crate seeding from `trust-std` lets the registry
    // resolve qualified call sites like
    // `trust_std::fs::write_text(path: ..., contents: ...)`. The
    // simple-name lookup means a *bare* `write_text(...)` matches too —
    // that's a known limitation (worth flagging if it ever causes a real
    // mis-lowering bug).
    #[test]
    fn cross_crate_std_seed_reorders_named_args() {
        let src =
            "fn main() { let _ = trust_std::fs::write_text(contents: \"hi\", path: \"x\"); }";
        let (out, diags) = lower_strict(src);
        assert!(diags.is_empty(), "expected no diags: {diags:?}");
        // After reorder, declared param order is (path, contents), so
        // "x" must come before "hi".
        let x_pos = out.find("\"x\"").expect("path arg present");
        let hi_pos = out.find("\"hi\"").expect("contents arg present");
        assert!(x_pos < hi_pos, "expected path before contents: {out}");
    }

    #[test]
    fn cross_crate_std_seed_emits_r3001_on_unknown_param() {
        // `write_text` is in the std index — supplying a name it doesn't
        // declare should fire R3001 just like a local-fn unknown name.
        let src =
            "fn main() { let _ = trust_std::fs::write_text(path: \"x\", body: \"hi\"); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            diags.iter().any(|d| d.rule == "R3001"),
            "expected R3001 for unknown std param: {diags:?}"
        );
    }

    // RT-42 regression: R0042's span used to be `0..0` (proc-macro2's
    // byte_range without span-locations), which ariadne renders as line 1
    // col 1 — landing on the strict marker for every diagnostic. Verify
    // the span now points at the actual call site.
    #[test]
    fn r0042_span_points_at_call_site_not_line_one() {
        // First line is reserved for a hypothetical strict marker — the
        // offending call is on line 4. Without RT-42 the span collapses
        // to 0..0; with it, the span covers the opening paren of the call.
        let src = "// strict marker would go here\n\
                   fn make_rect(width: u32, height: u32) -> u32 { width + height }\n\
                   fn main() {\n\
                       let _r = make_rect(10, 5);\n\
                   }\n";
        let tokens: TokenStream = src.parse().unwrap();
        let registry = CalleeRegistry::collect(&tokens);
        let mut diags = Vec::new();
        let _ = rewrite(tokens, &registry, &mut diags, true);
        let diag = diags
            .iter()
            .find(|d| d.rule == "R0042")
            .expect("expected R0042");
        assert!(
            diag.span.start > 0 || diag.span.end > 0,
            "R0042 span must not be 0..0 (RT-42): {:?}",
            diag.span
        );
        // The call sits after the first three lines (~104 bytes in), so
        // the span must not be inside the first source line.
        let line_one_end = src.find('\n').expect("first newline present");
        assert!(
            diag.span.start >= line_one_end,
            "R0042 span must point past line 1 (RT-42): start={}, line1_end={}",
            diag.span.start,
            line_one_end
        );
    }

    // RT-42 regression: R3001 (unknown named arg) had the same `0..0`
    // span bug. The span should now point at the offending name token.
    #[test]
    fn r3001_span_points_at_unknown_name_not_line_one() {
        let src = "// strict marker would go here\n\
                   fn make_rect(width: u32, height: u32) -> u32 { width + height }\n\
                   fn main() {\n\
                       let _r = make_rect(wodth: 10, height: 5);\n\
                   }\n";
        let tokens: TokenStream = src.parse().unwrap();
        let registry = CalleeRegistry::collect(&tokens);
        let mut diags = Vec::new();
        let _ = rewrite(tokens, &registry, &mut diags, true);
        let diag = diags
            .iter()
            .find(|d| d.rule == "R3001")
            .expect("expected R3001");
        let line_one_end = src.find('\n').expect("first newline present");
        assert!(
            diag.span.start >= line_one_end,
            "R3001 span must point past line 1 (RT-42): start={}, line1_end={}",
            diag.span.start,
            line_one_end
        );
        // The span should land on the literal text "wodth".
        let wodth_pos = src.find("wodth").expect("wodth in source");
        assert_eq!(
            diag.span.start, wodth_pos,
            "R3001 span should start at `wodth`: {:?}",
            diag.span
        );
    }

    // RT-41: a method call `xs.insert(k, v)` must NOT be matched against
    // a free fn with the same simple name. Without the leading-`.` check,
    // the registry entry for `insert` (e.g. from trust-std's
    // `hashmap_insert`, or a local `fn insert(...)`) would either fire a
    // spurious R0042 or rewrite the call by the wrong param list.
    #[test]
    fn r0042_silent_on_method_call_matching_free_fn_name() {
        // The free fn `insert(a, b)` would normally make `insert(1, 2)`
        // fire R0042. The method call `m.insert(...)` must be exempt.
        let src = "fn insert(a: u32, b: u32) -> u32 { a + b }\n\
                   fn main() { let mut m = std::collections::HashMap::new(); m.insert(1, 2); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            !diags.iter().any(|d| d.rule == "R0042"),
            "method call must not match free-fn registry entry: {diags:?}"
        );
    }

    #[test]
    fn method_call_with_named_args_doesnt_reorder_against_free_fn() {
        // A free fn `insert(a, b)` is in the registry. A method call
        // `m.insert(b: 2, a: 1)` must NOT be reordered (the method's real
        // signature has nothing to do with the free fn). Names get stripped
        // in the same order they appear.
        let src = "fn insert(a: u32, b: u32) -> u32 { a + b }\n\
                   fn main() { let mut m = std::collections::HashMap::new(); m.insert(b: 2, a: 1); }";
        let (out, _diags) = lower_str(src);
        let m_insert = out.find("m . insert").or_else(|| out.find("m.insert"));
        assert!(m_insert.is_some(), "expected method call preserved: {out}");
        let two_pos = out[m_insert.unwrap()..].find('2').expect("expected 2");
        let one_pos = out[m_insert.unwrap()..].find('1').expect("expected 1");
        assert!(
            two_pos < one_pos,
            "method-call args must keep source order (no reorder against free fn): {out}"
        );
    }

    // RT-40: cross-file extras let the registry resolve fns defined in
    // sibling files of the same crate. Local fns still win on conflict.
    #[test]
    fn registry_seeds_from_extras() {
        let src = "fn main() { let _ = make_rect(width: 10, height: 5); }";
        let tokens: TokenStream = src.parse().expect("test input must tokenize");
        let extras = vec![(
            "make_rect".to_string(),
            vec!["width".to_string(), "height".to_string()],
        )];
        let reg = CalleeRegistry::collect_with_extras(&tokens, &extras);
        let params = reg.fns.get("make_rect").expect("extras should seed registry");
        assert_eq!(params, &vec!["width".to_string(), "height".to_string()]);
    }

    #[test]
    fn registry_local_wins_over_extras() {
        let src = "fn f(local_param: u32) {}\nfn main() { f(local_param: 1); }";
        let tokens: TokenStream = src.parse().expect("test input must tokenize");
        let extras = vec![("f".to_string(), vec!["different".to_string()])];
        let reg = CalleeRegistry::collect_with_extras(&tokens, &extras);
        let params = reg.fns.get("f").expect("f should be in registry");
        assert_eq!(params, &vec!["local_param".to_string()]);
    }

    #[test]
    fn registry_keeps_identical_redeclarations() {
        // Same name, identical params (e.g. impls of the same trait) — fine.
        let src = "struct A; struct B;\n\
                   impl A { pub fn area(width: u32, height: u32) -> u32 { width * height } }\n\
                   impl B { pub fn area(width: u32, height: u32) -> u32 { width + height } }\n\
                   fn area(width: u32, height: u32) -> u32 { width }\n\
                   fn main() { let _ = area(4, 6); }";
        let (_out, diags) = lower_strict(src);
        assert!(
            diags.iter().any(|d| d.rule == "R0042"),
            "identical-param re-declarations don't make the name ambiguous: {diags:?}"
        );
    }
}
