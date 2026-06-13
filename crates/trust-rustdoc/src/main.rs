//! `trust-rustdoc` — `RUSTDOC` (and `RUSTDOC_WRAPPER`) shim that runs
//! Trust's lowering pass before invoking the real `rustdoc`.
//!
//! ## Why this exists
//!
//! `rustdoc` does **not** honour `RUSTC_WRAPPER`. It re-parses crate source
//! and the doc-test extractor calls into rustc directly, so any doc-test
//! that uses Trust syntax extensions (named args, pipe) fails
//! with a plain rustc parse error during `cargo test --doc`.
//!
//! Cargo, however, lets users override the rustdoc binary via the `RUSTDOC`
//! env var (full replacement) and on newer cargo versions also
//! `RUSTDOC_WRAPPER` (wrapper-style, like `RUSTC_WRAPPER`). This binary
//! supports both calling conventions:
//!
//! - **Replacement (`RUSTDOC=<this>`):** argv is just rustdoc's args. We
//!   need to find the real rustdoc ourselves — first try the sibling
//!   `rustdoc` in `rustc --print sysroot`/bin, then fall back to `rustdoc`
//!   on PATH (but never re-invoke ourselves).
//! - **Wrapper (`RUSTDOC_WRAPPER=<this>`):** cargo passes the real rustdoc
//!   path as argv\[0\], same shape as RUSTC_WRAPPER. Detect this by checking
//!   whether argv\[0\] resolves to an executable file whose basename is
//!   `rustdoc` (or `rustdoc.exe`).
//!
//! The lowering itself is the shared `trust_rustc::prepare_strict_input`
//! used by the rustc wrapper — same FNV cache, same module-tree mirror.

use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use trust_rustc::{find_input_rs, prepare_strict_input};

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(e) => {
            eprintln!("trust-rustdoc: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32> {
    let argv: Vec<String> = env::args().skip(1).collect();

    // Detect wrapper vs replacement mode (see module docs).
    let (rustdoc, doc_args): (PathBuf, Vec<String>) = match argv.first() {
        Some(first) if looks_like_rustdoc_path(first) => (PathBuf::from(first), argv[1..].to_vec()),
        _ => (find_real_rustdoc()?, argv),
    };

    let Some(idx) = find_input_rs(&doc_args) else {
        return run_rustdoc(&rustdoc, &doc_args);
    };
    // `idx` is a valid index by construction, but go through `get`/`get_mut`
    // so the strict no-bare-index rule (R0014) is satisfied without an allow.
    let Some(input_arg) = doc_args.get(idx) else {
        return run_rustdoc(&rustdoc, &doc_args);
    };

    let input_path = PathBuf::from(input_arg);
    let Some(prepared) = prepare_strict_input(&input_path)
        .with_context(|| format!("preparing {}", input_path.display()))?
    else {
        return run_rustdoc(&rustdoc, &doc_args);
    };

    let mut new_args = doc_args.clone();
    if let Some(slot) = new_args.get_mut(idx) {
        *slot = prepared.lowered_root.to_string_lossy().into_owned();
    }
    // NOTE: rustdoc on stable rejects `--remap-path-prefix` (it's gated
    // behind `-Z unstable-options`). Doc-test failures will therefore
    // point at the lowered cache path rather than the user's source —
    // acceptable trade-off until the flag is stabilised for rustdoc.
    let _ = prepared.remap_flag;
    run_rustdoc(&rustdoc, &new_args)
}

/// True if `s` looks like a path to a `rustdoc` executable (wrapper mode).
fn looks_like_rustdoc_path(s: &str) -> bool {
    let p = Path::new(s);
    let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    if stem != "rustdoc" {
        return false;
    }
    // Must look like a real path (has parent components OR is an existing
    // file). A plain `--something` arg or input.rs wouldn't reach here
    // because the stem check above already failed.
    p.is_file() || p.components().count() > 1
}

/// Find the real rustdoc binary. Prefer the one next to the active rustc
/// (matches the user's toolchain), fall back to `rustdoc` on PATH.
fn find_real_rustdoc() -> Result<PathBuf> {
    // Try `rustc --print sysroot`/bin/rustdoc.
    if let Ok(out) = Command::new("rustc").arg("--print").arg("sysroot").output() {
        if out.status.success() {
            let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let candidate = PathBuf::from(sysroot).join("bin").join(rustdoc_filename());
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    // Fall back to PATH lookup. Skip our own binary (avoid infinite recursion).
    let me = env::current_exe().ok();
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join(rustdoc_filename());
            if candidate.is_file() && Some(&candidate) != me.as_ref() {
                return Ok(candidate);
            }
        }
    }
    bail!("could not find a real rustdoc binary (tried `rustc --print sysroot` and PATH)");
}

fn rustdoc_filename() -> &'static str {
    if cfg!(windows) {
        "rustdoc.exe"
    } else {
        "rustdoc"
    }
}

fn run_rustdoc(path: &Path, args: &[String]) -> Result<i32> {
    let status = Command::new(path)
        .args(args)
        .status()
        .with_context(|| format!("invoking {}", path.display()))?;
    Ok(status.code().unwrap_or(1))
}
