# Rationale

Why each rule and extension exists, with the bug class it targets, the shape
of the rule, the tradeoffs, and the escape hatch. This document is for the
reader who would otherwise dismiss Rustricted as a kabuki suit of clippy
opinions. Where a rule costs something, this document says so.

The audience is an experienced Rust programmer who has not been convinced yet.

## Lints

### R0001 — no-unwrap

#### Bug class

An LLM, asked to "load a config file", writes:

```rust
fn load_config(path: &Path) -> Config {
    let raw = std::fs::read_to_string(path).unwrap();
    toml::from_str(&raw).unwrap()
}
```

The function then ships. Six weeks later a customer reports a `panicked at
src/config.rs:7:35` in production from a config file with one stray tab. The
agent picked `.unwrap()` because every Rust tutorial on the web uses it as the
ergonomic shortcut around `Result`, and the agent's training data does not
distinguish "shortcut for the REPL" from "shortcut for `main.rs`".

#### Why this shape

The rule could be looser ("ban `.unwrap()` only on `Result`") or stricter
("ban `.unwrap()` everywhere, including tests"). The middle is right because:

- The test exemption matches what humans actually want. Test failures should
  be loud and lazy. Forcing `?` plumbing through every test setup is the
  pedant's loss.
- Banning on both `Result` and `Option` is the right call even though `Option`
  unwrap is sometimes "safe" by surrounding logic. The agent that wrote
  `.unwrap()` on an `Option` did not _prove_ it was safe; it _hoped_. The
  remediation (`?` for `Option<T>` returning a `Result`, `unwrap_or_else`,
  `ok_or`) is well within an agent's reach and forces the question.

#### Tradeoffs

- More keystrokes for prototypes. `let port: u16 = "8080".parse()?` is fine in
  a function returning `Result`, but inside `fn main() -> ()` you must change
  the signature to `Result<()>` or use `expect("…")`. This is a real cost
  during quick scripting.
- Library authors sometimes want `.unwrap()` on an `Infallible` conversion.
  The escape is `#[allow(no_unwrap)] // reason: From<Self> is Infallible`.

#### Escape hatch

- Tests: any function under `#[cfg(test)]` or marked `#[test]` is exempt.
- Item-level: `#[allow(no_unwrap)] // reason: ...` on the enclosing fn or
  module.

### R0002 — empty-expect

#### Bug class

```rust
let port = env::var("PORT").expect("");
```

An agent learns that "`.expect()` is the polite version of `.unwrap()`" and
then dispatches the polite version with an empty string. The result is
strictly worse than `.unwrap()`: the panic message says `panicked at … with
message: ""`, which is now untraceable to the call site without a debugger.

#### Why this shape

A trivial rule that pays for itself the first time someone reads a panic
message. The whole value of `.expect()` is the message. The rule is, strictly,
"if you're going to take the syntactic cost of writing `.expect("…")` over
`.unwrap()`, write something."

#### Tradeoffs

None. There is no reason to write `.expect("")`.

#### Escape hatch

None.

### R0003 — no-as-cast

#### Bug class

```rust
fn pack(value: u64) -> u8 {
    value as u8
}
```

The agent wanted to "convert to byte" and got a silent truncation. `as` casts
do not warn, do not error, do not check, and round-trip through three signed
representations on the way down. The same agent writing `value.try_into()`
would have had a `Result` to deal with.

#### Why this shape

`as` is overloaded — pointer casts, numeric truncation, numeric widening,
ptr-to-int, int-to-ptr, fn-pointer casts. The rule bans all of them in
expression position because:

- Widening (`u8 as u32`) is `From::from`. Free.
- Narrowing (`u32 as u8`) is `try_into()`. Returns `Result`. Right.
- Pointer casts are rare enough to live behind `#[allow]` with an explanation
  of the invariant.
- `as` for `*const T -> *const U` should be a `.cast()` method call, which is
  not banned.

The rule is whole-keyword rather than per-conversion-kind because the LLM
failure mode is "reaches for `as` to make the type checker stop complaining",
and that reflex must be denied at the keyword level. Carving exceptions
re-opens the gate.

