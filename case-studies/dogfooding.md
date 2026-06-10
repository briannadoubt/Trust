# Dogfooding trust-* crates with `#![strict]`

**Ticket:** RT-31 — eat your own cooking.

We tried to mark each `trust-*` workspace crate `#![strict]` and build
it under `RUSTC_WRAPPER=target/release/trust-rustc`. Goal: prove the
dialect is usable for real work and surface every remaining gap by hitting
it ourselves.

This file records per-crate status — what went strict, what broke, what
was a real bug, and what was a language/tooling gap.

## Summary

| Crate | Status | Real bugs fixed | FPs / gaps surfaced |
| ----- | ------ | --------------- | ------------------- |
| `trust-syntax`   | STRICT (pre-existing) | 0 | 0 |
| `trust-diag`     | STRICT | 0 | 0 |
| `trust` (CLI)    | STRICT | 0 (rewrote 4 positional call sites to named) | 0 |
| `trust-lsp` bin  | STRICT | 0 | 0 (lib still strict-marked from before) |
| `trust-rustc` bin | STRICT | 0 (rewrote 3 call sites) | 0 |
| `xtask`               | STRICT | 0 (rewrote 6 call sites) | 0 |
| `trust-lints/rules.rs`     | STRICT (file-level) | 0 | 0 |
| `trust-attrs`    | SKIPPED | — | proc-macro crate; strict markers don't apply to `#[proc_macro]` definitions |
| `trust-std`      | STRICT (RT-44) | 0 (rewrote 2 fs shims to named form) | tests hoisted to non-strict sibling `tests.rs` (generic-fn arity gap in registry) |
| `trust-lints/lib.rs`     | SKIPPED | — | 45+ positional helper calls in `#[cfg(test)]` block; bulk rewrite hits >100-LOC stop |
| `trust-lints/runner.rs`  | SKIPPED (with documented reason in file head) | — | RT-40 (cross-file calls to `crate::strict::run_rule`) |
| `trust-lints/strict.rs`  | SKIPPED | — | RT-41 (visit-pattern method calls match free-fn signatures by simple name) |
| `trust-lower`    | NOT ATTEMPTED | — | known: uses `|>` operator in its own tests; `fmt.sh` already skips it. Worth a follow-up. |

**Crates fully strict-marked:** 7 of 11 (after RT-44). **At least 3 acceptance criterion: met.**



**Update (RT-50):** All three bin crates (`trust`, `trust-rustc`, `xtask`) and `trust-std` are now strict-marked after RT-48 (skip attribute-internal call-like syntax) and RT-49 (skip std/core/alloc-prefixed qualified calls) landed. The trust-std signature index has been restored to its full set including `command`, `copy`, `rename`, `set_var` — these no longer false-positive on clap's `#[command(...)]` derive or on real `std::fs::copy(...)` calls.

**Update (RT-76) — bin crates reverted to stage-0 plain Rust.** Marking
`trust`, `trust-rustc`, and `xtask` `#![strict]` created an unbuildable
bootstrap cycle: their named-arg syntax only compiles *through* the
`trust-rustc` wrapper, but `trust-rustc` **is** that wrapper — so a clean
checkout (and CI, before a prebuilt binary exists) could not build them at
all. A toolchain cannot self-host its own frontend before it is built. These
three are now **stage-0 plain Rust** (no `#![strict]`, positional calls),
built by stock `cargo`. The dialect is still dogfooded: **lints** on the
strict library crates (`trust-diag`, `trust-lsp`, `trust-std`,
`trust-syntax`, `trust-lints/rules.rs`), and the **named-arg / pipe syntax**
on the `examples/` fixtures built through the wrapper in CI. This is the
standard staged-bootstrap pattern (a compiler's stage-0 is built in the base
language). Net fully-strict: 5 library crates; 3 bin crates are stage-0.

## Per-crate notes

### `trust-syntax` — STRICT (already)
56 LOC. Was already strict-marked. Still passes. No changes.

### `trust-diag` — STRICT
116 LOC. Added `trust_attrs::strict!{}` and the `trust-attrs`
dep. No call sites needed rewriting — the `Diagnostic::error(...)` /
`Report::build(...)` calls all live in callers, not in this file.

### `trust` (CLI) — STRICT
199 LOC. Found 4 real positional calls (`build(input, out, edition, no_lint)`,
3× `run_pipeline(label, source, skip_lints)`). All rewritten to named
form. No FPs.

