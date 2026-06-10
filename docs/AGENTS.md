# AGENTS.md — Working on Trust as an LLM

Audience: a future-you agent walking into this codebase cold, asked to add a
lint or extend the lowering pipeline. The goal of this document is to make
the first PR boring.

If you only read one other doc, read `SPEC.md`. If you read two, also read
`RATIONALE.md`. This file is the workflow primer; the others are the
substrate.

## Project layout

```
Cargo.toml                 # workspace root; pins workspace.dependencies
rust-toolchain.toml        # pinned stable
README.md
.github/workflows/ci.yml   # fmt + clippy + test + round-trip example
crates/
  trust/              # CLI driver: `trust build|check|lower`
  trust-syntax/       # parse + roundtrip; identity round-trip in Phase 0
  trust-lower/        # named-args and pipe token passes
  trust-lints/        # the `#![strict]` lint preset (R0001–R0008)
  trust-diag/         # Diagnostic shape + ariadne renderer
  trust-lsp/          # tower-lsp stub (Phase 5)
  trust-std/          # named-arg-friendly std shims
  cargo-trust/        # `cargo trust` subcommand wrapper
examples/
  00-hello.rs              # the round-trip smoke test
tests/                     # (planned: ui/, snapshots/, golden/)
docs/
  SPEC.md                  # language reference
  RATIONALE.md             # why each rule exists
  AGENTS.md                # this file
```

`cargo-trust` is a one-file binary that strips the `trust` argv prefix cargo
prepends, then either runs `cargo` with the `trust-rustc`/`trust-rustdoc` shims
wired in (for `build`/`run`/`test`/… — so users need no `RUSTC_WRAPPER` env
setup) or forwards the rest to the `trust` CLI (for `lower`/`explain`/…). See
SPEC.md § `cargo trust`. You will rarely need to touch it.

Examples and tests live at the workspace root, not per-crate, on purpose:
they exercise the full pipeline.

## The Trust way

When you write Rust _inside_ this workspace, you eat the dialect. The crate
roots will grow `#![strict]` as Phase 1 lands. Until then, the lints are
informational. Write as if they were enforced anyway:

- **Every fn declares its effects.** Even if the parser doesn't accept the
  clause yet, leave a doc comment naming the effect set. Effects coming in
  Phase 4 — see `SPEC.md#effect-keyword`.
- **Named args past arity 1.** Enforced for in-crate callees, and for
  cross-crate callees whose signature index is loaded via
  `TRUST_SIGNATURE_PATH` (generate one with `trust index`; RT-66). Calls
  into an unindexed dependency stay positional.
- **No `.unwrap()` outside tests.** Use `?`, `.expect("…")` with a real
  message, or restructure to return `Result`. See R0001.
- **`// safety:` for `unsafe`.** Every block, every `unsafe fn`. R0005.
- **`// reason:` for `#[allow]`.** Every attribute, no exceptions. R0006.
- **No `use foo::*`.** Enumerate. R0004.
- **No `as` casts.** `try_into()` for numerics, `.cast()` for pointers.
  R0003.
- **No user macros without opt-in.** R0008. The allowlist covers everything
  you actually want.

Rule codes are stable. When in doubt, grep `crates/trust-lints/src/rules.rs`.

## The teaching-error contract

Every `Diagnostic` this codebase emits **must** include three things:

1. A stable rule code in the banner (`error[R0001]: ...`).
2. A `why:` note — one sentence on why the rule exists.
3. A `help:` line carrying a literal replacement when one is available.

The shape is enforced by `trust_diag::Diagnostic` and its `.with_why()`
/ `.with_help()` builders (see `crates/trust-diag/src/lib.rs`). The
renderer is `trust_diag::render`, which formats via `ariadne`.

**Optionally** attach a structured, machine-applicable fix with
`.with_fix(Fix::new(span, replacement, applicability))` (RT-70). Prefer one
whenever the replacement is deterministic — it is what an agent loop applies
without re-parsing the prose `help`. Be honest with `Applicability`:
`Automatic` only for semantics-preserving rewrites, `MaybeIncorrect` when it
depends on context the lint can't see (e.g. `.unwrap()` → `?` assumes the fn
returns `Result`), `HasPlaceholders` when the replacement contains `...`.
Fixes surface in `trust check --format json` (`trust_diag::to_json`).

