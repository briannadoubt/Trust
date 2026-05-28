# Case study: converting `trust-syntax` to strict mode

Date: 2026-05-25
Crate: `crates/trust-syntax` (56 lines pre-conversion, single `lib.rs`)
Activation: `trust_attrs::strict! {}` (cargo-mode marker macro)
Toolchain commit: post-`e86c949`

## Summary

Total violations: **1** (R0004 — glob import).
Fix difficulty: trivial (one-line enumeration).
Escape hatches used: **0**.
Would I recommend this conversion to other crate authors? **Yes — at this size, with this lint set, the friction is essentially zero.** But this crate is also so small and so production-clean that it is not a meaningful stress test of the dialect. The lints did not surface a real bug; they reaffirmed that a 56-line round-trip wrapper is already clean code.

## Per-rule breakdown

| Rule  | Name                | Count | Fix style                                            |
| ----- | ------------------- | ----- | ---------------------------------------------------- |
| R0004 | no-glob-import      | 1     | Enumerated `use super::*` → `use super::{Error, roundtrip}`. |
| —     | (every other rule)  | 0     | Did not fire.                                        |

Notable non-firings worth recording:
- No `.unwrap()` anywhere (R0001 clean).
- All `.expect("…")` calls carry real messages (R0002 clean).
- No `as` casts (R0003 clean).
- No `unsafe`, no `#[allow]`, no `impl Trait` returns, no user macros, no `todo!`/`panic!`, no `bool` parameters, no bare indexing (R0005–R0014 all clean).
- Cross-crate `syn::parse_str(source)` is exempt from R0042 — single positional arg of arity 1, and the callee isn't local-registry-tracked anyway.

## Hardest fix

There wasn't one. The single violation:

```diff
 #[cfg(test)]
 mod tests {
-    use super::*;
+    use super::{roundtrip, Error};
```

The test module imports two names total (`roundtrip` and `Error`) — the glob was pure laziness, not load-bearing. Enumeration is the right answer and cost nothing.

## Rules that felt wrong

None. The single rule that fired (R0004) fired correctly. No escape hatches were needed or considered.

## Lints that felt too permissive — a real finding

The test-module exemption in R0001/R0010/R0011/R0014 is broad: `#[cfg(test)] mod tests { … }` is a blanket pass for `.unwrap()`, `todo!()`, `panic!()`, and non-literal indexing. **R0004 (no-glob-import) is the exception** — it has no test exemption, which is why it fired on `use super::*` inside `#[cfg(test)] mod tests`.

That is the right design for R0004 (a test module's glob import still hides which symbols are in scope, which the rule cares about) — but it's worth flagging the asymmetry. A reasonable contributor reading the rule table would assume the test-exemption pattern is uniform; it isn't. If anything, R0001's test exemption is the looser one — and arguably the wrong default for a dialect whose stated mission is "agents misuse `.unwrap()` reflexively". An agent writing test code is still an agent reaching for `.unwrap()` reflexively.

This isn't a bug in either rule, but it's a design seam worth surfacing: **the per-rule test-exemption policy is currently implicit and per-visitor.** Either codify it in `Rule` metadata (e.g. `exempt_in_cfg_test: bool` so SPEC.md can render it) or unify the visitors so every rule makes the same call. The asymmetry is invisible until you hit it.

A separate observation: `roundtrip(source: &str)` is a single-positional-arg call to a string parameter. If `source` were ever renamed to something ambiguous (say `text` vs `input`), no lint would catch a swap. R0042 only fires at arity > 1. That is by design — but the dialect's strongest claim is "named args eliminate positional bugs", and arity-1 calls are still positional. Worth thinking about whether arity-1 should also be named-encouraged when the parameter type is a primitive (`&str`, `usize`, `bool` would already be banned for R0012). Not a bug, just a coverage edge.

## Recommendations for other trust-* crates

Ordered by size and expected friction:

| Crate                  | LOC   | Recommendation                                            |
| ---------------------- | ----- | --------------------------------------------------------- |
| `trust-attrs`     | 35    | Skip — it's a proc-macro crate and shouldn't activate itself (would create a build-order circularity worth verifying separately, and there's nothing to lint anyway). |
| `trust-effects/rule.rs` | 39 | Trivial — convert as a warm-up. |
| `trust-lower/rule.rs`   | 46 | Trivial — same. |
| `trust-lints/runner.rs` | 61 | Easy — dispatch table, likely zero or one violations.    |
| `trust-diag`      | 116   | Likely easy — diagnostic builder, mostly methods on a struct. Watch for `impl Trait` returns. |
| `trust-lints/rules.rs`  | 116 | Easy — pure data, enum + metadata. |
| `trust-std`       | 122   | Easy — thin wrappers around `std::fs`, `Duration::new`. The R0042 named-args call sites in callers will exercise this once they convert. |
| `trust/src/main.rs` | 157 | **Best next target.** CLI with `clap`, file I/O, real error paths. Likely 5–15 violations, all honest. Will exercise R0001, R0003 (path-length casts?), and the cross-crate positional-fallback story for `clap`. |
| `trust-effects/parser.rs` | 159 | Medium — token-stream parsing tends to grow `.unwrap()` on "this can't happen" branches and bare indexing into peekable iters. Good R0001/R0014 stress test. |
| `trust-lower/lib.rs` | 182 | Medium — orchestration. |
| `trust-effects/check.rs` | 211 | Medium-hard — type-directed checks, likely some `as` casts on sizes. |
| `xtask/src/main.rs`    | 246   | Easy-ish — build-tool code is forgiving. |
| `trust-lower/pipe.rs` | 309 | Hard — token-stream rewriting is `.unwrap()`-heavy and uses bare indexing. **The crucible.** Real test of whether R0001 + R0014 cover the right surface or are too aggressive for parser-like code. |
| `trust-lints/lib.rs` | 354 | Hard-ish — visitor patterns plus file I/O. |
| `trust-lower/named_args.rs` | 599 | Hardest — same concerns as pipe.rs at 2× the size. Save for last; lessons from pipe.rs will inform whether escape hatches need formalisation. |
| `trust-lints/strict.rs` | 837 | **Skip until pipe.rs and named_args.rs are converted.** This is the lint engine. Self-applying it will be the most rigorous dogfooding but also the highest-risk for circular reasoning ("the rule that flags itself"). Convert only after the rules have proved themselves on adjacent crates. |

**Concrete next step:** convert `trust/src/main.rs`. It is the smallest crate with realistic surface area (file I/O, `clap`, error handling) and will produce the first non-trivial violation report. If that report has more than 5 escape hatches, the dialect needs tightening before going further.

## Honest verdict

The dialect is livable on this file, but this file is not where the dialect's claims get tested. One violation in 56 lines of clean wrapper code is a sample size of one; the only finding with teeth is the **asymmetric test-exemption policy** across rules, which is worth promoting from convention to metadata. The real dogfooding starts when `trust/src/main.rs` and the lowering passes get the same treatment — those are where the friction (or the bug-prevention payoff) will actually be measurable.
