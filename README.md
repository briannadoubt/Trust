# Rustricted

A strict Rust dialect for LLM agents.

LLMs systematically mishandle a small, predictable set of Rust footguns:
positional arguments past arity 2, `.unwrap()` in production paths, `as` casts
that silently truncate, glob imports, undeclared side effects, macros whose
expansion isn't local. Rustricted is a thin layer over stable Rust that bans
those patterns and adds three extensions designed to make agent-authored code
reliable on the first compile:

1. **Named arguments**, mandatory past arity 1.
2. **Effect tracking** generalized beyond `async`.
3. **Pipe operator** `|>`.

Activate with `#![strict]`. Lower via `rustricted build` to plain Rust + `rustc`.

## Status

Phase 0 — scaffolding. The driver round-trips Rust source through `syn` and
`prettyplease`, then shells out to `rustc`. Lints, syntax extensions, and effect
tracking come in later phases.

## Build

```sh
cargo build --workspace
cargo test --workspace
cargo run -p rustricted -- build examples/00-hello.rs
./examples/00-hello
```

## License

MIT OR Apache-2.0.
