//! `rustricted-rustc` — RUSTC_WRAPPER shim that runs Rustricted's lowering
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
//! - **Incremental cache.** The lowered output is cached in
//!   `$TMPDIR/rustricted-cache/<hash>/` keyed by an FNV-1a hash of the
//!   source content and the lowering-version constant. A cache hit skips
//!   re-lowering and re-checks entirely — cargo's own incremental logic
//!   often means the same file is submitted multiple times across targets.
//! - **Lowering diagnostics go to stderr.** They look like rustc-style
//!   notes but aren't structured for editor consumption.

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Version string mixed into the cache key. Bump this (or it bumps
/// automatically from the package version) whenever the lowering logic
/// changes in a way that would make cached output stale.
const LOWERING_VERSION: &str = env!("CARGO_PKG_VERSION");

/// FNV-1a 64-bit hash of the lowering-version string concatenated with the
/// source bytes. Fast, no deps, deterministic across processes and OSes.
fn source_cache_key(source: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in LOWERING_VERSION.bytes().chain(source.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

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

    let file_name = input_path
        .file_name()
        .context("input path has no file name")?;

    // Incremental cache: keyed by FNV-1a hash of (LOWERING_VERSION, source).
    // A cache hit means the same strict source was already lowered this
    // session (or a previous one before /tmp was cleared) — skip re-lowering.
    let cache_key = source_cache_key(&source);
    let cache_dir = env::temp_dir()
        .join("rustricted-cache")
        .join(format!("{cache_key:016x}"));
    let cached_file = cache_dir.join(file_name);

    if !cached_file.exists() {
        // Cache miss — lower, check, write.
        let out = rustricted_lower::lower(&source)
            .with_context(|| format!("lowering {}", input_path.display()))?;

        // Bubble lowering diagnostics to stderr so the user sees them. R0042,
        // R3001 are emitted from the lowering pass and would otherwise be
        // invisible under `cargo build`.
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

        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("creating cache dir {}", cache_dir.display()))?;
        // Write atomically: temp → rename, so a concurrent invocation never
        // sees a partial file.
        let tmp_write = cache_dir.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id()
        ));
        fs::write(&tmp_write, &out.source)
            .with_context(|| format!("writing {}", tmp_write.display()))?;
        // rename is atomic on POSIX within the same filesystem (tmp is).
        fs::rename(&tmp_write, &cached_file)
            .with_context(|| format!("renaming to {}", cached_file.display()))?;
    }

    let temp_file = &cached_file;

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
        cache_dir.display(),
        parent.display()
    ));

    run_rustc(rustc, &new_args)
}

fn run_rustc(path: &str, args: &[String]) -> Result<i32> {
    let status = Command::new(path)
        .args(args)
        .status()
        .with_context(|| format!("invoking {path}"))?;
    Ok(status.code().unwrap_or(1))
}