When you add a new lint, copy the pattern from an existing one. Do not skip
`why` or `help`. The agent reading your diagnostic in production has no
other context; the diagnostic _is_ the documentation.

## Adding a new lint

Phase 1 work. Step-by-step:

1. **Pick a code.** Find the next free `R00NN` in
   `crates/trust-lints/src/rules.rs`. Add a new `Rule` variant, extend
   the `code()`, `name()`, and `rationale()` match arms, and add the variant
   to `ALL`.
2. **Document the rule in `SPEC.md`.** Append a `### R00NN — name` subsection
   under the `## Lints` heading. Use the existing entries as templates:
   bug example, accepted form, escape hatch.
3. **Write the rationale in `RATIONALE.md`.** Bug class, why this shape,
   tradeoffs, escape hatch. Don't be defensive about the tradeoffs; concede
   them.
4. **Implement the visitor in `crates/trust-lints/src/strict.rs`.**
   `run_rule` currently dispatches a no-op. Add a match arm that delegates
   to a per-rule function. The function takes `(&syn::File, &str,
   &mut Vec<Diagnostic>)` and walks the AST with `syn::visit::Visit`.
   Emit `Diagnostic::error("R00NN", message, span).with_why(...).with_help(...)`
   for each violation.
5. **Add a UI test.** Create `tests/ui/r00NN_<name>.rs` with a fixture that
   triggers the rule. Snapshot the diagnostic output with `insta`. _(Phase 1
   will introduce the test harness; if it isn't there yet, add the fixture
   and write a unit test in `lib.rs` instead.)_
6. **Update the catalogue in `SPEC.md`.** The table at the top of `## Lints`
   needs the new row.
7. **Run the test suite.** `cargo test --workspace` and `cargo clippy
   --workspace --all-targets -- -D warnings`.

When in doubt about diagnostic copy, mirror the prose style of existing
rules: imperative verb, no jargon, explain the fix in concrete code.

## Running tests

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p trust -- build examples/00-hello.rs --out /tmp/hello && /tmp/hello
cargo run -p trust -- build examples/02-pipe/chain.rs   # (Phase 2)
cargo run -p trust -- check examples/01-lints/unwrap.rs # (Phase 1)
cargo run -p trust -- lower examples/03-named-args/swap.rs # (Phase 3)
```

CI runs the first two plus a round-trip of `examples/00-hello.rs`. See
`.github/workflows/ci.yml`. The CI job is the source of truth for what
"green" means; reproduce it locally before pushing.

`RUSTFLAGS=-D warnings` is set in CI, so any warning fails the build. The
strict-mode lints will be additive on top of that once they go live.

## Pitfalls the codebase has caught

- **Positional argument ordering.** R0001-R0008 in `SPEC.md`. The exact
  motivating case for the named-args extension. Catch it at the agent's
  authoring time, not at someone else's debugging time.
- **`.unwrap()` reflex.** R0001. The single most common LLM authoring
  mistake in this codebase's training corpus. Use `?`.
- **Glob imports.** R0004. Especially `use crate::types::*;` in test
  modules. Enumerate.
- **Missing effect declarations.** Phase 4. When a fn calls `std::fs` and
  forgets to declare `effect io`, the check pass will flag it at the
  call site, not the fn declaration. Annotate up front; don't wait for the
  pass.
- **`as` for narrowing.** R0003. Always reach for `.try_into()` instead.
  If you genuinely need truncation, write `#[allow(no_as_cast)] // reason:
  <invariant>`.
- **Empty `.expect("")`.** R0002. There is no reason. Write a real message.
- **Forgetting `// safety:` on a new `unsafe` block.** R0005. The lint
  fires on the block, not on the surrounding fn — easy to miss when
  refactoring.
- **`#[allow]` without a reason.** R0006. The comment is the rule. Without
  it, the suppression is invisible.

When you finish a change, before opening the PR: re-read your diff for
these patterns. The codebase will eventually enforce them; the version of
you that runs in two weeks will thank the version of you that runs today.
