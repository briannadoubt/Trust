# Trust

**A strict Rust dialect for the bugs LLMs actually ship.**

Agents write Rust that compiles, type-checks, and reviews clean — then ships a
small, predictable set of bugs: positional arguments in the wrong order,
`.unwrap()` in production paths, `as` casts that silently truncate, glob
imports. Add `#![strict]` and those become compile errors with a fix in the
message. In our eval, **60% of agent-authored files shipped one of these bugs
in plain Rust; 0% shipped under Trust** — across four models from three
vendors.

```
[badges: build · crates.io · license — added with the public push, RT-57/58]
```

## Install

```sh
cargo install trust        # the CLI: `trust check`, `trust build`, `trust lower`
```

> **0.1 is not on crates.io yet** (publishing is the last launch step, RT-58).
> Until then, build from source — see [From source](#from-source) below. The
> command above is what `cargo install` will look like the day 0.1 lands.

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

**0.1 — agent-authored, evaluation-backed prototype.** 18 rules across
`trust-lints` (strict mode) and `trust-lower` (named args, pipe); a working CLI,
`RUSTC_WRAPPER`/`RUSTDOC` shims for cargo, and an LSP server (diagnostics,
hover, go-to-def). Cross-crate named-argument enforcement works against a
signature index extracted from any crate with `trust index`
([`examples/cross-crate-index`](examples/cross-crate-index/)); zero-config
discovery of those indices from cargo's dependency graph, and editor packaging,
are the active gaps. Validated by 6 eval runs and 4 case-study conversions of
real crates. MIT OR Apache-2.0 — see the [case studies](#case-studies) and
`eval/` for exactly what is and isn't proven.

---

## From source

```sh
git clone https://github.com/briannadoubt/trust && cd trust
cargo build --workspace
cargo test --workspace

cargo run -p trust -- build examples/00-hello.rs --out /tmp/hello && /tmp/hello
cargo run -p trust -- check examples/01-lints/positional-fail.rs   # fails with R0042
```

The `check`, `build`, and `lower` subcommands accept `-` in place of a path to
read source from stdin:

```sh
printf '#![strict]\nfn main() { println!("hi"); }\n' | trust check -
```

(`build -` additionally needs `--out PATH`, since there's no filename to derive
the binary name from.)

## Using strict source from `cargo`

The lints work on any toolchain. The **syntax extensions** (named args, pipe)
need a wrapper, because the rewrite has to run before `rustc` sees the file and
`cargo build` calls `rustc` directly:

```sh
cargo build -p trust-rustc -p trust-rustdoc
export RUSTC_WRAPPER=$(realpath target/debug/trust-rustc)
export RUSTDOC=$(realpath target/debug/trust-rustdoc)
cargo build         # lowers .rs files before rustc
cargo test --doc    # also lowers code inside doc comments
```

`RUSTC_WRAPPER` covers ordinary builds and unit/integration tests. `RUSTDOC` is
needed separately because rustdoc does not honour `RUSTC_WRAPPER` — without it,
doc-tests using named-arg syntax fail to parse. Activation differs by build
mode: single-file inputs use the inner attribute `#![strict]`; cargo-built
crates use the `trust_attrs::strict!{}` marker macro (stock `rustc` rejects
`#![strict]`). See [docs/SPEC.md § Activation](docs/SPEC.md).

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

### Case studies

- [`heck`](case-studies/heck-strict.md) — a pure library, converted end to end.
- [`tre`](case-studies/tre-strict.md) — an 8-file CLI with real I/O; surfaces the cross-file gaps.
- [`trust-syntax`](case-studies/trust-syntax-strict.md) — per-file dogfood of our own crate.
- [dogfooding](case-studies/dogfooding.md) — which `trust-*` crates build under `#![strict]` today.
- [false-positive audit](eval/false-positives/REPORT.md) — the FP sweep against `anyhow` and the workspace.

## License

MIT OR Apache-2.0.
