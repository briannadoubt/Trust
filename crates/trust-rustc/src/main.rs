
//! `trust-rustc` — RUSTC_WRAPPER shim that runs Trust's lowering
//! pass on each strict-marked `.rs` file before handing it to the real
//! `rustc`. Set as `RUSTC_WRAPPER` to make `cargo build` understand the
//! dialect's syntax extensions (named args, pipe) on cargo crates.
//!
//! ## How cargo invokes a RUSTC_WRAPPER
//!
//! When `RUSTC_WRAPPER=<path>` is set, cargo calls every rustc invocation
//! as `<wrapper> <rustc-path> <rustc-args...>`. This binary:
//!
//! 1. Finds the input `.rs` file in the rustc args (cargo passes exactly
//!    one per invocation, the crate root).
//! 2. If the file lacks a strict marker, passes through to rustc unchanged.
//! 3. Otherwise lowers the file (and its module tree) via the shared logic
//!    in `trust_rustc::prepare_strict_input`, rewrites the rustc arg
//!    to point at the cached lowered file, adds a `--remap-path-prefix`,
//!    and exec's the real rustc.
//!
//! The doc-test sibling `trust-rustdoc` (set as `RUSTDOC`) reuses the
//! same lowering/cache layer — see `src/lib.rs`.

// Stage-0 bootstrap crate (RT-76): plain Rust, built by stock `cargo`.
// This binary IS the RUSTC_WRAPPER that lowers strict crates, so it cannot
// require itself to build — it must stay free of the syntax extensions.

use anyhow::{bail, Context, Result};
use trust_rustc::{find_input_rs, prepare_strict_input};
use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(e) => {
            eprintln!("trust-rustc: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32> {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.is_empty() {
        bail!("usage: trust-rustc <real-rustc-path> [rustc-args...]");
    }
    let rustc = &argv[0];
    let rustc_args: Vec<String> = argv[1..].to_vec();

    let Some(idx) = find_input_rs(&rustc_args) else {
        return run_rustc(rustc, &rustc_args);
    };

    let input_path = PathBuf::from(&rustc_args[idx]);
    let Some(prepared) = prepare_strict_input(&input_path)
        .with_context(|| format!("preparing {}", input_path.display()))?
    else {
        return run_rustc(rustc, &rustc_args);
    };

    let mut new_args = rustc_args.clone();
    new_args[idx] = prepared.lowered_root.to_string_lossy().into_owned();
    new_args.push(prepared.remap_flag);
    run_rustc(rustc, &new_args)
}

fn run_rustc(path: &str, args: &[String]) -> Result<i32> {
    let status = Command::new(path)
        .args(args)
        .status()
        .with_context(|| format!("invoking {path}"))?;
    Ok(status.code().unwrap_or(1))
}
