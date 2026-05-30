# Trust Language Reference

Status: Phase 0 driver shipped. Lints, syntax extensions, and effect tracking
are stubbed in-tree and land in subsequent phases. Items marked _unimplemented_
or _Phase N_ are specified here but not yet enforced by the toolchain.

## Overview

Trust is a strict dialect of Rust aimed at LLM-authored code. It bans a
short list of patterns that agents misuse on first contact and adds three
genuine grammar extensions: mandatory named arguments past arity 1, a pipe
operator `|>`, and an `effect` keyword for tracking side effects in function
signatures.

Trust is not a fork of `rustc` and not a new language family. The
toolchain is a frontend that lowers Trust source to plain Rust source and
hands the result to `rustc` via `cargo`. The pipeline is:

```
source → token stream → lowering passes → syn::File → prettyplease → rustc
```

Lowering is token-level (`proc_macro2::TokenStream`) until the extensions have
been desugared, at which point `syn` parses the result and `prettyplease` emits
the final Rust. The driver lives in `crates/trust`; the orchestration is
`trust_lower::lower`.

## Activation

Trust is opt-in per file. Two activation forms are accepted; pick the
one that matches your build setup.

### Single-file mode: `#![strict]`

For files run through `trust check` directly (no cargo build of the
file is required), an inner attribute at the crate root activates strict
mode:

```rust
#![strict]

fn main() { /* ... */ }
```

This is the form used by `examples/01-lints/*.rs` and the eval tasks. Stock
`rustc` rejects `#![strict]` because it is not a registered attribute —
which is fine for single-file inputs the Trust toolchain handles
end-to-end, but unsuitable for files that need to compile under
`cargo build`.

### Cargo mode: `trust_attrs::strict!{}` (lints only by default)

For files that participate in a `cargo build` (e.g. crates written in
Trust), use the marker macro from the `trust-attrs` crate
instead:

```rust
trust_attrs::strict! {}

fn main() { /* ... */ }
```

Add `trust-attrs = "0.1"` to `[dependencies]`. The macro expands to
nothing for `rustc`, so cargo builds are unaffected; the Trust
toolchain detects the invocation and activates the **lints** that work
at the AST level (R0001 unwrap, R0003 as-cast, R0004 glob, R0007
impl-trait, R0010 todo, R0011 panic, R0012 bool-param, R0014 bare-index,
R0005/R0006 justify-{unsafe,allow}, R0008 user-macros).

**Caveat — syntax extensions need the wrapper.** The marker alone does
not enable the syntax extensions (named arguments, pipe, `effect`). Those
are token-level rewrites that must run *before* rustc sees the file, and
`cargo build` invokes rustc directly. To make cargo crates accept the
extensions, set `RUSTC_WRAPPER` to the `trust-rustc` binary
(`crates/trust-rustc/`):

```sh
cargo build -p trust-rustc -p trust-rustdoc
RUSTC_WRAPPER=$(realpath target/debug/trust-rustc) \
RUSTDOC=$(realpath target/debug/trust-rustdoc) \
  cargo build
```

The wrapper detects strict-marked input files, runs the lowering pass,
substitutes the lowered source into the rustc invocation, and exec's the
real rustc with `--remap-path-prefix` set so diagnostics still point at
the original source. See `examples/cargo-strict-fixture/` for an
end-to-end demo (the file uses `make_point(x: 1, y: 2, z: 3)` named-arg
syntax which stock rustc rejects).

**Why both `RUSTC_WRAPPER` and `RUSTDOC`?** `rustdoc` does NOT honour
`RUSTC_WRAPPER` — it invokes rustc directly when compiling each doc-test
snippet. So any doc-test that uses Trust syntax (named-args, pipe)
would fail with a plain rustc parse error during `cargo test --doc`. The
sibling `trust-rustdoc` shim wraps rustdoc the same way: it lowers
the source file before rustdoc extracts doc-tests, and also rewrites the
code inside `///` / `//!` fenced code blocks so the extracted snippets
parse as plain Rust. Set it via the `RUSTDOC` env var (cargo replaces
the rustdoc binary outright on stable; `RUSTDOC_WRAPPER` works on
newer-cargo wrapper-style invocations and is also supported by the
shim). See `examples/cargo-strict-fixture-multimod/src/geom.rs` for a
doc-test that uses named-arg syntax — `cargo test --doc` fails without
the shim and passes with it.

