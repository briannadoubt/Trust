use anyhow::{Context, Result};
use std::env;
use std::process::Command;

/// `cargo-trust` is a cargo subcommand wrapper. When invoked as
/// `cargo trust <args>`, cargo prepends the literal `trust`
/// to the args; we strip it and forward the rest to `trust`.
fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let rest: Vec<String> = match args.next() {
        Some(first) if first == "trust" => args.collect(),
        Some(first) => std::iter::once(first).chain(args).collect(),
        None => Vec::new(),
    };
    forward(&rest)
}

fn forward(args: &[String]) -> Result<()> {
    let status = Command::new("trust")
        .args(args)
        .status()
        .context("invoking `trust` (is it on PATH?)")?;
    std::process::exit(status.code().unwrap_or(1));
}
