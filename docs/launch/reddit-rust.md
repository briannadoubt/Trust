# r/rust launch post

> Status: DRAFT — do not post. Timing is bri's call (RT-61).

## Title

> Trust: a strict Rust dialect targeting the bugs LLMs ship — named
> arguments checked against declarations, lowered to plain Rust

## Body

We kept seeing the same thing: agent-written Rust that compiles,
type-checks, and reads fine in review, with a same-typed argument swap
three files away from the definition. So we built a measurement harness
first and a dialect second.

**The eval** (all runs and the scoring harness are in the repo): five
single-file tasks, run per model in plain Rust and again under strict
mode, across Claude Haiku/Sonnet, GPT-4o, and Gemini 2.5 Flash. In
plain Rust, 9/15 files per model shipped a known-bad pattern —
positional-order swaps and silently-truncating `as` casts dominated.
Under strict mode: 0 shipped, every instance caught at build time.
Narrow claim, stated narrowly: two of the five audited bug classes
never fired at this scale, so their rules are untested, not vindicated.

**The dialect**, technically:

- **Named arguments past arity 1**, validated and *reordered* against
  the declaration: `make_rect(height: 5, width: 10)` lowers to
  `make_rect(10, 5)`. Cross-file within a crate via a crate-wide
  signature scan; cross-crate via generated signature indices.
- **A pipe operator** `e |> f(args)` → `f(e, args)`.
- **A strict lint set** (no-unwrap, no-as-cast, justify-unsafe with a
  `// safety:` comment contract, no same-typed adjacent params, ...) —
  every diagnostic carries `why:` and `instead:` because the primary
  reader is an agent that will retry.

Everything is token-level source-to-source: lowering runs in a
`RUSTC_WRAPPER` (and a `RUSTDOC` shim, because rustdoc ignores
RUSTC_WRAPPER and doc-tests would otherwise fail to parse). `cargo
trust build` wires it all up; activation is `[package.metadata.trust]
strict = true` or per-file `#![strict]`.

**Dogfood honesty**, because r/rust will ask: our own workspace is only
partially strict. The lowering/diagnostic crates build whole-package
strict in CI; the linter itself can't yet (its visitor internals
violate the named-arg rule in ways that need path-aware callee
resolution we haven't built); the bootstrap binaries are deliberately
plain Rust because a wrapper can't self-host. The case-studies
directory converts real crates (`heck`, `tre`) end-to-end and records
every gap we hit — several of which became fixed bugs this cycle.

MIT/Apache-2.0. Happy to answer anything about the lowering pipeline,
the eval design, or the rules.
