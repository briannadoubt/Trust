# Trust

**A strict Rust dialect for the bugs LLMs actually ship.**

Agents write Rust that compiles, type-checks, and reviews clean — then ships a
small, predictable set of bugs: positional arguments in the wrong order,
`.unwrap()` in production paths, `as` casts that silently truncate, glob
imports. Add `strict = true` to `Cargo.toml` (or `#![strict]` to a file) and
those become compile errors with a fix in the message. In our eval, **60% of agent-authored files shipped one of these bugs
in plain Rust; 0% shipped under Trust** — across four models from three
vendors.

[![CI](https://github.com/briannadoubt/Trust/actions/workflows/ci.yml/badge.svg)](https://github.com/briannadoubt/Trust/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/trust-lang.svg)](https://crates.io/crates/trust-lang)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

## What Trust is

Trust **is** a strict dialect of Rust — a thin layer that turns the bugs agents
ship (wrong argument order, `.unwrap()`, silent `as` casts) into compile errors
with a fix in the message, caught in **one pass** before the code ever runs.
That dialect is the product, and it's what the [eval numbers](#the-numbers)
measure: 60% of agent files shipped a bug in plain Rust, **0% under the dialect**.

You can run it two ways — but they are not equals:

1. **The dialect — a build gate.** Opt in via `Cargo.toml`, build with `cargo
   trustc`, and the full rule set — including named-argument enforcement
   (R0042), the rule that makes the argument-swap bug *unrepresentable* — is
   enforced at compile time. **This is Trust.**
2. **An advisory linter — an on-ramp.** Not ready to change how you build? Point
   the `trust` CLI at existing plain Rust and it *reports* the subset of rules
   that work without the dialect — zero commitment, partial value, a way to see
   what Trust catches before you switch the build over. It cannot enforce R0042,
   and it reports rather than blocks.

## Install

```sh
# The dialect (the build gate) — the CLI, the cargo subcommand, and the two
# lowering shims (cargo doesn't install a dependency's binaries):
cargo install trust-lang cargo-trustc trust-rustc trust-rustdoc

# Or just the advisory linter — only the `trust` CLI (the crate is `trust-lang`):
cargo install trust-lang
```

All four crates are published on crates.io (latest: **0.2.0**); MSRV is **Rust
1.85**. Building from source also works — see [From source](#from-source) below.

## The dialect — two steps

```toml
# Cargo.toml
[package.metadata.trust]
strict = true
```

```sh
cargo trustc build    # also: run, test, check, clippy, doc, bench
```

That's the whole setup. `cargo trustc` wires the lowering shims into cargo
itself — no environment variables, no per-file markers, no extra
dependencies — and enforces the full rule set at build time: the syntax
extensions lower, and every strict lint (`.unwrap()`, `as`-casts, positional
args, …) is a build error with a fix in the message. Dependencies are never
touched. `[workspace.metadata.trust] strict = true` opts in a whole
workspace at once. Because the opt-in lives in `Cargo.toml` metadata (which
stock cargo ignores), **every source file stays a valid plain `cargo build`** —
nothing in your `.rs` files changes.

## Try it first — the advisory linter

New to Trust, or not ready to change your build? Run the bug-catching rules over
your existing plain Rust — no marker, no metadata, no dialect:

```sh
trust check --rules bugs  src/     # the runtime-bug rules
trust check --rules safety src/    # every rule that applies to plain Rust
```

`trust check` takes a file, a directory, or a `Cargo.toml` and walks the tree —
one command, CI-ready (non-zero exit on findings). Nothing is added to your
source, so it can't break a normal `cargo build`. Tune it in a `trust.toml`:

```toml
# trust.toml — at the project root
rules = "bugs"          # default selection (--rules overrides)
allow = ["R0012"]       # dropped project-wide
warn  = ["R0017"]       # kept, but a non-failing warning
```

Emit `--format json` (agent-native) or `--format sarif` (GitHub code-scanning);
`trust fix <file> --safety` rewrites `.unwrap()`/`.expect(…)` → `?`. **Mind the
ceiling, though:** advisory mode *reports* a subset and **cannot enforce R0042**
(named arguments) — the rule that prevents the argument-swap bug and the
strongest result in the eval. For one-pass *enforcement* rather than
after-the-fact reports, you need the dialect above. The advisory linter is the
doorway; the dialect is the room.

## What it looks like

The bug class Trust catches most reliably is positional argument order. A model
that defines `make_rect(width, height)` will, three files later, call it
`make_rect(height, width)`. Nothing downstream notices.

```rust
// Plain Rust — compiles, ships the swap.
let area = make_rect(height, width);
```

```rust
#![strict]
// Trust — rejected at `trust check` with R0042; names make the order explicit.
let area = make_rect(width: 1920, height: 1080);   // order is now free and checked
```

> **On the `#![strict]` marker:** it is understood only by the Trust toolchain
> and is **not valid stock Rust** — a file carrying it fails a plain `cargo
> build` with `cannot find attribute 'strict'`. Build marked files with `cargo
> trustc` (or check single files with `trust`). For committed code that must
> also build under stock cargo, prefer the `[package.metadata.trust]` opt-in
> above — it's invisible to stock cargo and leaves every file a valid plain
> build. See [docs/SPEC.md § Activation](docs/SPEC.md#activation).

Trust is a thin layer over **stable Rust**. The named-argument and pipe (`|>`)
syntax lower to plain positional Rust before `rustc` ever sees the file; the
lints are ordinary static checks. The output is plain Rust source handed to a
stock compiler — stop using Trust tomorrow and the lowered code your codebase
produced still builds.

## The numbers

Same five single-file tasks, run twice per model — once in plain Rust
(`vanilla`), once with `#![strict]` (`trust`). *Shipped* means the known-bad
pattern was present **and** the dialect did not catch it. Lower is better; it
is the only column that matters.

| Model | Vendor | Vanilla shipped | Trust shipped | Run |
|-------|--------|-----------------|---------------|-----|
| Claude Haiku | Anthropic | 9/15 (60%) | **0/15 (0%)** | [002](eval/runs/002/summary.md) |
| Claude Sonnet | Anthropic | 9/15 (60%) | **0/15 (0%)** | [004](eval/runs/004/summary.md) |
| GPT-4o | OpenAI | 9/15 (60%) | **0/15 (0%)** | [005](eval/runs/005/summary.md) |
| Gemini 2.5 Flash | Google | 9/15 (60%) | **0/14 (0%)** | [006](eval/runs/006/summary.md) |

The honest reading: of the five tasks, three (positional order R0042, `as`-cast
R0003) reliably elicited bugs and the dialect caught **every one**. The other
two (`.unwrap()` R0001, glob imports R0004) elicited **zero** bugs from these
models at this scale — those rules weren't vindicated by the eval, they simply
weren't tested. The claim the data supports is narrow and strong: *on the
audited bug classes, agents ship them ~60% of the time and Trust catches
100%.* It is not "Trust makes agent Rust bug-free." Full per-task tables,
notes, and the scoring harness are in [`eval/`](eval/).

## Why

Rust already makes a huge class of bugs unrepresentable. The gap between "Rust"
and "Rust an agent gets right on the first try" is small and nameable — it's the
list above. Trust closes that gap and nothing else: no new type system, no
runtime, no replacement std. See **[docs/WHY.md](docs/WHY.md)** for the full
rationale and how it compares to Sorbet, Mypy, and the TypeScript-over-JS
playbook.

## Status

**0.2 — agent-authored, evaluation-backed prototype.** The core is the
**dialect**: 18 rules across `trust-lints` (strict mode) and `trust-lower`
(named args, pipe), enforced at build time by `cargo trustc` (`RUSTC_WRAPPER`/
`RUSTDOC` shims, `[package.metadata.trust]` activation). Cross-crate
named-argument enforcement works against a signature index from `trust index`
([`examples/cross-crate-index`](examples/cross-crate-index/)). The **advisory
linter** (`trust check --rules bugs|safety` over plain Rust, dir/workspace walk,
`trust.toml`, JSON/SARIF, `trust fix --safety`, LSP) is a 0.2 on-ramp — the
dialect-free subset, for trying Trust before adopting it. Active gaps: zero-config
discovery of signature indices from cargo's dependency graph, honoring
`trust.toml` in the build gate, and editor packaging. MSRV is Rust 1.85.
Validated by 6 eval runs and 4 case-study conversions of real crates. MIT OR
Apache-2.0 — see the [case studies](#case-studies) and `eval/` for exactly what
is and isn't proven.

---

## From source

```sh
git clone https://github.com/briannadoubt/trust && cd trust
cargo build --workspace
cargo test --workspace

cargo run -p trust-lang -- build examples/00-hello.rs --out /tmp/hello && /tmp/hello
cargo run -p trust-lang -- check examples/01-lints/positional-fail.rs   # fails with R0042

# the two-step cargo flow, against this checkout:
export PATH="$PWD/target/debug:$PATH"     # cargo-trustc + shims
(cd examples/cargo-strict-config && cargo trustc run)   # zero markers, zero env vars

# or scaffold a fresh strict project in one command:
cargo trustc new demo && (cd demo && cargo trustc run)
```

The `check`, `build`, and `lower` subcommands accept `-` in place of a path to
read source from stdin:

```sh
printf '#![strict]\nfn main() { println!("hi"); }\n' | trust check -
```

(`build -` additionally needs `--out PATH`, since there's no filename to derive
the binary name from.)

## How `cargo trustc` works

`cargo trustc build` is exactly `cargo build` with `RUSTC_WRAPPER` and
`RUSTDOC` pointed at two thin shims (`trust-rustc`, `trust-rustdoc`) that
lower each strict file to plain positional Rust — and run the lints — before
the real compiler sees it. `RUSTDOC` matters because rustdoc ignores
`RUSTC_WRAPPER`; without the second shim, doc-tests using named-arg syntax
would fail to parse under `cargo trustc test --doc`. There is no custom
compiler anywhere: stop using Trust tomorrow and the lowered output still
builds on stock rustc.

Prefer file-by-file adoption over the project-wide key? Put `#![strict]` at
the top of just the files you want checked — `cargo trustc` handles either
form. See [docs/SPEC.md § Activation](docs/SPEC.md). If you need raw
`cargo`, the wrapper env vars still work
(`RUSTC_WRAPPER=…/trust-rustc RUSTDOC=…/trust-rustdoc cargo build`).

## Editor integration (LSP)

`trust-lsp` speaks LSP over stdio and runs the same lower + lint pipeline as the
CLI, publishing diagnostics live plus hover and go-to-definition for local
functions.

```sh
cargo build -p trust-lsp --release     # binary: target/release/trust-lsp
```

Point any LSP client at the binary (it takes no flags). For **Neovim**:

```lua
vim.lsp.start({
  name = "trust",
  cmd = { "/path/to/target/release/trust-lsp" },
  root_dir = vim.fs.dirname(vim.fs.find({ "Cargo.toml" }, { upward = true })[1]),
})
```

A dedicated VS Code extension lives in [`editors/vscode/`](editors/vscode/).

## Learn more

- **[docs/WHY.md](docs/WHY.md)** — the rationale, the numbers, and the neighbors (Sorbet, Mypy, TS).
- **[docs/SPEC.md](docs/SPEC.md)** — the full rule catalogue (R0001…R0042) and grammar for the two syntax extensions.
- **[docs/RATIONALE.md](docs/RATIONALE.md)** — phase-by-phase reasoning for each rule.
- **[docs/AGENTS.md](docs/AGENTS.md)** — the teaching-error contract every diagnostic follows.
- **[docs/templates/CLAUDE-md-snippet.md](docs/templates/CLAUDE-md-snippet.md)** — drop-in agent instructions for projects that adopt Trust.
- **[skills/](skills/README.md)** — agent skills for writing Trust, installable via the Claude Code plugin marketplace.

### Case studies

- [`heck`](case-studies/heck-strict.md) — a pure library, converted end to end.
- [`tre`](case-studies/tre-strict.md) — an 8-file CLI with real I/O; surfaces the cross-file gaps.
- [`trust-syntax`](case-studies/trust-syntax-strict.md) — per-file dogfood of our own crate.
- [dogfooding](case-studies/dogfooding.md) — which `trust-*` crates build under `#![strict]` today.
- [false-positive audit](eval/false-positives/REPORT.md) — the FP sweep against `anyhow` and the workspace.

## License

MIT OR Apache-2.0.
