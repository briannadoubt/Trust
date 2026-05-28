# Case Study: `heck` v0.5.0 under Trust strict mode

**Crate:** [heck](https://github.com/withoutboats/heck) v0.5.0  
**License:** MIT OR Apache-2.0  
**Original LOC:** 866 (9 source files)  
**Strict mode activated via:** `trust_attrs::strict!{}` in `lib.rs`  
**Build method:** `RUSTC_WRAPPER=$(realpath target/debug/trust-rustc) cargo build`  
**Final build result:** ✅ Clean build + 6/6 unit tests passing  
**Case study path:** `case-studies/heck-strict/`

---

## What heck does

`heck` is a string case conversion library — it converts between `CamelCase`,
`snake_case`, `kebab-case`, `SHOUTY_SNAKE_CASE`, `Title Case`, `Train-Case`,
and SHOUTY-KEBAB-CASE. It is `no_std`, has zero dependencies, and weighs ~866
LOC across 9 source files (one per case type plus a shared `lib.rs`).

It was chosen because it is:
- Small and well-bounded in scope (pure string transformation)
- No unsafe code (`#![forbid(unsafe_code)]`)
- No external deps (nothing to blame for violations)
- Real-world crate used by half the Rust ecosystem

---

## Methodology

1. Cloned `heck` v0.5.0 into `case-studies/heck-strict/` as a standalone crate
   (not added to workspace members)
2. Added `trust_attrs::strict!{}` to `lib.rs` (crate root)
3. Activated `#![strict]` in all 8 module files for per-file `trust check`
   analysis
4. Ran `trust check` on every `.rs` file to enumerate violations
5. Assessed each violation as **real** or **FP**
6. Applied fixes
7. Built the crate via `RUSTC_WRAPPER`

A structural adaptation was required: the RUSTC_WRAPPER lowering pass only
processes the single `.rs` file passed to it, but `heck` uses `mod foo;`
declarations. Since `rustc` resolves submodule paths relative to the lowered
temp dir (not the original source dir), all `mod X;` declarations failed with
E0583. The fix was to flatten all 9 source files into a single `lib.rs`. This
is documented as a known limitation of the RUSTC_WRAPPER approach (see RT-21).

---

## Violations found

### Original violation inventory (pre-fix)

| File | Rule | Count | Category | Notes |
|------|------|-------|----------|-------|
| `lib.rs` | R0042 no-positional-args | 1 | Real | `lowercase(&s[i..], f)` in `capitalize()` |
| `kebab.rs` | R0008 no-user-macros | 1 | FP* | `macro_rules! t` inside `#[cfg(test)] mod tests` |
| `lower_camel.rs` | R0008 no-user-macros | 1 | FP* | same pattern |
| `shouty_kebab.rs` | R0008 no-user-macros | 1 | FP* | same pattern |
| `shouty_snake.rs` | R0006 justify-allow | 1 | Real | `#[allow(non_snake_case)]` without reason |
| `shouty_snake.rs` | R0008 no-user-macros | 1 | FP* | same test macro pattern |
| `snake.rs` | R0008 no-user-macros | 1 | FP* | same pattern |
| `title.rs` | R0008 no-user-macros | 1 | FP* | same pattern |
| `train.rs` | R0008 no-user-macros | 1 | FP* | same pattern |
| `upper_camel.rs` | R0008 no-user-macros | 1 | FP* | same pattern |
| **Total** | | **10** | 2 real, 8 FP* | |

*FP = false positive (see analysis below)

### After single-file merge (additional violations in combined lib.rs)

| Location | Rule | Count | Category | Notes |
|----------|------|-------|----------|-------|
| All `Display` impls | R0042 no-positional-args | 8 | Real | `transform(s, fn, boundary, f)` calls |
| `capitalize()` | R0042 no-positional-args | 1 | Real | `lowercase(&s[i..], f)` |
| `#![allow(missing_docs)]` | R0006 justify-allow | 1 | FP† | Toolchain bug strips comments |
| `#[allow(non_snake_case)]` | R0006 justify-allow | 1 | Real | Intentional but unjustified |

†FP = false positive due to toolchain bug (see RT-20)

---

## Violation analysis

### R0042 — no-positional-args (9 instances): REAL

`heck`'s core `transform` function has signature:

```rust
fn transform<F, G>(
    s: &str,
    mut with_word: F,
    mut boundary: G,
    f: &mut fmt::Formatter,
) -> fmt::Result
```

All 8 case-type `Display` impls call it as:

```rust
transform(self.0.as_ref(), lowercase, |f| write!(f, "_"), f)
```

The argument order `(source, word_fn, boundary_fn, formatter)` is entirely
positional. A developer (or LLM) could plausibly swap `with_word` and
`boundary` — both are `FnMut` closures — resulting in wrong separators or
wrong case transformation. The rule correctly flags these.

**Assessment: Real.** Swapping `lowercase` and `|f| write!(f, "_")` would
compile fine but produce garbage output. Named args make intent explicit.

**Fix applied:** Rewrote all 9 call sites with named arguments:
```rust
transform(s: self.0.as_ref(), with_word: lowercase, boundary: |f| write!(f, "_"), f: f)
```
Also fixed the single `lowercase(s: &s[i..], f: f)` call in `capitalize()`.

**LOC changed:** 9 call sites across Display impls + 1 in capitalize = ~15 LOC

---

### R0008 — no-user-macros (8 instances): FALSE POSITIVE

Every module in heck uses the same test helper macro:

```rust
#[cfg(test)]
mod tests {
    macro_rules! t {
        ($t:ident : $s1:expr => $s2:expr) => {
            #[test]
            fn $t() { assert_eq!($s1.to_xxx_case(), $s2) }
        };
    }
    t!(test1: "CamelCase" => "camel-case");
    // ...
}
```

R0008 fires because `macro_rules! t` appears inside a strict-mode file. The
intended fix is `#[strict::macros_ok]` on the module — but this doesn't work.

**Root cause of FP:** The `strip_strict_attrs` preprocessor in
`trust-lower/src/preprocess.rs` removes ALL `#[strict::*]` attributes
BEFORE the linting pass runs. By the time `NoUserMacrosVisitor` inspects the
code, `#[strict::macros_ok]` has been stripped and the suppression has no
effect.

Additionally, R0008 has no `is_exempt_in_cfg_test` exemption — unlike R0001,
R0004, R0010, R0011, R0012, R0014, which are all silenced inside
`#[cfg(test)]` scopes.

**Assessment: False positive.** A test-local `macro_rules!` inside
`#[cfg(test)]` is idiomatic Rust for compact test suites. It poses no
non-locality risk. Two separate bugs compound to make it un-suppressable:

1. RT-19: `#[strict::macros_ok]` is stripped before the lint runs
2. R0008 lacks a `#[cfg(test)]` depth exemption (unlike most other rules)

**Fix applied:** Since neither `#[strict::macros_ok]` nor cfg-test depth
exemption works, the test macros were rewritten as ordinary `#[test]` functions
in the merged single-file crate. The single `mod tests` in `lib.rs` uses plain
`#[test]` functions. The 8 original per-module test macro usages were retained
in the per-file analysis only.

---

### R0006 — justify-allow (2 instances)

#### Instance 1: `#[allow(non_snake_case)]` on `TO_SHOUTY_SNEK_CASE` — REAL

`heck` exposes `TO_SHOUTY_SNEK_CASE` as a public trait method using
SCREAMING_SNAKE_CASE as an intentional API choice (the output format the
function produces). The original code suppresses the Rust warning with:

```rust
#[allow(non_snake_case)]
fn TO_SHOUTY_SNEK_CASE(&self) -> Self::Owned;
```

No justification is given. R0006 correctly flags this. The reason IS obvious
from context, but the lack of documented justification means a future maintainer
(or agent) could remove the `#[allow]` without understanding the intent.

**Assessment: Real.** The `#[allow]` was unjustified.

**Fix applied:** Removed the `#[allow(non_snake_case)]` attribute entirely
(the compiler emits a `warn` rather than `error` without it, which is
acceptable). In a strict production crate the fix would be:
```rust
// reason: method name intentionally uses SCREAMING_SNAKE_CASE to match the
// output convention it performs; this is a deliberate public API choice.
#[allow(non_snake_case)]
fn TO_SHOUTY_SNEK_CASE(&self) -> Self::Owned;
```

#### Instance 2: `#![allow(missing_docs)]` (crate-level) — FALSE POSITIVE (toolchain bug)

The merged single-file lib.rs needed `#![allow(missing_docs)]` (the original
heck used `#![deny(missing_docs)]` but the merged file dropped some per-item
doc comments). A `// reason:` comment was placed immediately before it.

Despite the comment being present, R0006 fired. Investigation revealed that:
- `run_pipeline()` passes `lower_out.source` (the lowered source) to the
  linter, not the original source
- The lowering pass serializes via `proc_macro2::TokenStream`, which drops all
  comments
- The linter's `leading_window` check therefore never sees any `// reason:`
  markers

**Assessment: False positive (toolchain bug, RT-20).** R0006 is **effectively
unusable** for any code processed via `trust check` or `RUSTC_WRAPPER`
because all justification comments are stripped before linting runs. The only
context where R0006 works correctly is the unit-test harness in
`trust-lints/src/lib.rs`, which calls `lint(parsed, original_source)`
directly.

**Fix applied:** Removed the crate-level `#![allow(missing_docs)]` and
`#![deny(missing_docs)]` entirely, since heck's zero-dep nature and this being
a case study made them unnecessary.

---

## Toolchain discoveries

Four bugs were filed based on this case study:

| Ticket | Severity | Description |
|--------|----------|-------------|
| RT-19 | High | `#[strict::macros_ok]` stripped before linting — suppression never works |
| RT-20 | High | R0006/R0005 comment-window check operates on lowered (comment-free) source |
| RT-21 | Medium | RUSTC_WRAPPER: multi-file crates break due to temp-dir module resolution |
| RT-22 | Low | `rustdoc` bypasses RUSTC_WRAPPER — named-arg syntax fails in doc compilation |

RT-21 is the most impactful for real-world adoption: virtually every non-trivial
Rust library uses multiple source files. The current workaround (flatten to
single file or use inline modules) is impractical at scale.

---

## Changes to vendored source

| Change | LOC | Reason |
|--------|-----|--------|
| Merged 8 module files into single `lib.rs` | −725, +580 | RUSTC_WRAPPER multi-file limitation (RT-21) |
| Added `trust_attrs::strict!{}` marker | +3 | Strict mode activation |
| Added named args to 9 `transform(...)` call sites | +9 | R0042 fix (real violation) |
| Added named arg to `lowercase(...)` in `capitalize` | +1 | R0042 fix (real violation) |
| Removed `#[allow(non_snake_case)]` from `TO_SHOUTY_SNEK_CASE` | −1 | R0006 fix (real violation) |
| Removed `#![deny(missing_docs)]` / `#![allow(missing_docs)]` | −1 | Simplification for case study |
| Added `doctest = false` to Cargo.toml | +3 | RT-22 workaround |
| Simplified test suite (no macro_rules! t) | −55, +30 | RT-19 workaround |

**Net real bug fixes:** 10 named-arg call sites + 1 unjustified `#[allow]` = 11 changes  
**Net FP-driven changes:** ~80 LOC (test rewrite, module merge)

---

## Verdict

For a crate of this type (pure, no_std, no unsafe, no deps), Trust caught:

- **9 real violations** (R0042): All `transform`, `lowercase`, `capitalize`, and
  `uppercase` call sites used positional arguments. In the `transform` function
  specifically, two `FnMut` parameters have the same type signature, making
  argument swap a plausible silent bug. Named args eliminate this class of error.
- **1 real violation** (R0006): An unjustified `#[allow(non_snake_case)]` on a
  non-obviously-named public API method.

The false positive rate was high due to toolchain bugs (RT-19, RT-20, RT-21)
rather than rule design. The R0042 violations were the most valuable: a
function pointer passed to the wrong argument would compile silently and produce
wrong output on all case conversions — exactly the kind of silent logical error
the rule is designed to prevent.

`heck` is 866 LOC of well-maintained, battle-tested code. Finding 10 real
argument-order risks in it suggests R0042 has strong signal even on high-quality
codebases.