#### Tradeoffs

- Performance-sensitive code that knows the value fits. `value as u8` is one
  instruction; `value.try_into().unwrap_or(0)` is several plus a branch on
  cold paths. Use `#[allow]` with a `// reason:` that names the bound.
- Pointer arithmetic gets clumsier. The `.cast()` method exists, but some
  ergonomic patterns require `as`. Same escape.

#### Escape hatch

`#[allow(no_as_cast)] // reason: <the invariant>` on the enclosing item. The
comment should name _why_ the conversion is total or _why_ truncation is
desired.

### R0004 — no-glob-import

#### Bug class

```rust
use std::collections::*;
use crate::types::*;
use serde::*;
```

The agent saw a missing-symbol error and resolved it by importing everything
in the namespace. Three things break:

1. Adding a symbol upstream can silently shadow a local symbol.
2. Reading the file no longer tells you where any given identifier came from
   — you have to grep the import graph.
3. Glob imports inside a `prelude` module compose: `use foo::prelude::*;`
   inherits whatever `foo` chose to put there. That choice changes between
   versions.

#### Why this shape

Strict and total. Glob imports are convenient at exactly one moment (writing
the import) and inconvenient forever after (reading anything else). The
ergonomic loss is real and small; the readability gain is real and large. The
specific cases where globs feel justified — preludes, derive macros' generated
code, `use super::*;` in test modules — are either things the linter doesn't
see (generated code) or things you can spell out (the test prelude lists what
it actually re-exports).

#### Tradeoffs

- `use super::*;` in tests is a real cost. You will type `use super::{a, b,
  c};` more often than you would like.
- Preludes from third-party crates become annoying. `use diesel::prelude::*;`
  is the documented entry point for diesel. The escape is `#[allow]` on the
  `use` itself; we accept the friction in exchange for not paying the global
  cost.

#### Escape hatch

`#[allow(no_glob_import)] // reason: <crate>'s prelude is the documented
entry point` on the `use` item.

### R0005 — justify-unsafe

#### Bug class

```rust
let s = unsafe { std::str::from_utf8_unchecked(bytes) };
```

`bytes` came from somewhere — a file, a socket, a previous call. Is it
actually UTF-8? The agent wrote `unchecked` because it appeared in an example
and was faster. There is no record of what invariant was supposed to hold.
Three months later, someone refactors the call site and now `bytes` can come
from a base64 decode.

#### Why this shape

The rule is comment-based, not type-based, because the invariant is almost
always something the type system cannot express. The point isn't to prove
safety to the compiler — it's to force the author to write down the
assumption so the next reader can check it against current reality.

The required prefix is the literal token `// safety:`. The renderer can grep
for these to produce an audit report.

#### Tradeoffs

- It is a comment. Comments lie. Stale `// safety:` lines are worse than no
  comment because they actively mislead.
- In a tight inner loop, the author may write a generic comment and reuse it
  across blocks. This is on the author.

#### Escape hatch

None. The point of the rule is the comment; suppressing it defeats the rule.

### R0006 — justify-allow

#### Bug class

```rust
#[allow(dead_code)]
mod future;
```

