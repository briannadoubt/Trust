use anyhow::{Context, Result};
use std::env;
use std::process::Command;

/// `cargo-rustricted` is a cargo subcommand wrapper. When invoked as
/// `cargo rustricted <args>`, cargo prepends the literal `rustricted`
/// to the args; we strip it and forward the rest to `rustricted`.
fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let rest: Vec<String> = match args.next() {
        Some(first) if first == "rustricted" => args.collect(),
        Some(first) => std::iter::once(first).chain(args).collect(),
        None => Vec::new(),
    };
    forward(&rest)
}

fn forward(args: &[String]) -> Result<()> {
    let status = Command::new("rustricted")
        .args(args)
        .status()
        .context("invoking `rustricted` (is it on PATH?)")?;
    std::process::exit(status.code().unwrap_or(1));
}
