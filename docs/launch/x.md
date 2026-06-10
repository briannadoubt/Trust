# X / Twitter launch thread

> Status: DRAFT — do not post. Timing is bri's call (RT-61).

## Tweet 1 (the hook)

LLMs write Rust that compiles, type-checks, and reviews clean — then
ship `make_rect(height, width)`.

We measured it: ~60% of agent-written files shipped a greppable bug
class. So we built Trust: strict Rust where that's a compile error.

60% → 0%, across 4 models, 3 vendors. 🧵

## Tweet 2 (what it is)

Trust = stable Rust + named arguments + a strict lint set.

`make_rect(width: 1920, height: 1080)` — checked against the
declaration, reorderable, lowered to plain positional Rust before rustc
ever sees it.

No fork. No runtime. Stop using it tomorrow, the output still builds.

## Tweet 3 (the setup)

Two steps:

```toml
[package.metadata.trust]
strict = true
```

```sh
cargo trustc build
```

Every diagnostic teaches: rule code, why it exists, what to write
instead. Built for an author that retries.

## Tweet 4 (the honest number)

The claim is narrow on purpose: on the audited bug classes (positional
swaps, `as`-cast truncation), agents shipped them ~60% of the time and
Trust caught 100%.

Not "bug-free Rust." The eval harness, prompts, and every run log are
in the repo.

## Tweet 5 (the close)

Repo: https://github.com/briannadoubt/trust

Why-doc with the Sorbet/mypy/TypeScript comparison, real-crate case
studies (including what *didn't* convert cleanly), and the rule
catalogue. MIT/Apache-2.0.
