# Rustricted Language Reference

Status: Phase 0 driver shipped. Lints, syntax extensions, and effect tracking
are stubbed in-tree and land in subsequent phases. Items marked _unimplemented_
or _Phase N_ are specified here but not yet enforced by the toolchain.

## Overview

Rustricted is a strict dialect of Rust aimed at LLM-authored code. It bans a
short list of patterns that agents misuse on first contact and adds three
genuine grammar extensions: mandatory named arguments past arity 1, a pipe
operator `|>`, and an `effect` keyword for tracking side effects in function
signatures.

Rustricted is not a fork of `rustc` and not a new language family. The
toolchain is a frontend that lowers Rustricted source to plain Rust source and
hands the result to `rustc` via `cargo`. The pipeline is:

```
source → token stream → lowering passes → syn::File → prettyplease → rustc
```

Lowering is token-level (`proc_macro2::TokenStream`) until the extensions have
been desugared, at which point `syn` parses the result and `prettyplease` emits
the final Rust. The driver lives in `crates/rustricted`; the orchestration is
`rustricted_lower::lower`.

## Activation

Rustricted is opt-in at the crate root via an inner attribute:

```rust
#![strict]
```

Lints in the `rustricted-lints` crate consult this attribute via
`detect_strict` and return an empty report when it is absent. Lowering passes
(pipe, named-args, effects) run unconditionally because they are pure rewrites,
but in a crate without `#![strict]` they have nothing to rewrite — pipe and
`effect` are syntax errors in vanilla Rust, and positional calls remain
positional.

In practice: crates without `#![strict]` round-trip through the driver
unchanged. Treat the attribute as a hard switch.

## Lints

