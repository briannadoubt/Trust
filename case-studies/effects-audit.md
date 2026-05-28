# Effects audit — what `trust-effects` actually does

**Purpose.** Codex called the effect system "vestigial." This document
inventories — precisely — what `crates/trust-effects/` implements
today vs. what its name implies, so the RT-10 invest-or-kill decision
has data instead of vibes.

**TL;DR.** R4001 fires when a function declares an effect set that
doesn't cover the inferred effects from a tiny seed table of 13 stdlib
names (all `io`). One of the five built-in effect kinds (`io`) has any
inference; the other four (`mut`, `async`, `panic`, `unsafe`) are
declared-name-only. The analysis is intra-procedural, simple-name keyed,
direct-call only, no type info. Two latent bugs spotted (below). The
system as shipped is honest about its scope (the lib.rs docs say so) but
the name "effect tracking" oversells by a comfortable margin.

## What's implemented (with file:line references)

| Piece | Where | What it does |
|---|---|---|
| `effect` keyword | `parser.rs:30-99` | Token-level scan for `fn IDENT … effect E + E { …` between sig and body. Strips the clause, records `name → EffectSet` in a per-file `EffectTable`. Handles trait `;`-terminated sigs and recurses into impls/mods. |
| Built-in effect names | `lib.rs:21` | `["io", "mut", "async", "panic", "unsafe"]` — purely labels; any string is allowed in a clause. |
| Std seed table | `check.rs:27-48` | 13 hardcoded simple names → `io`: `println`, `eprintln`, `print`, `eprint`, `write`, `writeln`, `dbg`, `read_to_string`, `write_text`, `write`, `read`, `create`, `open`. **All five non-`io` effects have zero seed entries.** |
| Inference pass | `check.rs:65-122` | For each fn with declared effects: walk body, collect every call (`ExprCall`, `ExprMethodCall`, `Macro`) by **simple name**, look up each in declared-then-seed, union the matches into `inferred`, compare to declared, fire R4001 on any missing. |
| R4001 emission | `check.rs:91-107` | Real byte span (function body range), with `why:` and `help:` text. |
| EffectSet algebra | `registry.rs:14-39` | `BTreeSet<Effect>`, with `is_subset_of` and `union_with`. |
| Tests | `check.rs:152-211` + `parser.rs:101-159` | 9 unit tests total. 4 check the inference, 5 check parsing. |
| Examples | `examples/04-effects/` | 2 fixtures: `declared.rs` (clean) and `undeclared.rs` (must-fail). |

## What's not implemented (the actual gap list)

Each row below would need to be true for the name "effect tracking" to
match its connotation in language-design literature.

| Capability | Status | Why |
|---|---|---|
| Trait dispatch (`obj.method()` resolves to the right impl) | ❌ | `CallCollector::visit_expr_method_call` (check.rs:139) grabs the simple method ident with no receiver-type information. `s.read()` matches the std seed `read` regardless of what `s` is. |
| Async inference | ❌ | `async fn` calling other `async fn` doesn't get `async`. The keyword is in `BUILTIN_EFFECTS` purely as a label. |
| Panic inference | ❌ | `.unwrap()`, `arr[i]`, integer overflow, `panic!()` — none contribute `panic`. R0001/R0014 cover unwrap+index but at the lint layer, not as effect propagation. |
| Unsafe inference | ❌ | `unsafe` block / `unsafe fn` calls don't propagate an `unsafe` effect. R0005 covers the comment requirement. |
| `mut` inference | ❌ | `&mut self`, mutating a global, writing through a pointer — none auto-flag `mut`. |
| Transitive propagation through undeclared intermediaries | ❌ | If `c()` calls `println!`, `b()` calls `c()`, and `c` isn't in the declared table, then auditing `b` doesn't see the IO. Only 1 hop, only through declared fns. |
| Cross-crate effect annotations | ❌ | `external::do_thing()` looks up `do_thing` by simple name. Either matches a seed (likely false positive) or doesn't (likely false negative). |
| Aliases / re-exports | ❌ | `use std::fs::read_to_string as slurp; slurp(p)` collects `slurp`, misses. `let r = read_to_string; r(p)` same. |
| Function pointers / dyn callable | ❌ | `f()` where `f` is a `fn` or `Box<dyn Fn()>` collects `f`. |
| Closures into iterators | ⚠️ | The closure body IS visited (via default `Visit` traversal), but the effects are attributed to the enclosing fn, not the closure. Usually what you want, but the chain is invisible to the audit. |
| Allocations / `alloc` effect | ❌ | Not a category. |
| Per-call-site precision | ❌ | "Does IO on error path only" not expressible. Whole fn is binary. |
| `effect` keyword conflicts with identifiers | ⚠️ | A fn whose first sig token after the name is the literal ident `effect` (e.g. `fn f(effect: u32)`) trips the clause scanner. Parser would record garbage and strip a piece of the signature. **Untested edge case.** |

## Latent bugs spotted during the audit

### 1. Simple-name collisions with seed entries trigger phantom effects

A user crate that defines `fn read(input: &str) -> String { ... }` (no
declared effects) and then calls `read("x")` from a declared function:
the call collector grabs simple name `read`, declared table has no entry
for it, seed table returns `io`, the outer fn is now required to declare
`io` even though no IO actually happens. The diagnostic message is
correct ("missing io effect") but for entirely the wrong reason.