The agent suppressed a lint because the lint complained. Six months later
the suppressed warning has accreted a second meaning ("this whole module is
actually dead and should be deleted") and nobody knows.

#### Why this shape

`#[allow]` is the universal escape from every other rule in this document.
If it has no friction, the whole document is performance art. The friction is
the smallest possible: write one sentence next to it.

`// reason:` is the literal prefix. Same as `// safety:` for `unsafe`. Same
grep-ability.

#### Tradeoffs

- Test suites with many `#[allow(unused)]` get noisier. Apply the attribute
  once at the module top with a single reason instead of per-fn.
- Generated code (proc-macros, build scripts) sometimes emits `#[allow]`. The
  rule does not fire on items that span exactly one token range that the lint
  identifies as macro-generated. _Detection of "macro-generated" is a known
  hole; see Phase 5 plans._

#### Escape hatch

None — the comment _is_ the hatch.

### R0007 — no-impl-trait-return

_Phase 2 unimplemented._

#### Bug class

```rust
fn iter_active(&self) -> impl Iterator<Item = &User> + '_ {
    self.users.iter().filter(|u| u.active)
}
```

The author later adds `.map(|u| &u.id)`. The return type's identity changes
silently. Callers who relied on `Item = &User` now break, but the breakage
surfaces at the call site, not at the change site.

A second failure mode, agent-specific: when asked to "extract a helper that
returns this iterator", the agent copies the `impl Iterator<…>` signature
into the helper. Now there are two anonymous types with related-but-different
lifetimes, and a small refactor that changes one of them produces an
inscrutable trait-bound error somewhere two functions away.

#### Why this shape

A type alias is one line and forces the author to name what they're
returning:

```rust
type ActiveUsers<'a> = std::iter::Filter<std::slice::Iter<'a, User>, fn(&&User) -> bool>;
```

Yes, that signature is hideous. That hideousness is the rule's value — once
the author has written it down, the choice to box the iterator
(`Box<dyn Iterator<Item = &User> + '_>`) or change the data structure becomes
the obvious alternative.

The rule is return-position only. `impl Trait` in argument position
(`fn f(it: impl Iterator<…>)`) is unobjectionable — it's just sugar for a
generic.

#### Tradeoffs

- Real cost in noise for trivial iterator helpers.
- Some types are genuinely unnameable (closures, `async fn` futures
  pre-`TAIT`). The escape there is `Box<dyn Trait>` plus an alias.

#### Escape hatch

`#[allow(no_impl_trait_return)] // reason: <unnameable closure type>` on the
fn.

### R0008 — no-user-macros

#### Bug class

```rust
macro_rules! ensure_positive {
    ($x:expr) => { if $x <= 0 { return Err(Error::NonPositive); } };
}

pub fn process(n: i64) -> Result<()> {
    ensure_positive!(n);
    Ok(())
}
```

The macro hides a `return`. The agent calling `process` cannot see, from the
call site, that `n <= 0` is a possible error. Worse: when the agent then asks
to refactor `process` to use `anyhow`, the macro keeps emitting `Error::NonPositive`
and the refactor silently breaks.

A second failure mode: the agent _writes_ macros. Asked to "generate getters
for these fields", an LLM will reach for `macro_rules!` rather than just
listing the getters. The macro it produces will work; the macro it produces
six months later, after the struct changes, will not.

#### Why this shape

The allowlist (`vec!`, `format!`, `println!`, `assert!`, `derive`) covers
~95% of legitimate macro use. The remaining 5% is either:

- _Real metaprogramming_, in which case the file owner accepts the cost and
  writes `#![strict::macros_ok]` at the top.
- _Boilerplate reduction_, in which case the right answer is usually a
  function, a generic, or a derive macro already in the allowlist.

The opt-in is file-level rather than block-level because macro hygiene is
file-level. If you're going to use macros, accept the cost across the whole
file.

#### Tradeoffs

- DSL-heavy code (Diesel, Yew, Leptos) becomes annoying. These crates rely
  on extensive proc-macros. The escape is to opt in per file: the modules
  that touch the DSL get `#![strict::macros_ok]`; the rest of the crate
  doesn't.
- Some idioms genuinely require a macro to be ergonomic (`tracing::info!`).
  Allowlisting them individually is feasible but punted; for now,
  `#![strict::macros_ok]` per file is the workaround.

#### Escape hatch

- Per file: `#![strict::macros_ok]` at the top.
- Per macro definition: `#[strict::macros_ok]` on the `macro_rules!` item.

## Syntax extensions

### Named arguments

#### Bug class

```rust
let parts = buf.split_at(5, true);
```

`split_at` does not have a `bool` parameter, but if it did — or if the agent
called `Vec::splice(0..3, more)` with the range and the iterator swapped — the
type system would happily accept the call. Positional argument errors are
the single largest class of LLM-authored Rust bugs we have observed. They are
not random typos; they are the predictable failure mode of a model that has
seen the signature once and is now reconstructing the call from semantic
intent.

The classic offender is `Duration::new(0, 60)` (zero seconds, sixty
nanoseconds) when the author meant "sixty seconds". The compiler sees no
problem.

#### Why this shape

Named arguments are mandatory past arity 1, not past arity 2 or arity 3. The
threshold is one because:

- Arity-2 calls are by far the most common arity, and they are the most
  bug-prone — there is exactly one swap, and it's easy to make.
- The "obvious from context" argument breaks down at arity 2 already
  (`split_at(buf, 5)` vs `split_at(5, buf)` — which is the receiver?).

Arity-1 calls are exempt because the cost of `f(name: x)` instead of `f(x)`
is high and the bug class doesn't exist (there's nothing to swap with).

