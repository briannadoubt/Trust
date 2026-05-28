# Trust

A strict Rust dialect for LLM agents.

LLMs systematically mishandle a small, predictable set of Rust footguns:
positional arguments past arity 2, `.unwrap()` in production paths, `as` casts
that silently truncate, glob imports, macros whose expansion isn't local.
Trust is a thin layer over stable Rust that bans those patterns and
adds two extensions designed to make agent-authored code reliable on the
first compile:

1. **Named arguments**, mandatory past arity 1.
2. **Pipe operator** `|>`.

Activate with `#![strict]`. Lower via `trust build` to plain Rust + `rustc`.

The `check`, `build`, and `lower` subcommands accept `-` in place of an input
path to read source from stdin, matching the rustc/cat convention:

```
echo '#![strict]
fn main() { println!("hi"); }' | trust check -
```

`build -` additionally requires `--out PATH` because there is no input
filename to derive the binary name from.

[Why Trust?](docs/WHY.md) — the one-page rationale.

## Status

**Prototype.** The driver round-trips Rust source through `syn` and
`prettyplease`, then shells out to `rustc`. Sixteen lint rules are
implemented across `trust-lints` (strict mode) and `trust-lower`
(named-args, pipe). The syntax extensions — named arguments, pipe operator —
are implemented as token-level rewrites that lower to plain Rust.

Activation:
- Single-file inputs sent to `trust check` use the inner attribute
  `#![strict]` (stock `rustc` would reject this — Trust's toolchain
  handles it).
- Cargo-built crates use the `trust_attrs::strict!{}` marker macro
  from the `trust-attrs` proc-macro crate.

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
`case-studies/trust-syntax-strict.md` for a per-file dogfood
conversion and `eval/false-positives/REPORT.md` for the FP audit.

## Build

```sh
cargo build --workspace
cargo test --workspace
cargo run -p trust -- build examples/00-hello.rs
./examples/00-hello
```

## Using strict source from `cargo`

For `cargo build` / `cargo test` to accept Trust syntax extensions
(named args, pipe) you need both wrappers set:

```sh
cargo build -p trust-rustc -p trust-rustdoc
export RUSTC_WRAPPER=$(realpath target/debug/trust-rustc)
export RUSTDOC=$(realpath target/debug/trust-rustdoc)
cargo build         # lowers .rs files before rustc
cargo test --doc    # also lowers code inside doc comments
```

`RUSTC_WRAPPER` covers ordinary builds and unit/integration tests.
`RUSTDOC` (or `RUSTDOC_WRAPPER` on cargo versions that support it) is
needed separately because rustdoc does *not* honour `RUSTC_WRAPPER` —
without it, doc-tests that use named-arg syntax fail with a rustc parse
error. See `docs/SPEC.md` for details.

## Editor integration (LSP)

`trust-lsp` is a Language Server that speaks LSP over stdio. It runs
the same lower + lint pipeline as the CLI and publishes diagnostics live,
plus minimal hover (named-arg + callee signature) and go-to-definition
(local functions).

Build the server, then point your editor at the resulting binary:

```sh
cargo build -p trust-lsp --release
# binary path: target/release/trust-lsp
```

Editor wiring (any LSP client works; the binary takes no flags):

- **VS Code**: install any "generic LSP client" extension and configure
  it to launch `target/release/trust-lsp` for `*.rs` in Trust
  projects. (A dedicated VS Code extension is a follow-up.)
- **Neovim** (with `nvim-lspconfig`):
  ```lua
  vim.lsp.start({
    name = "trust",
    cmd = { "/path/to/target/release/trust-lsp" },
    root_dir = vim.fs.dirname(vim.fs.find({ "Cargo.toml" }, { upward = true })[1]),
  })
  ```
- **Helix** (`languages.toml`):
  ```toml
  [[language]]
  name = "rust"
  language-servers = ["trust-lsp"]
  [language-server.trust-lsp]
  command = "/path/to/target/release/trust-lsp"
  ```

Current capabilities: full-sync diagnostics, hover, go-to-def. Completion,
code actions, formatting, cross-file go-to-def, and watch-config are
follow-ups (see scope tickets).

## License

MIT OR Apache-2.0.