```rust
fn read(s: &str) -> String { s.to_uppercase() }   // pure!
fn shout(s: &str) effect {                        // declares "no effects"
    let upper = read(s);                          // collides with std seed
    upper                                         // → R4001 fires bogus io requirement
}
```

Workaround today: rename your local fn, or declare the same seed names
in the declared table (clobbering the seed). Neither is discoverable.

### 2. `effect` keyword scanner doesn't gate on context

`parser.rs:find_effect_clause` walks tokens after the fn name looking
for the literal ident `effect`, stopping only at `{` or `;`. It does not
peek inside generic param brackets `<...>` or where bounds. A signature
like:

```rust
fn f<T: SomeTrait<effect = u32>>() { ... }
```

(if that syntax existed — it's contrived but valid token-wise) would
trip the scanner inside the `<...>` group **only if the group walk
doesn't descend**. Reading the code: the scanner does NOT recurse into
groups (line 70 returns on `Brace`, so non-brace groups are skipped
over by the outer increment). So generic-bracket `effect` idents are
safe by accident.

The same scanner DOES walk the parameter group's contents because that
group is encountered as a `Group`, but the inner walk doesn't see
`effect` as a clause because `find_effect_clause` is only called once
per fn (from outside). So the actual bug surface is narrower than I
first thought — basically zero in practice. Mentioning for completeness.

## Coverage of the headline claim

The dialect's pitch lists "effect tracking generalized beyond `async`"
as one of three syntax extensions. The implementation provides:

- `io` effect with 13 stdlib-name seed entries → catches `println!` in
  the most obvious case, misses anything routed through a method or an
  alias.
- 4 other effect labels (`mut`, `async`, `panic`, `unsafe`) that the
  compiler accepts in `effect` clauses but never infers. They are
  syntactic vocabulary, not semantic checks.

The eval suite (runs 001-004) exercises **none** of the effect system.
The dialect's "100% bug prevention" finding does not depend on effects.

## What it would take to "invest"

Each bullet is its own engineering project, in rough size order:

1. **Trait + type-aware call resolution.** Requires a name-resolution
   pass. Realistic only by depending on `rust-analyzer`'s analyzers
   (heavy dep) or building a small one (months).
2. **Inference for `mut`.** Detect `&mut` params, global writes,
   `RefCell::borrow_mut`, etc. Tractable but each pattern is its own
   case.
3. **Inference for `panic`.** Detect `.unwrap()`, `arr[i]`, `panic!()`,
   division by literal zero, etc. Mostly already covered by lints; this
   would duplicate.
4. **Inference for `async`.** Walk `async fn` definitions, propagate.
   Moderate; mostly mechanical.
5. **Inference for `unsafe`.** Walk `unsafe fn` / `unsafe` blocks.
   Mostly mechanical.
6. **Fixed-point propagation through undeclared intermediaries.**
   Means walking every fn body, computing effect sets bottom-up, no
   declared/inferred distinction. The lib essentially becomes a
   whole-program purity analyzer.
7. **Cross-crate effect manifest.** Define a serialized format,
   teach `cargo` (or `trust-rustc`) to read sibling crates' effect
   tables, fall back to "unknown" for unannotated deps. Substantial.

Total honest estimate to get to "actually useful, in the same league as
Koka or Eff": 3-6 months of focused work, and even then the result
would be unsound without type info.

## What it would take to "kill"

About 30 minutes:

1. Remove `crates/trust-effects/` from the workspace.
2. Drop the dep from `crates/trust/Cargo.toml` and the call site
   in `run_pipeline`.
3. Drop the `effect` keyword from `trust-lower::lib.rs` (remove
   the `strip_effect_annotations` call from `lower()`).
4. Drop the R4001 entry from autogen tables (SPEC.md "Non-strict
   diagnostics" section).
5. Drop the two `examples/04-effects/` files.
6. Strike the "effect tracking" bullet from README's three extensions.
7. Leave `trust-attrs::strict!{}` and `#![strict]` activation as-is
   (they don't depend on effects).

The case study would say "we shipped a syntactic skeleton; building the
semantic part inside Phase 0 would have exceeded the prototype's
budget, and the eval suite never needed it. Removed pending revisit."

## Middle path (recommended if neither is appealing)

Demote effects to a **"declared intent" lint**, not a type-system
feature:

- Keep the `effect` keyword and the parser.
- Drop R4001's "missing inferred effect" semantics.
- Add **R4002 — declared-effect-required**: if a fn body contains a
  direct call to `println!` (or any name in a small "you should declare
  effect io" list), and the fn doesn't declare `io` in its effect set,
  fire the lint. No inference claim — just "you used a thing that's on
  the must-declare list."
- Document `mut`/`async`/`panic`/`unsafe` as user-defined vocabulary the
  compiler doesn't infer.

This is closer to Rust's `#[must_use]` than to Koka's effect system —
honest, mechanically simple, and aligned with the rest of the dialect
(every other lint is a pattern match, not an analysis).

## Recommendation for RT-10

Go with **kill** unless there's a specific user demand for declared
effects on internal API boundaries. The current implementation is
narrow enough to be misleading, the gap to "useful" is months of work,
and the eval data shows it isn't blocking any of the bug classes the
dialect actually catches.

The middle path is the second-choice answer if "remove a syntax
extension we already shipped" feels too aggressive. It preserves the
keyword without claiming inference we don't have.