**Current wrapper limitation.** Only the input `.rs` file passed to rustc
is lowered. Child modules referenced by `mod foo;` are read by rustc from
the original on-disk paths and are NOT lowered. A multi-file strict crate
must either keep all extension syntax in the crate root, or pre-lower its
sources manually. Generalising to a recursive walk of the crate's module
tree is a Phase 1 item.

### Detection rules

Both `trust_lower::detect_strict_mode` (token-level, runs before
parsing) and `trust_lints::detect_strict` (AST-level, runs after
lowering) accept either form. The lints crate returns an empty report
when neither is present; the lowering passes run unconditionally because
they are pure rewrites, but in a file without an activation marker they
have nothing to rewrite — pipe and `effect` are syntax errors in vanilla
Rust, and positional calls remain positional.

In practice: files without either activation marker round-trip through the
driver unchanged.

## Lints

Lint codes are stable. Each lint is emitted as a `Diagnostic` carrying the rule
code, a primary message, a `why:` note, and a `help:` suggestion. See the
[Diagnostic format](#diagnostic-format) section for the rendered shape.

The table below is auto-generated from `crates/trust-lints/src/rules.rs`
by `cargo xtask gen-docs`. Do not edit by hand — modify the `Rule` enum and
regenerate.

<!-- BEGIN auto-generated: lints-table -->

| Code  | Name                  | Phase | Severity |
| ----- | --------------------- | ----- | -------- |
| R0001 | no-unwrap             | 1     | error    |
| R0002 | empty-expect          | 1     | error    |
| R0003 | no-as-cast            | 1     | error    |
| R0004 | no-glob-import        | 1     | error    |
| R0005 | justify-unsafe        | 1     | error    |
| R0006 | justify-allow         | 1     | error    |
| R0007 | no-impl-trait-return  | 1     | error    |
| R0008 | no-user-macros        | 1     | error    |
| R0010 | no-todo-macro         | 1     | error    |
| R0011 | no-panic              | 1     | error    |
| R0012 | no-bool-param         | 1     | error    |
| R0014 | no-bare-index         | 1     | error    |
| R0015 | allow-missing-reason  | 1     | error    |
| R0016 | allow-unknown-code    | 1     | error    |
| R0042 | no-positional-args    | 1     | error    |

<!-- END auto-generated: lints-table -->

Rule metadata lives in `crates/trust-lints/src/rules.rs`. R0007
(`no-impl-trait-return`) is reserved but not yet implemented in the runner;
the catalogue entry exists so the rule code is stable when implementation
lands.

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

No general escape hatch. Prelude re-exports inside `trust-std` count as
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

### Per-callsite escape hatch: `#[allow(trust::Rxxxx, reason = "…")]`

Any item, statement, or expression can suppress one or more Trust
rules for its own scope with an inline `#[allow(trust::Rxxxx, reason
= "…")]` attribute (RT-46). The `reason = "…"` argument is **mandatory**
— a `#[allow(trust::R0014)]` without a reason emits R0015 and does
*not* suppress.

```rust
// arena indexing — the index is a Key, not a usize
#[allow(trust::R0014, reason = "Slab key, not usize")]
fn get(arena: &Arena, key: NodeKey) -> &Node { &arena[key] }

// per-statement suppression also works
fn f(v: &[u32], i: usize) -> u32 {
    #[allow(trust::R0014, reason = "bounds checked above")]
    let x = v[i];
    x
}

// multiple rules in one attribute
#[allow(trust::R0001, trust::R0014, reason = "test scaffold")]
fn scratch() { /* … */ }

// crate-level
#![allow(trust::R0014, reason = "this crate is all arena access")]
```

Two validation rules guard the mechanism:

- **R0015 allow-missing-reason** — `#[allow(trust::…)]` without a
  non-empty `reason = "…"` argument.
- **R0016 allow-unknown-code** — `#[allow(trust::R9999, …)]`
  referencing a rule code that isn't in the registry. The help text
  lists every valid code.

R0015 and R0016 themselves cannot be suppressed — that would let a
malformed allow silence its own validation diagnostic. Non-trust
allows (`#[allow(dead_code)]`, `#[allow(clippy::xxx)]`) are ignored by
this mechanism entirely; R0006 still governs them via its `// reason:`
comment requirement.

### R0007 — no-impl-trait-return

`impl Trait` in return position is rejected. Name the type with a `type`
alias (or use a concrete type / generic) and return that.

Rationale: anonymous return types kill local reasoning; name the type
with an alias so readers and tooling can see what's coming back.

```rust
// rejected
fn iter() -> impl Iterator<Item = u32> { [1u32].into_iter() }

// accepted
type Counts = std::array::IntoIter<u32, 1>;
fn iter() -> Counts { [1u32].into_iter() }
```

Argument-position `impl Trait` remains allowed. The lint targets only the
return-type slot, on free `fn`s, inherent / trait impl methods, and trait
method declarations.

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

### R0010 — no-todo-macro

`todo!()` and `unimplemented!()` are rejected outside `#[cfg(test)]` /
`#[test]` scopes.

Rationale: both macros ship as runtime panics. Strict mode treats them as
"you forgot to finish this" and forces either an implementation or a typed
`Err` return before the code can ship.

```rust
// rejected
fn compute_total(x: u32, y: u32) -> u32 { todo!() }

// accepted (finished)
fn compute_total(x: u32, y: u32) -> u32 { x + y }

// accepted (fenced for tests)
#[cfg(test)]
mod tests {
    fn skip_for_now() -> u32 { todo!() }
}
```

No escape hatch beyond `#[cfg(test)]`.

### R0011 — no-panic

`panic!()` is rejected outside `#[cfg(test)]` / `#[test]` scopes.

Rationale: explicit panics drop typed error information on the floor. Return
an `Err` and let the caller decide whether to abort.

```rust
// rejected
fn divide(a: i32, b: i32) -> i32 {
    if b == 0 { panic!("division by zero"); }
    a / b
}

// accepted
fn divide(a: i32, b: i32) -> Result<i32, &'static str> {
    if b == 0 { return Err("division by zero"); }
    Ok(a / b)
}
```

No escape hatch beyond `#[cfg(test)]`.

### R0012 — no-bool-param

Visible (`pub`, `pub(crate)`, `pub(super)`, `pub(in path)`) functions and
trait methods cannot take parameters of type `bool`. Private `fn` and
methods inside `#[cfg(test)]` scopes are exempt.

Rationale: raw `bool` parameters are positional footguns even with named
args — `spawn(detached: true)` does not say what `true` means. Use a
named enum so the call site reads as `Detached::Yes`.

```rust
// rejected
pub fn spawn(detached: bool, inherit_env: bool) { ... }

// accepted
pub enum Detached { Yes, No }
pub enum InheritEnv { Yes, No }
pub fn spawn(detached: Detached, inherit_env: InheritEnv) { ... }
```

Escape hatch: keep the function private, or wrap behind a `#[cfg(test)]`
helper. There is no `#[allow]` exemption — the dialect is opinionated
about boolean API surface.

### R0014 — no-bare-index

`expr[idx]` indexing is rejected when `idx` syntactically looks like a
`usize` position. Literal indices (`v[0]`, `arr[7]`) and range slices
(`v[..n]`, `v[a..b]`) are exempt because they typically encode
intentional access to a known position or window.

Rationale: `v[i]` panics on out-of-bounds; `.get(i)` returns `Option<&T>`
and forces the call site to handle the missing case. Const indices are
mostly used for tuple-like array access where bounds are known
statically; non-const integer indices are where the bugs live.

**Heuristic (RT-43):** the lint has no type information, so it fires
only when the index *looks* `usize`-typed:

- bare identifiers commonly used as numeric indices: `i`, `j`, `k`,
  `n`, `idx`, `index`
- identifiers ending in `_idx`, `_index`, `_i` (e.g. `child_idx`)
- arithmetic on `.len()`: `xs[xs.len() - 1]`

Anything else — `arena[key]`, `map[&node_key]`, `slab[entity_id]` — is
treated as key-style indexing into a `Slab`/`IndexMap`-shaped type and
is *not* flagged. Users who want the lint to fire on a key-style
callsite anyway can re-introduce it; users who want to silence a true
positive can use the per-callsite escape hatch.

```rust
// rejected (looks usize)
fn first_or_zero(v: &[u32], i: usize) -> u32 { v[i] }
fn last(v: &[u32]) -> u32 { v[v.len() - 1] }

// accepted (literal index)
fn first(v: &[u32]) -> u32 { v[0] }

// accepted (key-shaped, not flagged by heuristic)
fn get(arena: &Slab<Node>, node_key: NodeKey) -> &Node { &arena[node_key] }
```

Escape hatch: use `.get(i)` and handle the `Option`, move the call
under `#[cfg(test)]`, or attach
`#[allow(trust::R0014, reason = "…")]` to the enclosing item or
statement (see the per-callsite escape hatch section above).

### R0042 — no-positional-args

The dialect's main bug-prevention lint. Calls to locally-defined functions
with arity > 1 must use named arguments at the call site.

Rationale: positional argument ordering is the largest LLM-authored bug
class in Rust; named arguments eliminate it.

```rust
fn area(width: u32, height: u32) -> u32 { width * height }

// rejected
let a = area(1920, 1080);

// accepted
let a = area(width: 1920, height: 1080);
```

Emission lives in `trust-lower::named_args` rather than the lints
crate, because the check must run before names are stripped from call
sites during lowering. The catalogue entry stays in `Rule` so the code
is stable.

Scope:
- Fires when the callee is in the per-file callee registry (i.e. a `fn`
  defined in the same file) AND the call has more than one argument AND
  not all arguments are named.
- Silent for calls of arity 0 or 1, fully-named calls, and calls to
  unregistered callees (cross-crate, method calls on external types).
  Cross-crate enforcement requires `trust-std`-style annotated
  signatures; until then the cross-crate slot is the dialect's largest
  coverage gap.

Escape hatch: `#[allow(no_positional_args)] // reason: ...`.

## Non-strict diagnostics

Codes outside the `R00xx` strict-mode range are emitted by lowering and
analysis passes rather than the lint runner. They fire regardless of
`#![strict]` when their pass produces an error.

The table below is auto-generated from the `Rule` enum in
`trust-lower` by `cargo xtask gen-docs`.
Add new codes by extending the enum and regenerating.

<!-- BEGIN auto-generated: lowering-diagnostics-table -->

| Code  | Pass                | Crate                  | Message shape                                       |
| ----- | ------------------- | ---------------------- | --------------------------------------------------- |
| R2001 | pipe lowering       | `trust-lower`     | pipe `|>` requires a path-call on the right         |
| R3001 | named-args lowering | `trust-lower`     | `{fn}` has no parameter named `{arg}`               |

<!-- END auto-generated: lowering-diagnostics-table -->

The numeric prefix is a soft grouping (`R2xxx` for Phase 2 / pipe,
`R3xxx` for Phase 3 / named args, `R4xxx` for Phase 4 / effects) and is
documentation, not enforced by code. New codes in these ranges should
follow the convention.

## Syntax extensions

The three extensions below are recognised by the lowering passes in
`crates/trust-lower`. They are pure source-to-source rewrites with no
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

Calls into a crate Trust has no signature index for accept positional
arguments unconditionally. This is the interop escape hatch — most of the
ecosystem ships unannotated signatures, and you cannot retroactively force
names onto them. `trust-std` ships a bundled index for the worst offenders
(see [Standard library shims](#standard-library-shims)).

#### Cross-crate signature indices (RT-66)

To enforce named arguments on calls into a *specific* dependency, extract its
public-fn signature index and make it visible to the build:

```sh
trust index <dep-src-dir> -o <dep>.txt        # extract; works on any crate
TRUST_SIGNATURE_PATH=<dep>.txt cargo build     # (with the trust-rustc wrapper)
```

`trust index` walks a crate's `src/` (or a single `.rs` file, or stdin) and
emits one `name:p1,p2` line per public `fn`, in the same manifest format as
`trust-std/std-signatures.txt`. The `trust-rustc` / `trust-rustdoc` wrappers and
the `trust` CLI read the `TRUST_SIGNATURE_PATH` environment variable — a
platform-separated list of manifest files and/or directories of `*.txt`
manifests — and seed the [`CalleeRegistry`](#named-arguments) from them. A name
that resolves to conflicting parameter lists across the loaded indices (or
between an index and the crate being built) is dropped, degrading to the
positional fallback rather than a wrong reorder. What is not yet automatic:
discovering those indices from cargo's dependency graph without naming them
explicitly. See [`examples/cross-crate-index`](../examples/cross-crate-index/).

#### Lowering

The lowering pass (`trust_lower::named_args`) walks the token stream,
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

## Standard library shims

`trust-std` ships named-arg-friendly wrappers over the slice of `std` that
the lints and named-arg checker hit most often. Live at
`crates/trust-std/src/lib.rs`.

| Shim                                | Wraps                  |
| ----------------------------------- | ---------------------- |
| `trust_std::fs::read_to_string(path: &Path)` | `std::fs::read_to_string` |
| `trust_std::fs::write_text(path: &Path, contents: &str)` | `std::fs::write` |
| `trust_std::time::duration(secs: u64, nanos: u32)` | `Duration::new` |

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
`crates/trust-diag/src/lib.rs`. The renderer is `trust_diag::render`.

### Machine-readable output (RT-70)

`trust check --format json` emits the same diagnostics as a stable JSON
document for agent consumers (`trust_diag::to_json`). Each entry carries the
rule code, severity, message, the byte span **and** 1-based line/column, the
`why` rationale, the prose `help`, and — when the toolchain can produce one — a
structured `fix`:

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
      "fix": {"span": {"start": 50, "end": 62, …},
              "replacement": "(width: ..., height: ...)",
              "applicability": "hasPlaceholders"}
    }
  ]
}
```

A `fix` is a span + exact `replacement` + an `applicability` confidence —
`automatic` (semantics-preserving, apply unattended), `maybeIncorrect` (review
first; may depend on unseen context), or `hasPlaceholders` (`...` markers must
be filled before it compiles). An agent loop applies `automatic` fixes
directly and treats the rest as guided suggestions. `why`, `help`, and `fix`
are `null` when absent; the document is emitted on stdout and the process still
exits non-zero when any diagnostic is an error.

## Tooling

### `trust` CLI

```
trust build <input.rs> [--out <path>] [--edition <2021|2024>] [--no-lint]
trust check <input.rs> [--format <human|json>]
trust lower <input.rs>
trust index <src-dir|input.rs> [--out <path>]
```

- `build`: lower, lint, write the lowered source to a tempfile, shell out to
  `rustc` to produce a binary at `--out` (default: input with extension
  stripped).
- `check`: lower and lint without invoking `rustc`. Exits non-zero if any
  diagnostic has error severity. `--format json` emits machine-readable
  diagnostics with structured fixes (RT-70; see
  [Machine-readable output](#machine-readable-output-rt-70)).
- `lower`: print the lowered Rust to stdout. Lints are skipped (useful for
  debugging the lowering passes).
- `index`: extract a crate's public-fn signature index to a `name:p1,p2`
  manifest (RT-66), for a dependent build to enforce named args against via
  `TRUST_SIGNATURE_PATH`. Writes to `--out` or stdout. See
  [Cross-crate signature indices](#cross-crate-signature-indices-rt-66).

`build`, `check`, and `lower` honour `TRUST_SIGNATURE_PATH` — they seed the
callee registry from the named dependency manifests before lowering, so
cross-crate calls get the same R0042 / named-arg treatment as in-crate ones.

### `cargo trust`

`cargo-trust` is a thin subcommand wrapper. When `cargo trust <args>`
is invoked, cargo prepends the literal `trust` to argv; the wrapper strips
it and execs `trust` with the remainder. The binary must be on `PATH`.

### `trust-lsp`

_Phase 5 — stubbed._ Currently the binary prints a placeholder message. The
planned server is `tower-lsp`-based; it will run the lowering and lint passes
on save, surface diagnostics, and quick-fix the highest-frequency violations
(positional → named, `.unwrap()` → `?`, glob → enumerated import).
