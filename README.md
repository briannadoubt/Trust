# Rustricted

A strict Rust dialect for LLM agents.

LLMs systematically mishandle a small, predictable set of Rust footguns:
positional arguments past arity 2, `.unwrap()` in production paths, `as` casts
that silently truncate, glob imports, macros whose expansion isn't local.
Rustricted is a thin layer over stable Rust that bans those patterns and
adds two extensions designed to make agent-authored code reliable on the
first compile:

1. **Named arguments**, mandatory past arity 1.
2. **Pipe operator** `|>`.

Activate with `#![strict]`. Lower via `rustricted build` to plain Rust + `rustc`.

The `check`, `build`, and `lower` subcommands accept `-` in place of an input
path to read source from stdin, matching the rustc/cat convention:

```
echo '#![strict]
fn main() { println!("hi"); }' | rustricted check -
```

`build -` additionally requires `--out PATH` because there is no input
filename to derive the binary name from.

[Why Rustricted?](docs/WHY.md) — the one-page rationale.

## Status

**Prototype.** The driver round-trips Rust source through `syn` and
`prettyplease`, then shells out to `rustc`. Sixteen lint rules are
implemented across `rustricted-lints` (strict mode) and `rustricted-lower`
(named-args, pipe). The syntax extensions — named arguments, pipe operator —
are implemented as token-level rewrites that lower to plain Rust.

Activation:
- Single-file inputs sent to `rustricted check` use the inner attribute
  `#![strict]` (stock `rustc` would reject this — Rustricted's toolchain
  handles it).
- Cargo-built crates use the `rustricted_attrs::strict!{}` marker macro
  from the `rustricted-attrs` proc-macro crate.

**What the eval supports.** The four runs in `eval/runs/` show that on a
small, deliberately-curated suite of single-file tasks, Haiku and Sonnet
both ship positional-argument bugs (R0042) and `as`-cast bugs (R0003) in
vanilla Rust, and the dialect catches them every time. The same suite
does **not** show that the dialect helps on .unwrap reflexes (R0001),
glob imports (R0004), macros, unsafe, or any multi-file scenario. The
generalised claim "the dialect catches LLM Rust bugs" is not yet
defensible from the data.

**What's missing for real-world use.** A cross-crate signature registry so
R0042 fires on calls to upstream code, an LSP, and a multi-crate workspace
story beyond "add the strict marker to each file." See
`case-studies/rustricted-syntax-strict.md` for a per-file dogfood
conversion and `eval/false-positives/REPORT.md` for the FP audit.

## Build

```sh
cargo build --workspace
cargo test --workspace
cargo run -p rustricted -- build examples/00-hello.rs
./examples/00-hello
```

## License

MIT OR Apache-2.0.
