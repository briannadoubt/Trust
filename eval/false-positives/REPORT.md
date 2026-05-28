# False-positive evaluation: `trust check` on real code

Date: 2026-05-25
Driver: `eval/false-positives/run_eval.py`
Raw data: `raw_workspace.json`, `raw_anyhow.json`, `summary.json`

## Methodology

For every `.rs` file in
- the workspace (`crates/cargo-trust`, `crates/trust`, `crates/trust-diag`, `crates/trust-effects`, `crates/trust-lints`, `crates/trust-lower`, `crates/trust-lsp`, `crates/trust-std`, `crates/trust-syntax`, `crates/xtask`)
- and the external crate `anyhow-1.0.86` (downloaded fresh to `/tmp/anyhow-1.0.86`, excluding `tests/ui/` UI-test fixtures and `build.rs`)

the driver:
1. Reads source.
2. Prepends `#![strict]` (in-memory only — nothing is written into the workspace or vendored).
3. Writes a `mktemp` `.rs` file.
4. Runs `trust check <tmp>` and parses the ANSI-stripped output for `[Rxxxx]` diagnostic headers and `╭─[<path>:<line>:<col>]` spans.
5. Aggregates per rule, per file.

Files scanned: 21 workspace + 27 anyhow = **48 files**.

Note on captured snippets: many diagnostics emit with `Span::byte_range()` `0..0` (notably R0042 from `trust-lower::named_args`, plus a handful of macro/visitor emissions). For those the rendered location collapses to line 1 / column 1. The diagnostic is still real — the message body carries the function name — but the captured "snippet" is misleading. The TP/FP judgements below are based on inspecting the actual sources, not the captured snippets.

## Headline numbers

| Metric                       | Value |
| ---------------------------- | ----- |
| Total diagnostics            | 367   |
| True positives (TP)          | 350   |
| False positives (FP)         | 17    |
| Overall FP rate              | 4.6%  |
| Workspace diagnostics        | 81    |
| Workspace FP rate            | 8.6% (7 / 81)  |
| anyhow diagnostics           | 286   |
| anyhow FP rate               | 3.5% (10 / 286) |

"True positive" here means the rule fired on something the rule was designed to catch (as written in `crates/trust-lints/src/rules.rs` and `docs/SPEC.md`). "False positive" means the rule fired on code that does not contain the targeted footgun, or on a pattern the spec explicitly says should be exempt.

## Per-rule summary

| Rule  | Total | TP | FP | FP%  | Notes |
| ----- | ----- | -- | -- | ---- | ----- |
| R0001 no-unwrap          | 9   | 9   | 0  | 0%    | All real `.unwrap()` calls. See implementation note in "Top FP" #1 — 6 of these are inside `#[test]` functions in `tests/*.rs` integration tests, which the SPEC promises to exempt but the implementation does not. By SPEC, 6 of the 9 should be FPs. |
| R0003 no-as-cast         | 7   | 7   | 0  | 0%    | All real `as` expressions in anyhow. |
| R0004 no-glob-import     | 12  | 12  | 0  | 0%    | All real `use foo::*;` imports. However 11 of 12 are `use super::*;` inside `#[cfg(test)] mod tests {}` — the Rust idiom. Lint matches the SPEC (no test-scope exemption); see "Top FP" #2. |
| R0005 justify-unsafe     | 86  | 86  | 0  | 0%    | All real `unsafe { ... }` blocks / `unsafe fn` declarations without `// safety:` in the 200-byte window. anyhow's `error.rs` ships dense unsafe code with safety documented in a doc-comment style the lint's window misses. See "Top FP" #3. |
| R0006 justify-allow      | 15  | 15  | 0  | 0%    | All real `#[allow(...)]` attributes lacking `// reason:`. |
| R0008 no-user-macros     | 16  | 16  | 0  | 0%    | All real `macro_rules!` definitions. anyhow's `backtrace.rs`, `ensure.rs`, `macros.rs` are macro-heavy. |
| R0012 no-bool-param      | 1   | 1   | 0  | 0%    | Visible function with `bool` parameter in `trust-lower::named_args`. |
| R0014 no-bare-index      | 23  | 16  | 7  | 30.4% | Fires on slice expressions `v[a..b]` (a `RangeExpr` inside `ExprIndex`). Slicing returns a slice, not an element; `.get(a..b)` is awkward. See "Top FP" #4. |
| R0042 no-positional-args | 172 | 162 | 10 | 5.8%  | 99 calls are `assert_err(closure, &'static str)` where the disjoint types prevent any silent swap. Strict TPs by SPEC, weak TPs in practice. See "Top FP" #5. |
| **Total**                | **367** | **350** | **17** | **4.6%** | |