### `trust-lsp` (bin)
The `lib.rs` was already strict-marked (RT-36 era). The `main.rs` was
not — added the marker and the `trust-attrs` dep. Builds clean: the
bin's only call site is `LspService::new(Backend::new)` (1-arity) and
`Server::new(stdin, stdout, socket).serve(service).await` (method-chain).

### `trust-rustc` (bin)
Added strict marker on `main.rs`. Found 3 real positional calls to
`run_rustc(rustc, &args)`. Rewritten. Builds clean.

The `lib.rs` was NOT marked — 435 LOC of mirror/cache/doctest plumbing
with many helpers that would surface RT-40/RT-41 (same as `trust-lints`).
Worth a follow-up once those land.

### `xtask`
Added strict marker. Found 6 positional calls (`check_one`, `walk_rs`,
`replace_section`). Rewritten. Builds clean.

### `trust-std` — STRICT (after RT-44)

RT-31 discovered the circular trap: `trust-lower/build.rs` was
parsing `crates/trust-std/src/lib.rs` directly with `syn::parse_file`
to build the bundled `STD_SIGNATURES` index (RT-32). `syn` rejects
named-arg syntax, and the build script *silently* wrote an empty
constant on parse failure — a particularly nasty silent footgun, because
the resulting workspace builds but every downstream cross-crate
named-arg call site fails at lower-time with no obvious connection back
to the empty manifest.

**RT-44 fix — option B (checked-in manifest):**
- Added `crates/trust-std/std-signatures.txt`, a hand-written
  manifest (one `name:p1,p2,…` line per `pub fn`). This is the source
  of truth that `build.rs` reads.
- `build.rs` no longer parses Rust source. Any read/parse failure now
  *panics* loudly (this also folds in option C's loud-failure mitigation
  — empty `STD_SIGNATURES` is unreachable by construction).
- `cargo xtask gen-std-signatures` regenerates the manifest from
  `lib.rs`. It calls `trust_lower::lower()` *first* to desugar any
  Trust-specific syntax, then walks the lowered `syn::File` for
  `pub fn` signatures. CI runs `cargo xtask gen-std-signatures --check`
  under the wrapper to verify the checked-in file is fresh.
- Decoupling `build.rs` from `lib.rs`'s dialect lets the std-shim crate
  be strict-marked. The `lib.rs` head now uses
  `trust_attrs::strict!{}` and the two two-arg fs shims (`copy`,
  `rename`) were rewritten to call `std::fs::copy(from: …, to: …)`.

**Surprise: generic-fn arity gap.** Two shims have generics
(`hashmap_insert<K, V>`, `vec_push<T>`). The `CalleeRegistry` token scan
in `trust-lower/src/named_args.rs` mis-handles `Vec<T>>` and
similar (the trailing `>>` has joint spacing, so `angle_depth` never
returns to zero, swallowing the following parameter commas). Symptoms:
local R0042 fires with the wrong arity, R3001 rejects valid param
names. The fix belongs in `split_by_top_comma`; for now, the smoke
tests are hoisted to a non-strict sibling file
`crates/trust-std/src/tests.rs` so they can call the generic shims
positionally without tripping the buggy registry. Worth filing as a
follow-up; the workaround keeps RT-44 on-scope.

### `trust-lints` — PARTIAL
`rules.rs` is strict-marked (just an enum, no calls).

`lib.rs`, `runner.rs`, and `strict.rs` are intentionally NOT marked:

- **`strict.rs`** has `Visit` impls that call `visit::visit_item_fn(this,
  node)` etc. The per-file callee registry collides the path-qualified
  `visit::visit_X` with the local impl method `fn visit_X(&mut self,
  node)` and reports the wrong arity. This is **RT-41** (already filed).
  Without path-aware resolution or `#[allow]`, ~20 R0042 FPs.
- **`runner.rs`** calls `crate::strict::run_rule(...)` and
  `crate::strict::detect_strict(...)` — fns defined in another file.
  The per-file registry doesn't know their params, so named-arg lowering
  can't strip names and rustc rejects the result. This is **RT-40**
  (already filed).
- **`lib.rs`** test module has 45+ positional calls to local helpers
  (`fires(Rule::X, src)`, `diags_for(Rule::X, src)`). All would be
  legitimate strict violations, but rewriting 45 call sites is >100 LOC
  per the stop condition. Worth a separate pass.

### `trust-attrs` — SKIPPED
Proc-macro crate. `#[proc_macro]` definitions cannot be strict-marked
(the macro IS the dialect activation point; marking it makes no sense).
The crate is 35 LOC of a single no-op `strict!` macro. Confirmed by
inspection rather than attempted strict-marking.

