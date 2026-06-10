---
name: writing-trust
description: "Write, fix, or convert Rust in a project that uses Trust, the strict Rust dialect. Use when you see `cargo trust` in build instructions, `[package.metadata.trust] strict = true` (or `[workspace.metadata.trust]`) in a Cargo.toml, a `#![strict]` inner attribute at the top of a .rs file, build errors with R-codes like R0042/R0001/R0014 (`error[R0042]: call to ... must use named arguments`), named-argument call syntax such as `f(width: 1920, height: 1080)`, the pipe operator `|>`, or `requires!(...)` preconditions in function bodies. Also use when asked to convert an existing crate to Trust or scaffold a new Trust project with `trust new`."
---

# Writing Trust

## What Trust is

Trust is a strict dialect of Rust: 20 hard lints (R-codes) plus three syntax extensions, lowered to plain Rust before rustc sees it. Build with `cargo trust build` (or `trust build` for single files) — **never plain `cargo`**: stock rustc rejects the dialect syntax and skips the lints.

## Project setup

- **Greenfield:** `trust new <name>` scaffolds a strict project (Cargo.toml with the metadata key, named-arg hello-world, .gitignore, README). Build it with `cargo trust build`.
- **Existing crate:** add two lines to `Cargo.toml`:

  ```toml
  [package.metadata.trust]
  strict = true
  ```

- **Workspace-wide:** `[workspace.metadata.trust] strict = true` in the root manifest opts in every member. Dependencies are never affected.
- **Mixed crates / single files:** put `#![strict]` as an inner attribute at the top of a file to opt in file-by-file. Stock rustc rejects `#![strict]`; only the Trust toolchain compiles it (the wrapper strips it during lowering).

## The syntax you must write

- **Named arguments on every call with arity > 1** to a function Trust can see (in-crate, or indexed via `TRUST_SIGNATURE_PATH`):

  ```rust
  fn make_rect(width: u32, height: u32) -> Rect { /* … */ }

  make_rect(width: 1920, height: 1080)   // required — never make_rect(1920, 1080)
  make_rect(height: 1080, width: 1920)   // order is free; names are checked,
                                         // lowering reorders to declaration order
  ```

  No mixing named and positional in one call. Arity-0/1 calls and calls into unindexed dependencies stay positional. A wrong name is error R3001.
- **Pipe operator:** `e |> f(args)` lowers to `f(e, args)` — the receiver becomes the first argument.

  ```rust
  let s = read_input()? |> normalize(mode: Mode::Strict) |> render();
  // == render(normalize(read_input()?, mode: Mode::Strict))
  ```

  Works with paths (`e |> path::to::f(a)`) and named args. Binds lower than `.`/field/indexing, left-associative. Keep method chains as `.method()`; the pipe is for free functions.
- **Preconditions:** `requires!(cond)` at the top of a strict fn body lowers to `debug_assert!(cond, "requires violated: …")` — checked in debug builds, gone in release. No `ensures!`, no solver.

  ```rust
  fn withdraw(balance: u64, amount: u64) -> u64 {
      requires!(amount <= balance);
      balance - amount
  }
  ```

**Important:** this syntax only compiles via `cargo trust` / `trust build`. Plain `cargo build` failing on `f(width: 1920, ...)` is *expected behavior*, not a bug — do not "fix" it by deleting the names; switch the build command instead.

## The iteration loop

1. `cargo trust build --message-format json` — Trust diagnostics arrive on **stderr** as one JSON document per failing file (same shape as `trust check --format json`):

   ```json
   {
     "version": "0.1",
     "file": "src/main.rs",
     "diagnostics": [
       {
         "rule": "R0042",
         "severity": "error",
         "message": "call to `make_rect` must use named arguments (arity 2)",
         "span": {"start": 50, "end": 62, "startLine": 2, "startColumn": 40,
                  "endLine": 2, "endColumn": 52},
         "why": "positional argument ordering is the largest LLM-authored bug class…",
         "help": "rewrite as `make_rect(width: ..., height: ...)`",
         "fix": {"span": {"...": "..."}, "replacement": "(width: ..., height: ...)",
                 "applicability": "hasPlaceholders"}
       }
     ]
   }
   ```

   Spans are byte offsets plus 1-based line/column. `why`, `help`, `fix` may be `null`.
2. Apply the fix. `applicability: "automatic"` fixes are safe to apply unattended; `"maybeIncorrect"` needs review; `"hasPlaceholders"` contains `...` you must fill. Otherwise follow the `help`/`instead` guidance literally.
3. Rebuild. Repeat until clean (exit code is non-zero while any error remains).

- For a file full of R0042 violations, `trust fix <file> --write` mechanically inserts the `name:` prefixes (idempotent; only the names are spliced in). Pass `-` to read stdin.
- `trust explain <CODE>` explains any rule (why + what to write instead); with no code it prints the whole catalogue. `--format json` for machine consumption.
- Single-file lint without cargo: `trust check foo.rs --format json`. Note `cargo trust check` resolves to whole-crate `cargo check` instead.

Command quick reference:

