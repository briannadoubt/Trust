# Lobsters launch post

> Status: DRAFT — do not post. Timing is bri's call (RT-61).

## Title

> Trust: a strict Rust dialect for LLM-written code (named args, lowered
> to plain Rust, 60%→0% ship rate in a 4-model eval)

## Tags

`rust`, `compilers`, `ai`

## URL

https://github.com/briannadoubt/trust

## Comment (post alongside the link)

The design constraint that shaped everything: the primary *author* is an
agent, the primary *reader* is a human, and the output had to remain
plain Rust someone can walk away with.

Mechanically it's the TypeScript playbook applied to call-site
explicitness instead of types: a thin frontend lowers two grammar
extensions (mandatory named arguments past arity 1, a pipe operator) to
positional Rust before stock rustc sees the file, and a lint preset
turns the classic agent footguns (`.unwrap()`, `as` truncation,
unjustified `unsafe`) into teaching errors with `why:`/`instead:` text
aimed at a model that will retry.

The eval is small but multi-vendor and fully reproducible from the repo
(runs, prompts, scoring harness): ~60% of plain-Rust agent files shipped
an audited bug class; 0% under strict mode. The README is explicit about
which rules that does and doesn't validate.

The part I'd most like feedback on from this crowd: the wrapper
architecture (RUSTC_WRAPPER + a rustdoc shim, content-hash lowering
cache) and where it breaks — we've documented the self-hosting limits in
the dogfooding case study, and the cache has already taught us one
humbling lesson about partial state this cycle.
