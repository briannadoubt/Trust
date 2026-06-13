# cargo-trustc

Cargo subcommand for the Trust toolchain — `cargo trustc build/run/test` with the lowering shims wired in

Part of [**Trust**](https://github.com/briannadoubt/trust) — a strict Rust dialect (named arguments, a pipe operator, contracts) with bug-catching lints that also run as an advisory linter over plain Rust.

## Install

```sh
cargo install cargo-trustc
```

Then build/run/test a Trust crate with zero environment setup:

```sh
cargo trustc build
cargo trustc run
cargo trustc adopt    # onboard an existing crate into the dialect
```

See the [main README](https://github.com/briannadoubt/trust#readme) for the full guide.

## License

Licensed under either of [MIT](https://github.com/briannadoubt/trust/blob/main/LICENSE-MIT) or [Apache-2.0](https://github.com/briannadoubt/trust/blob/main/LICENSE-APACHE) at your option.