```sh
trust new <name>                            # scaffold a strict project
cargo trust build|run|test                  # cargo lifecycle through the shims
cargo trust build --message-format json     # machine-readable diagnostics on stderr
trust check <file.rs> [--format json]       # lower + lint one file, no rustc
trust fix <file.rs> --write                 # insert named args in place
trust explain [<CODE>] [--format json]      # rule catalogue / single rule
trust lower <file.rs>                       # print the lowered plain Rust
trust index <src-dir> --out sigs.txt        # signature index for a dependency
```

## Rule table

<!-- Generated from `cargo run -q -p trust -- explain --format json` (catalogue v0.1)
     at authoring time. When rules change, re-run that command and refresh this
     table — it is the source of truth. -->

"Test-exempt" = the rule does not fire inside `#[cfg(test)]` modules / `#[test]` fns. In project mode, files reachable only through a `#[cfg(test)] mod` are entirely exempt from all rules (RT-88).

| Code | Name | Write this instead | Test-exempt |
|------|------|--------------------|-------------|
| R0001 | no-unwrap | propagate with `?`, or `.expect("why this can't fail")` | yes |
| R0002 | empty-expect | give `.expect("…")` a real message explaining why it can't fail | no |
| R0003 | no-as-cast | use `T::try_from(x)?` for fallible casts, or `.into()` for widening | no |
| R0004 | no-glob-import | import the specific items: `use foo::{A, B};` | no |
| R0005 | justify-unsafe | precede the `unsafe` block with a `// safety: …` comment | no |
| R0006 | justify-allow | precede the `#[allow(…)]` with a `// reason: …` comment | no |
| R0007 | no-impl-trait-return | name the type with a `type Alias = …;` and return the alias | no |
| R0008 | no-user-macros | inline the logic, or opt in with `#[strict::macros_ok]` | no |
| R0010 | no-todo-macro | finish the implementation, or return a typed `Err` | yes |
| R0011 | no-panic | return a typed `Err` and let the caller decide whether to abort | yes |
| R0012 | no-bool-param | replace the `bool` with a named enum, e.g. `enum Mode { On, Off }` (private fns also exempt) | yes |
| R0014 | no-bare-index | use `.get(i)` and handle the `Option` (slicing `v[a..b]` is exempt) | yes |
| R0015 | allow-missing-reason | add a `reason = "…"` argument to the `#[allow(trust::…)]` | no |
| R0016 | allow-unknown-code | use a real rule code (run `trust explain` for the catalogue) | no |
| R0017 | no-same-type-params | wrap each in a distinct newtype — `trust_std::newtype!(pub Width(u32));` | yes |
| R0018 | error-context-dropped | carry the source: `.map_err(|e| MyError::Io(e))`, or use `?` with a `From` impl | yes |
| R0019 | no-unchecked-len-arith | make the choice explicit: `.checked_sub(1)?`, `.saturating_sub(1)`, or `.wrapping_*` if wrap is intended | yes |
| R0020 | no-lock-across-await | drop the guard before awaiting (scope it in a block), or use an async-aware lock like `tokio::sync::Mutex` | no |
| R0021 | no-capacity-as-len | use `.len()` for element counts; `.capacity()` only sizes future allocations | no |
| R0042 | no-positional-args | name the arguments — `f(width: …, height: …)` — or run `trust fix` | no |

All 20 rules are severity **error**. Codes outside R00xx come from lowering, not the lint runner: R2001 (pipe needs a path-call on the right), R3001 (no parameter with that name).

## Pitfalls

- Tests are exempt via `#[cfg(test)]` — do not rewrite test code into dialect syntax; stock `cargo test` must still parse it.
- `#[allow(trust::R0xxx, reason = "…")]` works ONLY in code built exclusively via `cargo trust` — stock rustc rejects tool-scoped allows, so published/stock-buildable crates must fix the violation, not suppress it.
- The `reason = "…"` is mandatory on every `trust::` allow (R0015 fires without it); a `// reason: …` comment must also precede any `#[allow(…)]` at all (R0006).
- `// safety:` (R0005) and `// reason:` (R0006) comment contracts go in the contiguous comment block **directly above** the site — a blank line breaks the association:

  ```rust
  // safety: ptr is non-null; checked by the guard two lines up
  unsafe { ptr.read() }

  // reason: Slab key, not a length-bounded index
  #[allow(trust::R0014, reason = "Slab key, not usize")]
  fn get(&self, key: Key) -> &Entry { /* … */ }
  ```
- `cargo trust build --message-format json` is Trust's flag, not cargo's — cargo's own `--message-format` takes different values; the wrapper strips the flag and sets `TRUST_MESSAGE_FORMAT=json`. Setting that env var directly is equivalent.
- R0012 has no `#[allow]` escape hatch at all: make the fn private, use an enum, or move it under `#[cfg(test)]`.
- Cross-crate calls into unindexed dependencies are positional by design — don't invent names for `std`/third-party fns unless `trust-std` or a `trust index` manifest covers them.

## Escalation

If a rule looks wrong for your case, read `case-studies/` and the rule's section in `docs/SPEC.md` **before** suppressing — most "false positives" have a documented compliant idiom. If suppression is genuinely right, it requires a written `reason = "…"` (and `// reason:` comment), and it only survives in code that is never built with stock cargo.