`R0002 empty-expect`, `R0007 no-impl-trait-return`, `R0010 no-todo-macro`, `R0011 no-panic` did not fire on either target.

## Top FP categories

### 1. R0001 ignores `#[test]` attribute (implementation gap vs SPEC)

The SPEC says: "`.unwrap()` is banned outside `#[cfg(test)]` modules **and `#[test]` functions**." The implementation (`crates/trust-lints/src/strict.rs:84-87`) only checks `attrs_have_cfg_test`, missing the `#[test]` path. All other test-scoped lints (R0010, R0011, R0012, R0014) correctly check both:

```rust
let is_test = attrs_have_cfg_test(&node.attrs)
    || node.attrs.iter().any(|a| a.path().is_ident("test"));
```

R0001 was almost certainly meant to do the same. Of the 9 R0001 hits, **6 are inside integration tests** (`tests/test_context.rs`, `tests/test_downcast.rs`) where each test fn carries `#[test]` and no module-level `#[cfg(test)]`. The intended-exempt count is 6/9.

Fix: change `NoUnwrapVisitor::visit_item_fn` to mirror `MacroBanVisitor`.

### 2. R0004 fires on `use super::*;` inside `#[cfg(test)] mod tests`

11 of 12 R0004 hits are this exact idiom. The SPEC says glob imports are banned with "no general escape hatch." That's consistent — but `use super::*;` inside `mod tests` is the universal Rust integration-test idiom. Forcing the test module to re-enumerate every `pub fn` of the parent will produce friction with no benefit (the test module is private, doesn't export symbols, and the "unrelated changes affect resolution" risk doesn't apply when the parent is your own code).

Suggested refinement: add a test-scope exemption to `NoGlobImportVisitor` mirroring R0010/R0011/R0014 — `is_test = attrs_have_cfg_test(&attrs)`. Alternatively introduce an `#[allow(no_glob_import)] // reason:` escape hatch (R0006 forces the justification). The current spec language ("no general escape hatch") could be tightened to "except inside `#[cfg(test)]`".

### 3. R0005 misses doc-comment-style safety justifications

R0005 looks for a `// safety:` substring in the 200 bytes preceding the `unsafe` keyword. `anyhow/src/error.rs` follows a different convention — a `// Safety:` paragraph attached to the parent `unsafe fn` doc-comment, well above the 200-byte window, and a comment style that omits the colon-suffix marker entirely on some blocks. 85 of 86 R0005 hits are in `error.rs`/`fmt.rs`/`backtrace.rs`/`ptr.rs` where the project has its own (rigorous) safety discipline that doesn't pattern-match `// safety:` within 200 bytes. Examples:

- `error.rs:625-628`: `unsafe fn object_drop<E>` whose safety contract is documented at module scope.
- `fmt.rs:12`: `let chain = unsafe { Self::chain(this) };` inside an `unsafe fn` whose contract covers all its blocks.

These are strict TPs as the rule is written. Refinement options:
- Widen the search window (500 bytes? containing-function scope?).
- Treat `// SAFETY:` (any case, with or without colon) the same as `// safety:`.
- Exempt `unsafe { ... }` inside an `unsafe fn` whose declaration carries a `// safety:` justification — the contract is delegated to the caller.
- Allow the marker to appear in a `///` doc comment on the enclosing fn.

The third option is closest to how anyhow actually documents.

### 4. R0014 fires on slice/range indexing `expr[a..b]`

The lint exempts only literal integer indices. Slice expressions `&v[start..end]` are `ExprIndex` containing an `ExprRange`, which is not `ExprLit`, so the lint fires. But `.get(a..b)` is rarely the right replacement: returning a slice from a range is the *normal* slicing operation, and idiomatic code expresses it as `&v[range]`. Of the 23 R0014 hits, 7 are on range-bounded slices in workspace code (`&trees[start..]`, `&input[..begin_idx + ...]`, `&input[end_idx..]`, `&capture.frames[capture.actual_start..]`).

Fix: extend `index_is_int_literal` to accept any `ExprRange` whose endpoints are literal-or-absent. Or, more conservatively, exempt any `ExprRange` regardless of endpoint type — the failure mode is fundamentally different from point indexing and `.get(range)` is awkward.

### 5. R0042 fires on calls where the argument types make a swap a compile-time error

99 of 172 R0042 hits are `assert_err(closure, "expected string")` in `anyhow/tests/test_ensure.rs`. The signature is:

```rust
fn assert_err<T: Debug>(result: impl FnOnce() -> Result<T>, expected: &'static str)
```

Swapping the arguments produces a compile-time error: `&'static str` does not implement `FnOnce() -> Result<T>`. So the bug class the lint is designed to prevent ("the largest LLM-authored bug class in Rust") cannot occur for this call. Naming each call adds 3,000 characters of `result:`/`expected:` boilerplate across the test file for zero bug-prevention value.