#### Why not enforce arity > 0

A unary call where the argument's role is unclear (`User::new(s: "alice")`)
is occasionally a real bug. We accept the loss because the linter would have
to fire on `Some(x)`, `vec![x]`, `Box::new(x)`, and many others — and we'd
have to special-case all of them. The marginal value isn't worth the rule
complexity.

#### Tradeoffs

- _Real cost._ Every call past arity 1 takes more keystrokes and more
  vertical space. Code with many small helpers (parser combinators,
  builders) gets visibly longer.
- _Interop friction._ Calls into unannotated crates use positional. A file
  that mixes a `#![strict]` crate's named-arg style with `std` positional
  calls reads inconsistently. The shims in `rustricted-std` address the
  worst offenders.
- _Refactoring._ Renaming a parameter is now a breaking change. This is a
  feature (the name is part of the contract) but also a cost (you can't
  silently rename things).

#### Escape hatch

- Cross-crate: positional fallback is always available when the callee is
  not in a `#![strict]` crate.
- Within-crate: there is no escape hatch. If your local callee is in your
  own `#![strict]` crate, you must name the arguments. The fix is one line.

### Pipe operator `|>`

#### Bug class

```rust
let counts = sort(group_by(filter(parse(input), valid), key));
```

The agent has written a four-stage data pipeline. To read it you start in the
middle and unwind outward. To write it the agent had to think backward. The
risk isn't a compile-time bug; it's that the agent (and the human reviewer)
misreads the data flow and fails to notice that, say, `filter` and `sort`
should be swapped.

The conventional Rust fix is method chaining, but it requires every operation
to be a method on the receiver. For free functions and trait-method-not-yet
patterns, you either nest or introduce a long chain of `let` bindings:

```rust
let parsed = parse(input);
let filtered = filter(parsed, valid);
let grouped = group_by(filtered, key);
let counts = sort(grouped);
```

That's better — but it pollutes the namespace with four single-use bindings,
and the agent must invent four names. Naming is hard; agents do it badly.

#### Why this shape

The pipe operator desugars `e |> f(args)` to `f(e, args)`. It's a syntactic
convenience with no semantic content. The agent can write left-to-right in
data-flow order:

```rust
let counts = parse(input)
    |> filter(by: valid)
    |> group_by(by: key)
    |> sort();
```

This is more readable than nested calls, no worse than method chaining, and
crucially does not require the agent to invent intermediate names.

The choice of `|>` over `>>`, `.>`, `||>`, etc. is conservative — `|>` is
the OCaml/Elixir/F# convention, and the agent's training data already knows
the syntax. We did not invent a novel operator.

#### Precedence: lower than `.`, higher than `=`

This is the precedence used in Elm, F#, and Elixir. The alternative — pipe
binds tighter than `.` — produces surprising parses on `xs |> filter(p).first()`
(does `.first()` apply to `xs`, to `filter`'s result, or to the whole
expression?). The current choice makes the pipe a true statement-level
combinator: `xs.method() |> next_stage()` is unambiguous.

#### Tradeoffs