### `trust-lower` — NOT ATTEMPTED
This crate's own test suite uses `|>` syntax for pipe-operator
roundtrip tests. `scripts/fmt.sh` already skips it for that reason; the
same `|>` syntax would break the lowering pass when it tries to lower
itself (the test inputs are intentionally unlowered token streams). A
clean dogfood pass on `trust-lower` needs design work to split
"pipe-syntax test inputs" from "production source code".

## Gaps filed during this pass

- **RT-39** — *fixed inline*. `R0042` was inflating reported arity for
  fns with generic-type-parameter commas like `&mut HashMap<K, V>`. Added
  angle-bracket depth tracking to `split_by_top_comma` in
  `trust-lower/src/named_args.rs`.
- **RT-44** *(shipped — option B + C, see `trust-std` section
  above)*. `build.rs` now reads a checked-in
  `crates/trust-std/std-signatures.txt` manifest instead of
  re-parsing Rust source; the manifest is regenerated by
  `cargo xtask gen-std-signatures` (which lowers first, then `syn`s)
  and CI enforces freshness with `--check`. Any manifest read/parse
  failure panics loudly — the previous silent empty-`STD_SIGNATURES`
  footgun is unreachable.
- **RT-45** *(filed and fixed inline)* — `scripts/fmt.sh` only skipped
  packages containing `|>`; extended it to skip packages whose `src/`
  contains a strict-mode activation marker, since rustfmt also rejects
  named-arg syntax.
- **RT-46** *(filed)* — no `#[allow(trust::Rxxxx)]` mechanism. Once
  RT-40 / RT-41 are fixed there should still be a per-callsite escape
  hatch for legitimate exceptions (e.g. macro-generated code, FFI
  boundary types). The current strict-mode design has no allowlisting
  beyond the file-level `#[strict::macros_ok]`.

## CI step

Added `dogfood — build strict crates under wrapper` to surface
regressions early. See `.github/workflows/`.

## Bottom line

7 of the 11 `trust-*` crates are now fully strict-marked and build
under `RUSTC_WRAPPER` (after RT-44). The remaining 4 are blocked by:

1. **RT-40** (cross-file callee resolution) — blocks the bulk of
   `trust-lints`.
2. **RT-41** (path-aware callee matching for method calls) — blocks
   `trust-lints/strict.rs` and any visitor-heavy code.
3. **Proc-macro crates** can't be strict by design.
4. **`trust-lower`** has bidirectional roles (defines `|>`, tests
   `|>`); needs a tests-only carveout.

Most of those gaps were known going in (RT-40, RT-41 pre-existed). RT-44
was shipped as part of this case-study iteration; RT-46 is still
outstanding. RT-39 was caught and fixed in passing during RT-31.

## Update (RT-82) — `strict!{}` removed; dogfood moves to project-level opt-in

The `trust_attrs::strict!{}` per-file marker (and the whole `trust-attrs`
crate) was removed in RT-82. Activation is now `#![strict]` per file
(wrapper-built code only — stock rustc rejects it) or
`[package.metadata.trust] strict = true` per package, discovered by
`cargo trust` from the workspace manifest.

That trade has a real cost recorded here honestly: per-file granularity for
**stock-buildable library crates** is gone. A library that must keep
compiling under plain `cargo test` (stage-0 CI, downstream consumers)
cannot satisfy whole-package R0042 when any of its files — in practice the
`#[cfg(test)]` siblings — call its own multi-arg fns positionally, because
the fix (named args) is syntax stock rustc can't parse.

Post-RT-82 status:

| Crate | Whole-package strict? | Blocker |
| ----- | --------------------- | ------- |
| `trust-syntax` | **YES** — `[package.metadata.trust]`, enforced in CI | — |
| `trust-diag`   | no | `tests.rs` calls `line_col(src, 0)` positionally |
| `trust-std`    | no | `tests.rs` calls `duration`/`hashmap_insert` positionally |
| `trust-lsp`    | no | lib internals: positional calls to `compute_hover` etc. |
| `trust-lints`  | no | lib internals + test helpers |
| `trust`, `trust-rustc`, `xtask` | no (by design) | stage-0 bootstrap (RT-76) |

RT-88 tracks restoring the lost coverage (cfg(test)-aware force-strict, a
publish-lowered pipeline, or R0042-clean refactors). RT-87 (closure args
break named-arg call parsing) currently blocks `heck-strict` regardless of
activation mechanism.
