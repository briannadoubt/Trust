//! Parse `fn NAME(...) -> RET effect E + E { ... }` annotations from a
//! token stream. The `effect` clause is recognised by the literal ident
//! `effect` appearing between the parameter list and the body `{` (or `;`
//! for trait declarations without bodies), at the top level of the
//! signature (i.e. not nested inside any `(...)` / `[...]` / `<...>`).
//!
//! Stripping happens at the token level so the downstream syn parse can
//! accept the function signature.

use crate::registry::{EffectSet, EffectTable};
use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};

/// Strip `effect E + E + ...` annotations from every function signature in
/// the stream and return both the cleaned stream and a table of the
/// declared effects.
pub fn strip_effect_annotations(tokens: TokenStream) -> (TokenStream, EffectTable) {
    let mut table = EffectTable::new();
    let stripped = walk(tokens, &mut table);
    (stripped, table)
}

fn walk(tokens: TokenStream, table: &mut EffectTable) -> TokenStream {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(trees.len());
    let mut i = 0;

    while i < trees.len() {
        // Detect `fn IDENT ...` and look for an `effect` clause in the
        // signature before the body brace or trailing semicolon.
        if let TokenTree::Ident(id) = &trees[i] {
            if *id == "fn" {
                if let Some(TokenTree::Ident(name)) = trees.get(i + 1) {
                    let fn_name = name.to_string();
                    if let Some((effect_start, effect_end, effects)) =
                        find_effect_clause(&trees, i + 2)
                    {
                        // Emit verbatim up to (but not including) the `effect` ident.
                        out.extend(trees[i..effect_start].iter().cloned());
                        table.insert(fn_name, EffectSet::from_names(effects));
                        i = effect_end;
                        continue;
                    }
                }
            }
        }

        // Recurse into groups so nested fns (in impls, mods, traits) are seen.
        match &trees[i] {
            TokenTree::Group(g) => {
                let inner = walk(g.stream(), table);
                let mut new_group = Group::new(g.delimiter(), inner);
                new_group.set_span(g.span());
                out.push(TokenTree::Group(new_group));
            }
            other => out.push(other.clone()),
        }
        i += 1;
    }

    out.into_iter().collect()
}

/// From `trees[start]` forward, scan for an `effect IDENT (+ IDENT)*`
/// clause appearing before the function body (`{...}`) or trailing `;`.
/// Returns `(effect_keyword_idx, one_past_last_effect_idx, names)`.
fn find_effect_clause(trees: &[TokenTree], start: usize) -> Option<(usize, usize, Vec<String>)> {
    let mut j = start;
    while j < trees.len() {
        match &trees[j] {
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => return None,
            TokenTree::Punct(p) if p.as_char() == ';' => return None,
            TokenTree::Ident(id) if id == "effect" => {
                let clause_start = j;
                let mut k = j + 1;
                let mut names: Vec<String> = Vec::new();
                while k < trees.len() {
                    if let TokenTree::Ident(name) = &trees[k] {
                        names.push(name.to_string());
                        k += 1;
                        if matches!(
                            trees.get(k),
                            Some(TokenTree::Punct(p)) if p.as_char() == '+'
                        ) {
                            k += 1;
                            continue;
                        }
                    }
                    break;
                }
                // Allow an empty list — `fn main() effect { ... }` means
                // "declared, with no effects". We still strip the `effect`
                // keyword and record the empty set so the check fires.
                return Some((clause_start, k, names));
            }
            _ => j += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(src: &str) -> (String, EffectTable) {
        let tokens: TokenStream = src.parse().expect("test src must tokenize");
        let (out, table) = strip_effect_annotations(tokens);
        (out.to_string(), table)
    }

    #[test]
    fn single_effect_stripped_and_recorded() {
        let src = "fn f() effect io { println!(\"hi\"); }";
        let (out, table) = strip(src);
        assert!(
            !out.contains("effect io"),
            "effect clause should be stripped: {out}"
        );
        let effects = table.get("f").expect("f should be in table");
        let names: Vec<&str> = effects.0.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, vec!["io"]);
    }

    #[test]
    fn multiple_effects_with_plus() {
        let src = "fn g(x: u32) -> u32 effect io + mut { x + 1 }";
        let (out, table) = strip(src);
        assert!(!out.contains("effect"), "should be stripped: {out}");
        assert!(out.contains("-> u32 {"), "return type preserved: {out}");
        let effects = table.get("g").expect("g should be in table");
        let names: Vec<&str> = effects.0.iter().map(|e| e.0.as_str()).collect();
        // BTreeSet sorts alphabetically.
        assert_eq!(names, vec!["io", "mut"]);
    }

    #[test]
    fn no_effect_clause_is_noop() {
        let src = "fn f() -> u32 { 1 }";
        let (out, table) = strip(src);
        assert!(out.contains("fn f"));
        assert!(table.get("f").is_none());
    }

    #[test]
    fn effect_inside_nested_impl() {
        let src = "impl S { fn m(&self) effect io {} }";
        let (out, table) = strip(src);
        assert!(!out.contains("effect"));
        assert!(table.get("m").is_some());
    }

    #[test]
    fn trait_signature_with_effect_and_semicolon() {
        let src = "trait T { fn m(&self) effect io; }";
        let (out, table) = strip(src);
        assert!(!out.contains("effect"));
        assert!(table.get("m").is_some());
    }
}