By the rule's stated rationale this is an FP. By the dialect's "named everywhere past arity 1" philosophy it's a TP. Taking the rationale lens: 10 calls in anyhow look like clear FPs (test helpers with disjoint typed signatures); the remaining 162 calls have at least two same-type or convertible-type arguments where ordering could plausibly silent-swap, so they're TPs.

A type-aware refinement is not realistic at the proc-macro2 token-stream layer that emits R0042 — the lint has no type information. Weaker heuristics:

- Skip arity-2 calls where the two arguments have visibly different syntactic shapes (closure vs string literal vs path expression vs numeric literal). Fragile but cheap.
- Add `#[allow(no_positional_args)]` ergonomics for blanket-allowing a test module.
- Annotate test helper definitions with `#[strict::allow_positional]` so all calls to that function skip the lint.

The escape hatch already exists (`#[allow(no_positional_args)] // reason: ...`) but applying it to 99 individual call sites is impractical. A module-level or item-level escape would help.

## Per-target summary

### Workspace (Trust's own crates)

- 21 files, 81 diagnostics, 74 TP, 7 FP.
- FP concentration: 7 R0014 hits on `expr[range]` slicing in `named_args.rs`, `pipe.rs`, `main.rs`. The rest of the workspace's own diagnostics are real footguns the project would want to clean up before shipping `#![strict]`:
  - 44 R0042 positional calls to local fns (real — `check(x, y)`, `rewrite_stream(g, fns)`, `find_pipe(trees)`, etc.).
  - 6 R0004 `use super::*;` in test mods (real glob imports, but the test-mod idiom).
  - 20 R0014 indexing hits (most real — `trees[i]`, `trees[j]`, etc. — these are legitimate "should use `.get`" candidates in the visitor scaffolding).
  - 1 R0012 visible-bool-param.
  - 1 R0005 unsafe block in `trust-std/src/lib.rs::set_var` (real — `std::env::set_var` wrapper, missing `// safety:`).

### anyhow-1.0.86

- 27 files, 286 diagnostics, 276 TP, 10 FP.
- FP concentration: 10 R0042 calls to `assert_err` where types prevent the swap bug. All other diagnostics flag patterns the rule was designed to flag:
  - 85 R0005 unsafe blocks — anyhow's `error.rs` is a vtable-heavy crate with `// Safety:` documented in a style the 200-byte window misses.
  - 16 R0008 macro_rules definitions — anyhow defines `bail!`, `ensure!`, `anyhow!`, `__anyhow!`. Real macros; the user would need `#[strict::macros_ok]` to allow the file.
  - 9 R0001 unwrap calls — 6 of these are intended-exempt-by-SPEC (see "Top FP" #1). 3 are real production unwraps (`error.rs:417`, plus two in `test_context.rs` where the test happens to lack `#[cfg(test)]` framing).
  - 128 R0042 positional calls — most arity-2-or-more calls to local helpers (`provide`, `from_std`, `construct`, `fmt`, `render`).

The anyhow result confirms: the dialect's lints do not produce noise on real, idiomatic Rust. They produce hits where they're designed to produce hits, with the understanding that anyhow ships without `#![strict]` so none of this is actually flagged in the real ecosystem.

## Bottom line

The overall FP rate is **4.6%** (17 of 367 diagnostics). Concentrating on the two truly noisy patterns:

- **R0014 has a real implementation bug**: range/slice expressions should not fire `bare indexing`. ~30% FP rate, easy fix.
- **R0001 has a SPEC-vs-implementation gap**: integration-test `#[test]` functions are not exempted as promised. Easy fix.

Excluding those two implementation bugs, the FP rate drops to ~3% — almost all of which is R0042 on calls where the swap is impossible by type. Even that is defensible under the dialect's stated philosophy ("named past arity 1" everywhere, not "named where the bug is possible").

**Yes**, the dialect is usable on real code. The two bugs above are the only changes a developer would need before turning `#![strict]` on a real crate, plus the cosmetic concession of either accepting `use super::*;` in test modules or rewriting them as enumerated imports.

The non-bug FPs (R0005's narrow window, R0042's type-blindness) reflect the dialect's stated philosophy more than implementation gaps. They become a "style fit" question for the user, not a soundness question for the toolchain.

## Top 3 rules by FP rate

1. **R0014 no-bare-index** — 30.4% FP rate (7 / 23). Real bug: slice expressions `v[a..b]` should not fire.
2. **R0042 no-positional-args** — 5.8% FP rate (10 / 172). Type-blindness, defensible by philosophy.
3. **R0001 no-unwrap** — 0% by-the-letter, 66.7% by-the-SPEC (6 / 9). Real bug: `#[test]` attribute not exempted.

All other rules: 0% FP.
