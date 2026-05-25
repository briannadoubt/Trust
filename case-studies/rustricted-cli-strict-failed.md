# Failed dogfood: `crates/rustricted/` CLI

## Update — gap closed for single-file crates

The architectural gap this writeup identified ("cargo can't build crates
that use Rustricted's syntax extensions") was closed by `crates/rustricted-rustc/`,
a `RUSTC_WRAPPER` shim that runs the lowering pass on each strict-marked
`.rs` file before invoking the real rustc. End-to-end demonstrated by
`examples/cargo-strict-fixture/`: `RUSTC_WRAPPER=$(realpath
target/debug/rustricted-rustc) cargo build` succeeds on a file using
named-arg syntax that stock rustc rejects.

**Scope of the unblock.** Only the input `.rs` file passed to rustc is
lowered. Child modules pulled in via `mod foo;` are loaded by rustc from
the original on-disk paths and are NOT lowered. So a multi-file crate
where `lib.rs` declares `mod helpers;` and `helpers.rs` uses named args
will still fail. Re-doing this CLI dogfood requires extending the
wrapper to walk the crate's full module tree, or restricting the
dialect's extensions to the crate root file.

What follows is the original writeup, kept as historical context for the
architectural reasoning.

---

## Outcome

Reverted. The CLI **cannot be self-hosted under cargo + R0042 simultaneously**
with the current Phase 0 architecture. This is a real design gap that the
previous (small) dogfood pass on `rustricted-syntax` did not surface.

## What happened

The previous dogfood subagent recommended this crate as "the first
internal crate likely to exceed 5 honest violations." A follow-up subagent
attempt stalled mid-conversion (commit `9158f68` accidentally committed
its partial state: marker macro added, no fixes applied). I picked up the
work, added `rustricted_attrs::strict!{}` to the top, ran
`rustricted check`, and saw 8+ R0042 violations on legitimate multi-arg
local calls (`build`, `run_pipeline`).

Tried to fix them with named-arg syntax — `run_pipeline(input: input,
source: &source, skip_lints: false)`. **rustc rejected the file**:

```
error: expected one of `)`, `,`, `.`, `?`, `}`, or an operator,
       found `:`
   --> crates/rustricted/src/main.rs:96:36
    |
 96 |     let _ = run_pipeline(input: input, source: &source, ...
    |                                ^ expected one of ...
```

## Root cause

`#![strict]` and `rustricted_attrs::strict!{}` only activate the
toolchain's **lints**. The toolchain's **syntax extensions** (named args,
pipe operator, `effect` keyword) require the Rustricted **lowering
pass** to rewrite them into plain Rust before rustc sees the file. The
lowering pass runs only when the toolchain owns the build pipeline
(`rustricted check`, `rustricted build`). `cargo build` invokes rustc
directly, with no lowering step. Therefore syntax-extension forms never
reach rustc as legal source.

For a crate that has `R0042` violations (any function with arity > 1
declared in the same file as its call sites), there is no way to fix
them under `cargo build` without one of:

1. A cargo-level integration (`RUSTC_WRAPPER` shimming `rustricted lower`
   in front of rustc). Standard pattern, real engineering work.
2. The `rustricted_attrs::strict!{}` macro becomes a full preprocessor
   that lowers its enclosing file's content. Possible but heavy — and
   proc-macros can only modify their own invocation site, not the
   surrounding file.
3. A `build.rs` that pre-lowers `.rs` files into a `target/` location
   before cargo compiles. Hacky and breaks editor / rust-analyzer
   integration.
4. Use `#[allow(no_positional_args)]` per call site. Not currently
   supported (R0042 emits from the lowering pass, not the lint runner,
   and has no allow mechanism wired up).

None of these are in scope for Phase 0.

## What `rustricted-attrs` *does* still buy

The proc-macro is not useless. Cargo-built crates that:

- have **no** arity > 1 calls to local functions (so R0042 never fires), AND
- want the AST-level lints (R0001 .unwrap, R0003 as cast, R0004 glob,
  R0007 impl-trait-return, R0010 todo, R0011 panic, R0012 bool-param,
  R0014 bare-index, R0005/R0006 justify-{unsafe,allow}, R0008 user-macros)

can opt in with `rustricted_attrs::strict!{}` and get those lints. This is
the slice that worked for `rustricted-syntax` (60 LOC, all arity-1
calls). It is a real but narrow subset.

## Honest verdict

The dialect is currently **two languages with one name**:

- **Single-file Rustricted** (`rustricted check` / `rustricted build`):
  full dialect, including named args / pipe / effects, plus all lints.
  Works for examples and the eval suite.
- **Cargo Rustricted** (`rustricted_attrs::strict!{}`): lints only. The
  syntax extensions are inaccessible. Suitable for crates whose call
  graph happens to be free of arity-2+ local-function calls.

Calling the cargo-mode subset "Rustricted" without that caveat oversells
it. SPEC.md should be updated to make the activation-vs-feature distinction
explicit, or the cargo activation should be renamed (e.g.
`rustricted_attrs::lints!{}`) to signal it's the lint-only subset.

## Recommended follow-ups

- **High value, medium effort**: build the cargo `RUSTC_WRAPPER`
  integration so `cargo build` can invoke Rustricted lowering before
  rustc. Unlocks the full dialect for cargo crates.
- **Low value, low effort**: rename the proc-macro export to
  `lints!{}` (or add it alongside `strict!{}` and deprecate the old
  name) so the activation name reflects the actual capability.
- **Just-document**: update SPEC.md activation section to call out the
  cargo-vs-single-file capability split.

The takeaway: Phase 0's self-hosting story stops at "lint a single
file." The full-dialect self-hosting story requires Phase-1-or-later
build integration work.
