//! `rustricted-rustc` — RUSTC_WRAPPER shim that runs Rustricted's lowering
//! pass on each strict-marked `.rs` file before handing it to the real
//! `rustc`. Set as `RUSTC_WRAPPER` to make `cargo build` understand the
//! dialect's syntax extensions (named args, pipe, `effect`) on cargo crates.
//!
//! ## How cargo invokes a RUSTC_WRAPPER
//!
//! When `RUSTC_WRAPPER=<path>` is set, cargo calls every rustc invocation
//! as `<wrapper> <rustc-path> <rustc-args...>`. This binary:
//!
//! 1. Finds the input `.rs` file in the rustc args (cargo passes exactly
//!    one per invocation, the crate root).
//! 2. If the file lacks a strict marker (`#![strict]` or
//!    `rustricted_attrs::strict!{}` / `strict!{}`), passes through to rustc
//!    unchanged.
//! 3. Otherwise lowers the file via `rustricted_lower::lower`, writes the
//!    result to a temp file, rewrites the rustc arg to point at it, adds a
//!    `--remap-path-prefix=<temp>=<original>` so diagnostics look familiar,
//!    and exec's the real rustc.
//!
//! ## Phase 0 scope (knowingly limited)
//!
//! - **Single-file lowering.** Only the `.rs` file passed to rustc is
//!   lowered. Child modules pulled in via `mod foo;` are loaded by rustc
//!   from the *original* on-disk paths and are NOT lowered. So a crate
//!   where `lib.rs` is strict but `lib.rs` contains `mod helpers;` whose
//!   `helpers.rs` uses named args will fail to compile. The workaround
//!   today is: keep strict crates single-file, or apply lowering manually
//!   per file before checking in.
//! - **No incremental cache.** Every rustc invocation re-lowers the file.
//!   Fine for prototypes; would matter for large workspaces.
//! - **Lowering diagnostics go to stderr.** They look like rustc-style
//!   notes but aren't structured for editor consumption.

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(e) => {
            eprintln!("rustricted-rustc: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32> {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.is_empty() {
        bail!("usage: rustricted-rustc <real-rustc-path> [rustc-args...]");
    }
    let rustc = &argv[0];
    let rustc_args: Vec<String> = argv[1..].to_vec();

    // Find the input .rs file argument. Cargo passes one per crate
    // invocation; flag arguments (`-Cfoo=bar`, `--edition=2021`) start
    // with `-`, and bare `-` means "read from stdin" (we pass through).
    let input_idx = rustc_args.iter().enumerate().find_map(|(i, a)| {
        if a == "-" {
            return None;
        }
        if a.ends_with(".rs") && !a.starts_with('-') {
            Some(i)
        } else {
            None
        }
    });

    let Some(idx) = input_idx else {
        // Probe / metadata invocation with no .rs input — pass through.
        return run_rustc(rustc, &rustc_args);
    };

    let input_path = PathBuf::from(&rustc_args[idx]);
    let source = match fs::read_to_string(&input_path) {
        Ok(s) => s,
        Err(_) => return run_rustc(rustc, &rustc_args),
    };

    if !rustricted_lower::is_strict_source(&source) {
        // Non-strict file — cargo's normal behaviour. Pass through.
        return run_rustc(rustc, &rustc_args);
    }

    // Lower.
    let out = rustricted_lower::lower(&source)
        .with_context(|| format!("lowering {}", input_path.display()))?;

    // Bubble lowering diagnostics to stderr so the user sees them. R0042,
    // R3001, R4001 are emitted from the lowering pass and would otherwise
    // be invisible under `cargo build`.
    for diag in &out.diagnostics {
        eprintln!(
            "[{}] {}: {}",
            diag.rule,
            if diag.is_error() { "error" } else { "warning" },
            diag.message
        );
    }
    if out.diagnostics.iter().any(|d| d.is_error()) {
        bail!("rustricted check failed on {}", input_path.display());
    }

    // Write the lowered source to a per-invocation temp file.
    let temp_dir = env::temp_dir().join(format!(
        "rustricted-rustc-{}-{}",
        std::process::id(),
        input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("input")
    ));
    fs::create_dir_all(&temp_dir).with_context(|| format!("creating {}", temp_dir.display()))?;
    let file_name = input_path
        .file_name()
        .context("input path has no file name")?;
    let temp_file = temp_dir.join(file_name);
    fs::write(&temp_file, &out.source)
        .with_context(|| format!("writing {}", temp_file.display()))?;

    // Substitute the lowered path in the rustc args and remap so
    // diagnostics still point at the original location.
    let mut new_args = rustc_args.clone();
    new_args[idx] = temp_file.to_string_lossy().into_owned();
    let parent = input_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    new_args.push(format!(
        "--remap-path-prefix={}={}",
        temp_dir.display(),
        parent.display()
    ));

    let exit = run_rustc(rustc, &new_args);

    // Best-effort cleanup. Failures here don't affect the build result.
    let _ = fs::remove_dir_all(&temp_dir);

    exit
}

fn run_rustc(path: &str, args: &[String]) -> Result<i32> {
    let status = Command::new(path)
        .args(args)
        .status()
        .with_context(|| format!("invoking {path}"))?;
    Ok(status.code().unwrap_or(1))
}
