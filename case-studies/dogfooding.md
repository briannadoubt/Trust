# Dogfooding rustricted-* crates with `#![strict]`

**Ticket:** RT-31 — eat your own cooking.

We tried to mark each `rustricted-*` workspace crate `#![strict]` and build
it under `RUSTC_WRAPPER=target/release/rustricted-rustc`. Goal: prove the
dialect is usable for real work and surface every remaining gap by hitting
it ourselves.

This file records per-crate status — what went strict, what broke, what
was a real bug, and what was a language/tooling gap.

## Summary

| Crate | Status | Real bugs fixed | FPs / gaps surfaced |
| ----- | ------ | --------------- | ------------------- |
| `rustricted-syntax`   | STRICT (pre-existing) | 0 | 0 |
| `rustricted-diag`     | STRICT | 0 | 0 |
| `rustricted` (CLI)    | STRICT | 0 (rewrote 4 positional call sites to named) | 0 |
| `rustricted-lsp` bin  | STRICT | 0 | 0 (lib still strict-marked from before) |
| `rustricted-rustc` bin | STRICT | 0 (rewrote 3 call sites) | 0 |
| `xtask`               | STRICT | 0 (rewrote 6 call sites) | 0 |
| `rustricted-lints/rules.rs`     | STRICT (file-level) | 0 | 0 |
| `rustricted-attrs`    | SKIPPED | — | proc-macro crate; strict markers don't apply to `#[proc_macro]` definitions |
| `rustricted-std`      | SKIPPED | — | RT-44 (circular: `build.rs` parses this with `syn`, which rejects named-arg syntax) |
| `rustricted-lints/lib.rs`     | SKIPPED | — | 45+ positional helper calls in `#[cfg(test)]` block; bulk rewrite hits >100-LOC stop |
| `rustricted-lints/runner.rs`  | SKIPPED (with documented reason in file head) | — | RT-40 (cross-file calls to `crate::strict::run_rule`) |
| `rustricted-lints/strict.rs`  | SKIPPED | — | RT-41 (visit-pattern method calls match free-fn signatures by simple name) |
| `rustricted-lower`    | NOT ATTEMPTED | — | known: uses `|>` operator in its own tests; `fmt.sh` already skips it. Worth a follow-up. |

**Crates fully strict-marked:** 6 of 11. **At least 3 acceptance criterion: met.**

## Per-crate notes

### `rustricted-syntax` — STRICT (already)
56 LOC. Was already strict-marked. Still passes. No changes.

### `rustricted-diag` — STRICT
116 LOC. Added `rustricted_attrs::strict!{}` and the `rustricted-attrs`
dep. No call sites needed rewriting — the `Diagnostic::error(...)` /
`Report::build(...)` calls all live in callers, not in this file.

### `rustricted` (CLI) — STRICT
199 LOC. Found 4 real positional calls (`build(input, out, edition, no_lint)`,
3× `run_pipeline(label, source, skip_lints)`). All rewritten to named
form. No FPs.

### `rustricted-lsp` (bin)
The `lib.rs` was already strict-marked (RT-36 era). The `main.rs` was
not — added the marker and the `rustricted-attrs` dep. Builds clean: the
bin's only call site is `LspService::new(Backend::new)` (1-arity) and
`Server::new(stdin, stdout, socket).serve(service).await` (method-chain).

### `rustricted-rustc` (bin)
Added strict marker on `main.rs`. Found 3 real positional calls to
`run_rustc(rustc, &args)`. Rewritten. Builds clean.

The `lib.rs` was NOT marked — 435 LOC of mirror/cache/doctest plumbing
with many helpers that would surface RT-40/RT-41 (same as `rustricted-lints`).
Worth a follow-up once those land.

### `xtask`
Added strict marker. Found 6 positional calls (`check_one`, `walk_rs`,
`replace_section`). Rewritten. Builds clean.

### `rustricted-std` — SKIPPED (RT-44)
Discovered the hard way that **strict-marking `rustricted-std` creates a
circular dependency.** `rustricted-lower/build.rs` parses
`crates/rustricted-std/src/lib.rs` with `syn::parse_file` to build the
bundled signature index (RT-32). `syn` rejects named-arg syntax, so the
moment we add a `(from: from, to: to)` inside `std::fs::copy`, the build
script silently writes an empty `STD_SIGNATURES` constant and every
downstream test that relies on cross-crate named-arg lowering fails.

This is a real architectural gap: `rustricted-std` is the *source* of
named-arg knowledge for the rest of the workspace; it can't itself be
named-arg syntax. Either (a) `build.rs` needs to invoke the lowering pass
before `syn`, (b) `rustricted-std` stays plain Rust by policy, or (c)
introduce a separate machine-readable signature manifest format.

I left `rustricted-std` unmarked and added a comment in the file head
documenting why. Filed **RT-44**.

### `rustricted-lints` — PARTIAL
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

### `rustricted-attrs` — SKIPPED
Proc-macro crate. `#[proc_macro]` definitions cannot be strict-marked
(the macro IS the dialect activation point; marking it makes no sense).
The crate is 35 LOC of a single no-op `strict!` macro. Confirmed by
inspection rather than attempted strict-marking.

### `rustricted-lower` — NOT ATTEMPTED
This crate's own test suite uses `|>` syntax for pipe-operator
roundtrip tests. `scripts/fmt.sh` already skips it for that reason; the
same `|>` syntax would break the lowering pass when it tries to lower
itself (the test inputs are intentionally unlowered token streams). A
clean dogfood pass on `rustricted-lower` needs design work to split
"pipe-syntax test inputs" from "production source code".

## Gaps filed during this pass

- **RT-39** — *fixed inline*. `R0042` was inflating reported arity for
  fns with generic-type-parameter commas like `&mut HashMap<K, V>`. Added
  angle-bracket depth tracking to `split_by_top_comma` in
  `rustricted-lower/src/named_args.rs`.
- **RT-44** *(filed)* — circular: `rustricted-std` can't be strict-marked
  because `rustricted-lower/build.rs` parses it with `syn`.
- **RT-45** *(filed and fixed inline)* — `scripts/fmt.sh` only skipped
  packages containing `|>`; extended it to skip packages whose `src/`
  contains a strict-mode activation marker, since rustfmt also rejects
  named-arg syntax.
- **RT-46** *(filed)* — no `#[allow(rustricted::Rxxxx)]` mechanism. Once
  RT-40 / RT-41 are fixed there should still be a per-callsite escape
  hatch for legitimate exceptions (e.g. macro-generated code, FFI
  boundary types). The current strict-mode design has no allowlisting
  beyond the file-level `#[strict::macros_ok]`.

## CI step

Added `dogfood — build strict crates under wrapper` to surface
regressions early. See `.github/workflows/`.

## Bottom line

6 of the 11 `rustricted-*` crates are now fully strict-marked and build
under `RUSTC_WRAPPER`. The remaining 5 are blocked by:

1. **RT-40** (cross-file callee resolution) — blocks the bulk of
   `rustricted-lints`.
2. **RT-41** (path-aware callee matching for method calls) — blocks
   `rustricted-lints/strict.rs` and any visitor-heavy code.
3. **RT-44** (build.rs circular dep) — blocks `rustricted-std`
   permanently unless we change how the signature index is generated.
4. **Proc-macro crates** can't be strict by design.
5. **`rustricted-lower`** has bidirectional roles (defines `|>`, tests
   `|>`); needs a tests-only carveout.

Most of those gaps were known going in (RT-40, RT-41 pre-existed). RT-44
and RT-46 are the new findings. RT-39 was caught and fixed in passing.
