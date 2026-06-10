# Why Trust

Audience: an engineer deciding whether to adopt Trust on an
agent-driven codebase, or a designer building something similar. If you
are looking to contribute to the toolchain itself, start with
[AGENTS.md](AGENTS.md) and [SPEC.md](SPEC.md) instead.

This document is the rationale, not the reference. It is short on
purpose.

## The problem

LLMs write Rust that compiles and looks plausible, then ships a small,
predictable set of bugs. The set is narrow enough to name:

- **Positional arguments past arity 2.** A model that knows
  `make_rect(width, height)` will, on a sibling call site three files
  later, write `make_rect(height, width)`. The compiler accepts it. The
  type checker accepts it. The reviewer skims it. The bug ships.
- **`.unwrap()` in production paths.** The reflex is overwhelming.
  Training data is saturated with example code that unwraps because the
  example is two lines long and the alternative would dilute the point.
  Models reproduce the reflex in 500-line files where the alternative is
  the point.
- **`as` casts that silently truncate.** `len as u32` on a `usize`
  that came from user input is a bug class, not an idiom. The model
  reaches for `as` because it is shorter than `.try_into()?`.
- **Glob imports.** `use crate::types::*;` in a test module hides what
  is in scope and lets unrelated refactors change which symbol resolves.
  Models add globs the moment a file has more than four imports.
- **Opaque macros.** A `macro_rules!` two files away expands into the
  call site and changes what the code means. The model that wrote the
  macro and the model that called it are not the same session.

These are not novel observations. They show up in every postmortem of
agent-authored Rust. What is novel is that the list is _short_ and
_stable_: the same handful of patterns dominate every corpus we have
looked at. See [`eval/runs/`](../eval/runs/) for the run logs and
[`case-studies/heck-strict.md`](../case-studies/heck-strict.md) and
[`case-studies/tre-strict.md`](../case-studies/tre-strict.md) for what
the lints catch on real third-party crates (a pure library and a small
CLI with real I/O respectively).

## The numbers

We ran five single-file tasks twice per model — once in plain Rust, once
with `#![strict]` — and measured the *ship rate*: the known-bad pattern
present in the output **and** not caught before it would reach a compile.
Lower is better.

| Model | Vendor | Vanilla shipped | Trust shipped |
|-------|--------|-----------------|---------------|
| Claude Haiku | Anthropic | 9/15 (60%) | **0/15 (0%)** |
| Claude Sonnet | Anthropic | 9/15 (60%) | **0/15 (0%)** |
| GPT-4o | OpenAI | 9/15 (60%) | **0/15 (0%)** |
| Gemini 2.5 Flash | Google | 9/15 (60%) | **0/14 (0%)** |

Four models, three vendors, the same result: about 60% of files shipped
one of the audited bugs in plain Rust, and none shipped under Trust. The
run logs are in [`eval/runs/`](../eval/runs/) — [002](../eval/runs/002/summary.md)
(Haiku), [004](../eval/runs/004/summary.md) (Sonnet),
[005](../eval/runs/005/summary.md) (GPT-4o),
[006](../eval/runs/006/summary.md) (Gemini).

Read the result narrowly, because that is how it is true. Of the five
tasks, three reliably elicited bugs — positional argument order (R0042)
and `as`-cast truncation (R0003) — and the dialect caught **every
instance**. The other two — `.unwrap()` reach (R0001) and glob imports
(R0004) — produced **zero** bugs from these models at this scale, so
their rules were not tested by this suite, not vindicated by it. The
stronger-model-ships-fewer-bugs effect we expected did **not** appear:
Haiku, Sonnet, and GPT-4o all bugged at the same 60% rate. The claim the
data licenses is precise: *on the bug classes that fire, agents ship them
~60% of the time and Trust catches 100%.* It is not "Trust makes agent
Rust bug-free," and the eval directory says so in more detail than this
paragraph does.

## The thesis

Rust is already excellent at the parts of language design that are hard
to get right. Exhaustive `match`. No null. Explicit mutation. Sum types
with payloads. A borrow checker that, whatever its costs, makes a large
class of bugs impossible to express. An agent writing Rust starts ahead.

