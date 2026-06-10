# Hacker News launch post

> Status: DRAFT — do not post. Timing is bri's call (RT-61).

## Title (under 80 chars)

> Show HN: Trust – a strict Rust dialect for the bugs LLMs actually ship

(Alternative if "dialect" draws pedantry: `Show HN: Trust – strict Rust
for agent-written code. 60% bug rate → 0% in our eval`)

## URL

https://github.com/briannadoubt/trust

## Text (optional body — HN Show posts allow it)

Agents write Rust that compiles, type-checks, and reviews clean — then
ship a small, predictable set of bugs: positional arguments in the wrong
order, `.unwrap()` in production paths, `as` casts that silently
truncate. We measured it: across four models from three vendors (Claude
Haiku/Sonnet, GPT-4o, Gemini 2.5 Flash), ~60% of agent-authored files
shipped one of these bugs in plain Rust. With Trust's strict mode, 0%
shipped — every instance was caught at build time with a fix in the
message.

Trust is a thin layer over stable Rust: two grammar extensions (named
arguments past arity 1, a pipe operator) that lower to plain positional
Rust before rustc sees the file, plus a strict lint set tuned for
teaching errors. Setup is two steps: `strict = true` in Cargo.toml,
`cargo trust build`. There is no custom compiler — stop using Trust
tomorrow and the lowered output still builds on stock rustc.

The honest version of the eval claim (the repo says this too): of five
task types, three reliably elicited bugs and Trust caught every one; the
other two elicited zero bugs at this scale, so those rules weren't
tested. It's "100% catch rate on the audited bug classes," not "bug-free
Rust."

## First comment (pre-empting the obvious questions — post from bri's account immediately)

A few questions I expect, answered up front:

**"Why not just Clippy?"** Most of the strict lints do have Clippy
analogues, and if lints were the whole story this wouldn't need to
exist. The one thing Clippy structurally cannot catch is a same-typed
positional-argument swap — `make_rect(height, width)` against
`make_rect(width: u32, height: u32)` is type-correct, so no analysis of
existing syntax can flag it. The fix needs syntax Rust doesn't have:
names at the call site, checked against the declaration. That's the
moat; it was also the single biggest bug class in our eval.

**"Why not rust-analyzer inlay hints?"** Hints solve the *reading*
problem for humans in an IDE. They do nothing for the *writing*
problem: the agent emitting tokens never sees them, and neither does a
reviewer reading a diff.

**"Newtypes already solve argument swaps."** They do, when you use them
— and Trust ships a one-line `newtype!` helper and an R-rule pushing you
toward distinct types. Named arguments cover the long tail where a
newtype per parameter is unergonomic (think `fs::rename(from, to)`).

**"Is this a fork of rustc?"** No. Source-to-source lowering + stock
rustc via a build wrapper that `cargo trust` manages for you. The
lowered output is plain Rust you could commit.

**"What's the catch?"** Verbose call sites (that's the point), a
compiler wrapper in your build, and an honest list of current gaps in
the README — including which crates of our own workspace can't be fully
strict yet and why.