Lint codes are stable. Each lint is emitted as a `Diagnostic` carrying the rule
code, a primary message, a `why:` note, and a `help:` suggestion. See the
[Diagnostic format](#diagnostic-format) section for the rendered shape.

| Code  | Name                  | Phase | Severity |
| ----- | --------------------- | ----- | -------- |
| R0001 | no-unwrap             | 1     | error    |
| R0002 | empty-expect          | 1     | error    |
| R0003 | no-as-cast            | 1     | error    |
| R0004 | no-glob-import        | 1     | error    |
| R0005 | justify-unsafe        | 1     | error    |
| R0006 | justify-allow         | 1     | error    |
| R0007 | no-impl-trait-return  | 1     | _unimplemented_ |
| R0008 | no-user-macros        | 1     | error    |

Rule metadata lives in `crates/rustricted-lints/src/rules.rs`.

### R0001 — no-unwrap

`.unwrap()` is banned outside `#[cfg(test)]` modules and `#[test]` functions.

Rationale: panics on `None` / `Err` are silent control flow; agents reach for
`.unwrap()` reflexively.

```rust
// rejected
fn load(path: &Path) -> Config {
    let raw = std::fs::read_to_string(path).unwrap();
    toml::from_str(&raw).unwrap()
}

// accepted
fn load(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&raw)?)
}
```

Escape hatch: move the call under `#[cfg(test)]` or attach
`#[allow(no_unwrap)] // reason: ...` to the enclosing item.

### R0002 — empty-expect

`.expect("")` with an empty (or whitespace-only) message is banned everywhere.

Rationale: empty messages defeat the point of `.expect()` — the message is the
audit trail.

```rust
// rejected
let port = env::var("PORT").expect("");

// accepted
let port = env::var("PORT").expect("PORT must be set; see deploy/README.md");
```

No escape hatch. Write a real message.

### R0003 — no-as-cast

The `as` keyword is banned in expression position. Use `TryFrom` / `try_into`
for numeric conversions, `From` for widening, and explicit pointer-cast helpers
for `*const`/`*mut`.

Rationale: `as` silently truncates and is a frequent source of integer-overflow
bugs.

```rust
// rejected
let small: u8 = big as u8;

// accepted
let small: u8 = big.try_into()?;
```

Escape hatch: `#[allow(no_as_cast)] // reason: lossy conversion is intentional
because <…>` on the enclosing item.

### R0004 — no-glob-import

`use foo::*;` is banned. Enumerate the symbols you actually use, or fully
qualify call sites.

Rationale: glob imports hide which symbols are in scope and let unrelated
changes affect resolution.

```rust
// rejected
use std::collections::*;

// accepted
use std::collections::{BTreeMap, HashMap};
```

No general escape hatch. Prelude re-exports inside `rustricted-std` count as
glob imports and must be itemised the same way.

### R0005 — justify-unsafe

Every `unsafe` block must be immediately preceded by a `// safety: ...`
line-comment explaining the invariant being upheld. `unsafe fn` declarations
similarly require a `// safety:` doc on the item.

Rationale: every `unsafe` block must explain the invariant being upheld.

```rust
// rejected
let s = unsafe { std::str::from_utf8_unchecked(bytes) };

// accepted
// safety: bytes came from a UTF-8 validated source above.
let s = unsafe { std::str::from_utf8_unchecked(bytes) };
```

No escape hatch. The comment is the lint's whole point.

### R0006 — justify-allow

Every `#[allow(...)]` attribute must be accompanied by a `// reason: ...`
comment on the line above (or immediately after the attribute on the same
line).

Rationale: every `#[allow]` must explain why the rule is being suppressed.

```rust
// rejected
#[allow(dead_code)]
fn future_use() {}

// accepted
// reason: scaffolding for the upcoming retry path; will be wired in PR #214.
#[allow(dead_code)]
fn future_use() {}
```

No escape hatch.

### R0007 — no-impl-trait-return

_Phase 2 unimplemented._ `impl Trait` in return position is rejected unless the
function uses a named type alias.

Planned rationale: anonymous return types kill local reasoning; name the type
with an alias.

```rust
// will be rejected
fn iter() -> impl Iterator<Item = u32> { ... }

// will be accepted
type Counts = std::vec::IntoIter<u32>;
fn iter() -> Counts { ... }
```

Argument-position `impl Trait` remains allowed. The lint targets only the
return-type slot.

### R0008 — no-user-macros

User-defined `macro_rules!` and proc-macro invocations are banned unless the
file opts in with `#[strict::macros_ok]`.

Rationale: macros expand non-locally; agents misuse them frequently.

Always allowed regardless of opt-in: `vec!`, `format!`, `println!`, `eprintln!`,
`write!`, `writeln!`, `assert!`, `assert_eq!`, `assert_ne!`, `debug_assert!`,
`debug_assert_eq!`, `debug_assert_ne!`, `dbg!`, and `derive` macros from the
standard library.

```rust
// rejected
macro_rules! shout { ($s:expr) => { format!("{}!", $s) }; }

// accepted
#[strict::macros_ok]
macro_rules! shout { ($s:expr) => { format!("{}!", $s) }; }
```

Escape hatch: file-level `#![strict::macros_ok]` to allow user macros across an
entire module.

## Syntax extensions

The three extensions below are recognised by the lowering passes in
`crates/rustricted-lower`. They are pure source-to-source rewrites with no
runtime cost.

### Named arguments

_Phase 3 partially scaffolded; rewrite pass is a token-level pass-through
until then._

#### Definition

In a `#![strict]` crate, the parameter names on every `fn` declaration are part
of its public signature. Renaming a parameter is therefore a breaking change
for callers in other `#![strict]` crates the same way renaming the function
itself would be.

```rust
fn split_at(self, at: usize) -> (&str, &str) { ... }
fn write_text(path: &Path, contents: &str) -> io::Result<()> { ... }
```

#### Call site

```
CallExpr ::= Path '(' (NamedArg | Expr) (',' (NamedArg | Expr))* ')'
NamedArg ::= Ident ':' Expr
```

A call may freely permute named arguments; the lowering pass reorders them
back into declaration order based on the per-crate `CalleeRegistry`. Mixing
positional and named arguments in the same call is not allowed.

```rust
// equivalent after lowering
write_text(path: &p, contents: "hi")
write_text(contents: "hi", path: &p)
write_text(&p, "hi")          // positional fallback; see below
```

#### Mandatory threshold

In a `#![strict]` crate, any call with arity > 1 to a function whose declaration
is visible in the same crate _must_ use named arguments. Arity-1 calls and
calls to functions in unannotated upstream crates are exempt.

#### Cross-crate positional fallback

Calls into crates that do not opt into `#![strict]` accept positional arguments
unconditionally. This is the interop escape hatch — most of the ecosystem ships
unannotated signatures, and you cannot retroactively force names onto them.
`rustricted-std` ships a handful of named-arg-friendly wrappers for the worst
offenders (see [Standard library shims](#standard-library-shims)).

#### Lowering

The lowering pass (`rustricted_lower::named_args`) walks the token stream,
builds a `CalleeRegistry` from local `fn` declarations, and rewrites every
named call to positional based on declared order. The rewritten Rust contains
no trace of the name annotations.

### Pipe operator `|>`

_Phase 2; the lowering hook exists as a pass-through in `pipe.rs`._

#### Grammar

```
PipeExpr ::= Expr '|>' Path '(' ArgList ')'
ArgList  ::= (Expr | NamedArg) (',' (Expr | NamedArg))*
```

#### Precedence

`|>` binds **lower than method call** (`.`), field access, and indexing, and
**higher than assignment**. It is **left-associative**. Concretely, with `??`
denoting the `?` postfix operator:

```
a.b.c() |> f(x) |> g(y) ?? = h
// parses as
((((a.b.c()) |> f(x)) |> g(y)) ?? ) = h
```

Method chaining stays `.method()`. The pipe operator is for free functions and
associated paths; if you have a receiver and a method, use `.`.

#### Rewrite

```
e |> f(a1, a2)        →  f(e, a1, a2)
e |> path::to::f(a)   →  path::to::f(e, a)
e |> f(name: a)       →  f(e, name: a)   // named args resolved in a later pass
```

If `f` resolves to a method (intrinsic or trait), the pipe lowers to the method
form `e.f(a1, a2)`. The receiver is always inserted as the first positional
argument.

The leading `e` is the longest preceding contiguous expression — parenthesised,
bracketed, and braced groups count as atomic units. Statement boundaries
(`{`, `}`, `;`, `,`, start-of-group) terminate the receiver.

### `effect` keyword

_Phase 4; parser, inference, and check are stubs in `rustricted-effects`._

#### Signature grammar

```
FnSig ::= 'fn' Ident '(' Params ')' RetType? EffectClause? WhereClause? Block
EffectClause ::= 'effect' Effect ('+' Effect)*
Effect ::= Ident
```

The `effect` clause sits after the return type and before the where-clause (or
the block, when no where-clause is present).

```rust
fn read_config(path: &Path) -> Result<Config> effect io { ... }
fn save(state: &State) effect io + mut { ... }
fn worker() -> () effect io + async + panic { ... }
```

#### Built-in effects

Defined in `BUILTIN_EFFECTS`:

| Effect   | Meaning                                                     |
| -------- | ----------------------------------------------------------- |
| `io`     | Reads or writes the filesystem, network, env, or clock.     |
| `mut`    | Mutates state observable outside the function (statics, interior mutability). |
| `async`  | Awaits, spawns tasks, or otherwise touches the async runtime. |
| `panic`  | May panic on inputs the caller can plausibly provide.       |
| `unsafe` | Contains an `unsafe` block or calls an `unsafe fn`.         |

Crates may introduce custom effects by listing them in
`rustricted-std/effects.toml` (planned) or by declaring them in-crate
(syntax TBD).

#### Inference rule

For each function `f`, let `Declared(f)` be the effect set on its signature
(empty if absent) and `Inferred(f) = ⋃ Declared(callee)` over every call site
inside `f`. The check is:

```
Inferred(f) ⊆ Declared(f)
```

If `f` calls `g` and `g` declares `effect io`, then `f` must declare at least
`io`. A function that calls only pure functions can declare no effects (the
empty set is the default).

#### Erasure

Lowering strips every `effect` clause from the output. Effects are a
compile-time check with no runtime cost — the lowered Rust is identical to the
same code without the clause.

#### Cross-crate assumption

When `f` calls into an unannotated upstream crate, the callee's effect set is
assumed to be the most-permissive built-in set: `io + mut + panic`. Annotate
upstream signatures via `rustricted-std/effects.toml` to tighten this on a
per-function basis.

## Standard library shims

`rustricted-std` ships named-arg-friendly wrappers over the slice of `std` that
the lints and named-arg checker hit most often. Live at
`crates/rustricted-std/src/lib.rs`.

| Shim                                | Wraps                  |
| ----------------------------------- | ---------------------- |
| `rustricted_std::fs::read_to_string(path: &Path)` | `std::fs::read_to_string` |
| `rustricted_std::fs::write_text(path: &Path, contents: &str)` | `std::fs::write` |
| `rustricted_std::time::duration(secs: u64, nanos: u32)` | `Duration::new` |

The wrappers exist primarily so the lowering pass has something to rewrite
against during development. Phase 6 expands the coverage; for now, calling
bare `std::fs::write(path, contents)` is permitted but uses positional.

## Diagnostic format

Every diagnostic is rendered through `ariadne` with the rule code in the
banner, the source span underlined, a `why:` note explaining the rule's
purpose, and a `help:` line carrying a literal replacement when one is
available. The shape:

```
error[R0001]: `.unwrap()` is banned outside #[cfg(test)]
   ╭─[src/main.rs:12:35]
   │
12 │     let raw = std::fs::read_to_string(path).unwrap();
   │                                             ──┬───
   │                                               ╰── `.unwrap()` is banned outside #[cfg(test)]
   │
   │ note: why: panics on None/Err are silent control flow; agents reach for `.unwrap()` reflexively
   │ help: replace with `?` and return `Result`, or `.expect("…")` with a real message
───╯
```

The `Diagnostic` struct that produces this is in
`crates/rustricted-diag/src/lib.rs`. The renderer is `rustricted_diag::render`.

## Tooling

### `rustricted` CLI

```
rustricted build <input.rs> [--out <path>] [--edition <2021|2024>] [--no-lint]
rustricted check <input.rs>
rustricted lower <input.rs>
```

- `build`: lower, lint, write the lowered source to a tempfile, shell out to
  `rustc` to produce a binary at `--out` (default: input with extension
  stripped).
- `check`: lower and lint without invoking `rustc`. Exits non-zero if any
  diagnostic has error severity.
- `lower`: print the lowered Rust to stdout. Lints are skipped (useful for
  debugging the lowering passes).

### `cargo rustricted`

`cargo-rustricted` is a thin subcommand wrapper. When `cargo rustricted <args>`
is invoked, cargo prepends the literal `rustricted` to argv; the wrapper strips
it and execs `rustricted` with the remainder. The binary must be on `PATH`.

### `rustricted-lsp`

_Phase 5 — stubbed._ Currently the binary prints a placeholder message. The
planned server is `tower-lsp`-based; it will run the lowering and lint passes
on save, surface diagnostics, and quick-fix the highest-frequency violations
(positional → named, `.unwrap()` → `?`, glob → enumerated import).