- It's a new operator. Rust's design philosophy is to avoid adding syntax.
  We accept the cost because the value of left-to-right pipelines for
  agent-authored data code is large.
- Misuse: `e |> e.method()` where the agent forgot the pipe receives the
  expression `e` is now valid syntax (the receiver is just `e`, and
  `e.method()` becomes the entire RHS — confusing). We accept this; the
  diagnostic is acceptable when it happens.

#### Escape hatch

None — `|>` is a pure addition. If you don't want it, don't write it.

### `effect` keyword

#### Bug class

```rust
pub fn render_template(name: &str, ctx: &Context) -> String {
    let path = format!("templates/{name}.html");
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    render(&raw, ctx)
}
```

The signature `fn render_template(&str, &Context) -> String` does not mention
the filesystem. The caller, looking at the signature, assumes a pure
computation. The agent calling this function from inside an async handler
will block the executor; the agent calling it inside a test will fail
unpredictably on a different machine.

This is the same class of bug `async`/`await` was invented to surface. Rust
already requires `async` and `unsafe` to appear in signatures because
those properties leak. Filesystem I/O leaks the same way. So does mutation
of global state. So does panicking.

#### Why this shape

The `effect` keyword generalises Rust's existing two-effect system (`async`
and `unsafe`) to a small open set. The default set is the five
common-denominator effects (`io`, `mut`, `async`, `panic`, `unsafe`); crates
can introduce custom effects if they want finer granularity (e.g., a web
framework distinguishing `db` from `http` from `fs`).

Inference is subset-checking, not equality-checking. A function declares
its effect set; the compiler verifies the declaration is at least as broad
as the union of its callees' effects. The author can over-declare (claim
`effect io + mut` when only `io` is reachable); the author cannot under-declare.

Erasure is total: the lowered Rust has no `effect` annotations. There is
zero runtime cost; the check is compile-time-only, like `unsafe`.

#### Granularity

Five built-in effects is too few for some users and too many for others.
The current design is "start coarse, refine later":

- `io` is one effect, not three (`fs`, `net`, `env`). Splitting later is a
  pure addition; merging later is a breaking change. Start merged.
- `panic` is one effect, not "panics on input X". Sub-effects (e.g.,
  `panic.bounds`) are an open design question, not part of Phase 4.
- User-defined effects are flat names. No subtyping, no inheritance.
  Hierarchies can be encoded as `+`-sums.

#### Cross-crate assumption

When `f` calls into an unannotated upstream crate, the callee is assumed to
have effect set `io + mut + panic`. This is the most permissive built-in
non-async, non-unsafe set; it is conservative and will produce false
positives (warnings on calls that are actually pure). The fix is to annotate
the upstream function in `rustricted-std/effects.toml`.

The alternative — assume the empty set — is wrong because it would silently
unsubscript I/O bugs. Better to over-declare and let the author tighten the
annotation, than to under-declare and miss the leak.

#### Tradeoffs

- _Cognitive cost._ Effects are a new column in every signature. Readers
  learning the codebase will scan past them at first; the value compounds
  only after the second or third refactor.
- _Annotation overhead._ Every public function in a strict crate grows a
  clause. Even pure helpers grow `effect` (empty, implicit) — or the
  author has to remember which functions are pure to omit the clause.
- _The std-effect file is a maintenance burden._ Every `std` function the
  agent calls needs an entry. Coverage gaps produce noisy warnings. The
  shims in `rustricted-std` are an attempt to localise this.
- _Custom effects fragment._ If every crate invents its own effect names,
  the system devolves into a verbose form of doc comments. The discipline
  is "keep the set small; pull common names up into `rustricted-std`."

#### Escape hatch

- _Bootstrap._ The check pass can be disabled crate-wide with
  `#![allow(rustricted::effect_check)]` (planned).
- _Per-call._ `#[allow(effect_check)] // reason: ...` on the call's
  enclosing fn.
- _Over-declare._ Declaring `effect io + mut + panic + async + unsafe`
  always type-checks. Over-declaration is honest but reduces the
  signature's value.
