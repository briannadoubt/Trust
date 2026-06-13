# trust-lang

CLI driver for the Trust dialect

Part of [**Trust**](https://github.com/briannadoubt/trust) — a strict Rust dialect (named arguments, a pipe operator, contracts) with bug-catching lints that also run as an advisory linter over plain Rust.

## Install

```sh
cargo install trust-lang
```

This installs the `trust` CLI:

```sh
trust check --rules bugs src/        # advisory lint over plain Rust
trust build path/to/file.rs --out a  # lower + compile a dialect file
trust fix --write .                  # migrate a tree into the dialect
```

See the [main README](https://github.com/briannadoubt/trust#readme) for the full guide.

## License

Licensed under either of [MIT](https://github.com/briannadoubt/trust/blob/main/LICENSE-MIT) or [Apache-2.0](https://github.com/briannadoubt/trust/blob/main/LICENSE-APACHE) at your option.