The delta from "Rust" to "Rust an agent writes correctly on the first
try" is small. It is the list above. Trust is the toolchain that
closes that delta and nothing else. There is no new type system, no new
runtime, no replacement standard library. The output is plain Rust
source, handed to a stock `rustc`. If you stop using Trust
tomorrow, the lowered Rust your codebase produced still builds.

## What you write

### Named arguments

In a strict crate, calls with more than one argument must name them.
The lowering pass rewrites the call back to positional before `rustc`
sees it.

```rust
#![strict]

fn make_rect(width: u32, height: u32) -> u32 {
    width * height
}

fn main() {
    // rejected by R0042
    let a = make_rect(10, 5);

    // accepted; order is free
    let b = make_rect(width: 10, height: 5);
    let c = make_rect(height: 5, width: 10);

    println!("{b} {c}");
}
```

Arity-1 calls remain positional. Calls into upstream crates that have
not opted in remain positional. The rule fires where the bug class
actually lives: in-crate calls with multiple parameters whose order is
easy to swap. See [`examples/03-named-args/area.rs`](../examples/03-named-args/area.rs).

### Strict lints

`.unwrap()` outside `#[cfg(test)]` is a hard error. The replacement is
in the diagnostic.

```rust
#![strict]

use std::path::Path;

// rejected: R0001 no-unwrap
fn load(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

// accepted: propagate the error
fn load(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}
```

The full catalogue (R0001 through R0042) is in
[SPEC.md § Lints](SPEC.md#lints). Every diagnostic carries a `why:`
note explaining the rule and a `help:` line with a literal replacement;
that contract is documented in
[AGENTS.md § The teaching-error contract](AGENTS.md#the-teaching-error-contract).

## What it costs

Honestly:

- **Verbose call sites.** `make_rect(width: 10, height: 5)` is longer
  than `make_rect(10, 5)`. Most engineers feel this most on the first
  day. The agent does not feel it at all.
- **Activation per crate or per file.** Crates opt in once with
  `[package.metadata.trust] strict = true` in `Cargo.toml` and build with
  `cargo trust build`; single files (and file-by-file opt-ins) use
  `#![strict]` at the top. See
  [SPEC.md § Activation](SPEC.md#activation).
- **Cargo needs a wrapper for the syntax extensions.** The named-arg
  rewrite has to run before `rustc` sees the file, and `cargo build`
  invokes `rustc` directly. `cargo trust` sets the wrapper up for you;
  only bare `cargo build` needs manual `RUSTC_WRAPPER` wiring.
  The lints alone work without the wrapper; the syntax
  extensions do not.
- **Cross-crate enforcement needs a generated index.** In-crate callees
  are automatic. For calls into a dependency, run `trust index <dep-src>
  -o <file>` to extract that crate's public-fn signatures, then point the
  build at them with `TRUST_SIGNATURE_PATH` (RT-66). R0042 and named-arg
  reordering then apply across the boundary — against an index extracted
  from *any* crate, not just the bundled `trust-std` shim. What is still
  manual is discovery: you generate the indices and name them, rather than
  Trust reading them automatically from cargo's dependency graph. That
  last step is the remaining gap; the extraction and enforcement
  themselves are done. See
  [`examples/cross-crate-index`](../examples/cross-crate-index/).

If that is a blocker, Trust is not ready for you yet.

## Design priorities

Trust is agent-first and human-second. When a design choice trades
verbosity for explicitness, or rigidity for fewer authoring mistakes,
the agent-friendly side wins and the doc does not apologise for it.
Mandatory named arguments, exhaustive `// safety:` and `// reason:`
comments, no-glob imports, no-bool-param on public surface — every one
of those is more typing for the human and fewer wrong call sites for
the agent.

The phrase from the original design conversation was "this is for me,
not for you" — me being the agent, you being the human reader. The
language exists to reduce the systematic mistakes LLMs make. Where a
choice is hostile to both humans and agents (cryptic errors, surprising
semantics), it is rejected on its own merits; this is about resolving
genuine tradeoffs, not licensing bad design.

The full lint catalogue, rule-by-rule rationale, and grammar for the
two syntax extensions are in [SPEC.md](SPEC.md). The
phase-by-phase rationale for each individual rule is in
[RATIONALE.md](RATIONALE.md).

## Compared to the neighbors

Trust is not the first "stricter dialect of a language people already
use." The playbook has shipped at scale three times, and the comparison
sharpens what Trust is and is not.

**TypeScript over JavaScript** is the closest structural match: a strict
layer that compiles down to the host language, adoptable file-by-file,
with the promise that you can stop using it and keep the output.
TypeScript's bet was that *humans* writing JavaScript needed types to
scale teams. Trust's bet is that *agents* writing Rust need call-site
explicitness to ship correct code on the first try — the type system is
already there; what's missing is syntax that makes argument order
checkable. Same shape, different missing piece.

**Sorbet over Ruby** and **mypy over Python** retrofitted gradual typing
onto dynamic languages, and both paid a heavy tax: annotations live in a
parallel universe (sigs, stubs, `# type:` comments) that drifts from the
code it describes. Trust deliberately avoids the parallel-universe trap —
named arguments are *in* the call site, the lints read the real source,
and the lowered output is plain Rust with no annotations to drift. The
cost of that choice is a wrapper in the build; the benefit is that
nothing can go stale.

**Clippy** is the neighbor people ask about first, and the boundary is
precise: most strict lints have Clippy analogues, and if the lints were
the whole story Trust would not need to exist. The one thing Clippy
structurally cannot do is catch a same-typed positional-argument swap —
`make_rect(height, width)` against `make_rect(width: u32, height: u32)`
is type-correct, so no analysis of the *existing* syntax can flag it.
The fix requires syntax Rust doesn't have: names at the call site. That
is the moat; the lints are the supporting cast, tuned for teaching
errors (every diagnostic carries `why:` and `instead:`) rather than
maximal coverage.

**rust-analyzer's inlay hints** show parameter names in the editor,
which solves the *reading* problem for humans. They do nothing for the
*writing* problem: an agent emitting tokens never sees the hints, and a
reviewer scanning a diff outside the IDE doesn't either. Trust puts the
names in the source, where both the agent and the checker can act on
them.

## When not to use it

- **Single-author projects with no LLM contribution.** The dialect's
  whole reason to exist is to catch agent-authoring mistakes. A human
  writing Rust alone gets the cost (verbosity, activation, wrapper)
  without the benefit.
- **Teams that cannot tolerate a compiler wrapper in their build.**
  `cargo trust` is how the syntax extensions reach `rustc` (it sets
  `RUSTC_WRAPPER`/`RUSTDOC` internally). If your CI, vendor builds, or
  distro packaging cannot route through it, you get the lints but not
  named arguments or pipe.
- **Performance-sensitive hot paths that audit every line.** Lowering
  is source-to-source with no runtime cost — the extra dispatch claim
  is not real. But if your team's bar is "every line is reviewed by a
  human who knows the codebase cold," the bugs Trust catches were
  already going to be caught at review, and the verbosity tax is pure
  loss.
- **Codebases that lean heavily on `macro_rules!` or proc-macros.**
  R0008 bans user macros without explicit opt-in. The opt-in exists
  (`#[strict::macros_ok]`), but if every other file needs it, the rule
  is fighting the codebase instead of helping it.
- **Multi-crate workspaces, if you need *zero-config* cross-crate
  enforcement.** Cross-crate named arguments work today, but you must
  generate each dependency's signature index (`trust index`) and point
  the build at it via `TRUST_SIGNATURE_PATH` — Trust does not yet read
  those indices automatically from cargo's dependency graph. See the
  [`heck` case study](../case-studies/heck-strict.md) for what a
  single-crate adoption looks like end to end, including the
  workarounds, and the
  [`tre` case study](../case-studies/tre-strict.md) for the same
  exercise on an 8-file CLI with real I/O — which surfaces the
  per-file callee registry limitation (RT-40) as the biggest gap
  for multi-module adoption.

If none of those apply and you are shipping Rust written largely by
agents, the rest of the documentation starts at
[SPEC.md](SPEC.md).
