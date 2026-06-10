//! `cargo-trust` — the cargo bridge for the Trust toolchain.
//!
//! Two responsibilities, dispatched on the first argument:
//!
//! 1. **Cargo lifecycle commands** (`build`, `run`, `test`, `check`,
//!    `clippy`, `doc`, `bench`, `install`): set `RUSTC_WRAPPER` and
//!    `RUSTDOC` to the bundled `trust-rustc` / `trust-rustdoc` shims, then
//!    exec the real `cargo` with the same arguments. This is what lets a
//!    cargo crate use the syntax extensions (named args, pipe) with a single
//!    command and **zero environment setup** —
//!
//!    ```sh
//!    cargo trust build      # == RUSTC_WRAPPER=… RUSTDOC=… cargo build
//!    cargo trust run
//!    cargo trust test
//!    ```
//!
//!    replaces the old multi-step `export RUSTC_WRAPPER=$(realpath …)` dance.
//!
//! 2. **Trust-native helpers** (`lower`, `index`, `fix`, `explain`, and any
//!    other non-cargo subcommand): forwarded verbatim to the `trust` CLI,
//!    preserving `cargo trust lower foo.rs`, `cargo trust explain R0042`, etc.
//!
//! ## Disambiguation: `check`
//!
//! `check` exists in both worlds. Under `cargo trust` it means **`cargo
//! check`** (whole-crate, project mode) — that is the useful thing in a cargo
//! workspace. For a single-file lint, call the `trust` CLI directly: `trust
//! check foo.rs`.
//!
//! ## How the shims are located
//!
//! In priority order:
//!   1. `TRUST_RUSTC` / `TRUST_RUSTDOC` env overrides, if set.
//!   2. A sibling of *this* binary (`cargo-trust`). Covers both a
//!      `cargo install`ed layout (all three land in `~/.cargo/bin`) and a dev
//!      checkout (`target/debug/`).
//!   3. A `PATH` lookup.
//!
//! If a shim can't be found we fail loudly with a fixable message rather than
//! silently running cargo without lowering (which would turn every named-arg
//! call site into a confusing `rustc` parse error).

use anyhow::{bail, Context, Result};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Cargo subcommands that compile code and therefore need the lowering shims
/// wired in. Anything not in this list is treated as a trust-native helper and
/// forwarded to the `trust` CLI.
const CARGO_COMMANDS: &[&str] = &[
    "build", "run", "test", "check", "clippy", "doc", "bench", "install",
];

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1)),
        Err(e) => {
            eprintln!("cargo-trust: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32> {
    // When invoked as `cargo trust <args>`, cargo passes argv as
    // `cargo-trust trust <args>`. Strip our own name (`args().skip(1)`) and the
    // literal `trust` cargo prepends.
    let rest = strip_trust_prefix(env::args().skip(1).collect());

    match dispatch(&rest) {
        Dispatch::Usage => {
            print_usage();
            Ok(0)
        }
        Dispatch::Cargo => run_cargo(&rest),
        Dispatch::Trust => forward_to_trust(&rest),
    }
}

/// Drop the literal `trust` token cargo prepends when invoked as a subcommand.
/// Idempotent for direct `cargo-trust <args>` invocations (no leading `trust`).
fn strip_trust_prefix(argv: Vec<String>) -> Vec<String> {
    match argv.split_first() {
        Some((first, rest)) if first == "trust" => rest.to_vec(),
        _ => argv,
    }
}

/// Which path the (prefix-stripped) args take.
#[derive(Debug, PartialEq, Eq)]
enum Dispatch {
    Usage,
    Cargo,
    Trust,
}

fn dispatch(args: &[String]) -> Dispatch {
    match args.first().map(String::as_str) {
        None => Dispatch::Usage,
        Some(cmd) if CARGO_COMMANDS.contains(&cmd) => Dispatch::Cargo,
        Some(_) => Dispatch::Trust,
    }
}

/// Set the lowering shims as `RUSTC_WRAPPER` / `RUSTDOC` and exec `cargo` with
/// the given args (the cargo subcommand is `args[0]`).
fn run_cargo(args: &[String]) -> Result<i32> {
    let rustc = locate_shim("trust-rustc", "TRUST_RUSTC")?;
    let rustdoc = locate_shim("trust-rustdoc", "TRUST_RUSTDOC")?;
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));

    let status = Command::new(&cargo)
        .args(args)
        // Respect a wrapper the user already set, but default to ours.
        .env("RUSTC_WRAPPER", &rustc)
        .env("RUSTDOC", &rustdoc)
        .status()
        .with_context(|| format!("invoking {}", cargo.to_string_lossy()))?;
    Ok(status.code().unwrap_or(1))
}

/// Forward a trust-native subcommand to the `trust` CLI.
fn forward_to_trust(args: &[String]) -> Result<i32> {
    let trust = locate_on_path("trust").unwrap_or_else(|| PathBuf::from("trust"));
    let status = Command::new(&trust)
        .args(args)
        .status()
        .context("invoking `trust` (is it on PATH?)")?;
    Ok(status.code().unwrap_or(1))
}

/// Resolve a bundled shim binary by name, honouring an env override.
fn locate_shim(bin: &str, env_override: &str) -> Result<PathBuf> {
    if let Some(p) = env::var_os(env_override) {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
        bail!(
            "{env_override} is set to `{}`, but no file exists there",
            path.display()
        );
    }
    if let Some(p) = sibling_of_self(bin) {
        return Ok(p);
    }
    if let Some(p) = locate_on_path(bin) {
        return Ok(p);
    }
    bail!(
        "could not find the `{bin}` shim.\n\
         Looked next to `cargo-trust` and on PATH. Fix by either:\n  \
         • installing it alongside cargo-trust (`cargo install --path crates/{bin}`), or\n  \
         • pointing {env_override} at the binary (e.g. \
         `{env_override}=$(realpath target/debug/{bin})`)."
    )
}

/// Look for `bin` (with the platform executable extension) in the same
/// directory as the currently-running `cargo-trust` binary.
fn sibling_of_self(bin: &str) -> Option<PathBuf> {
    let me = env::current_exe().ok()?;
    let dir = me.parent()?;
    let candidate = dir.join(exe_name(bin));
    candidate.is_file().then_some(candidate)
}

/// Scan `PATH` for an executable named `bin`.
fn locate_on_path(bin: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let want = exe_name(bin);
    env::split_paths(&path)
        .map(|dir| dir.join(&want))
        .find(|p| is_executable_file(p))
}

fn exe_name(bin: &str) -> String {
    if env::consts::EXE_SUFFIX.is_empty() {
        bin.to_string()
    } else {
        format!("{bin}{}", env::consts::EXE_SUFFIX)
    }
}

fn is_executable_file(p: &Path) -> bool {
    // `is_file()` is sufficient for our purposes across platforms; PATH dirs
    // hold executables, and the shims are always plain files.
    p.is_file()
}

fn print_usage() {
    eprintln!(
        "cargo-trust — the cargo bridge for the Trust toolchain\n\
         \n\
         USAGE:\n  \
         cargo trust <command> [args...]\n\
         \n\
         CARGO COMMANDS (run with Trust lowering wired in automatically):\n  \
         build, run, test, check, clippy, doc, bench, install\n  \
         e.g.  cargo trust build      # == RUSTC_WRAPPER/RUSTDOC set, then cargo build\n\
         \n\
         TRUST HELPERS (forwarded to the `trust` CLI):\n  \
         lower, index, fix, explain, …\n  \
         e.g.  cargo trust explain R0042\n\
         \n\
         For a single-file lint, use the `trust` CLI directly: trust check foo.rs"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn strips_cargo_prepended_trust_token() {
        assert_eq!(strip_trust_prefix(v(&["trust", "build"])), v(&["build"]));
        assert_eq!(
            strip_trust_prefix(v(&["trust", "explain", "R0042"])),
            v(&["explain", "R0042"])
        );
    }

    #[test]
    fn strip_is_idempotent_for_direct_invocation() {
        // `cargo-trust build` (no cargo prefix) must not lose `build`.
        assert_eq!(strip_trust_prefix(v(&["build"])), v(&["build"]));
        assert_eq!(strip_trust_prefix(v(&[])), v(&[]));
    }

    #[test]
    fn only_strips_a_leading_trust_not_a_later_one() {
        // `cargo trust run trust` → after prefix strip, `run trust` is intact.
        assert_eq!(
            strip_trust_prefix(v(&["trust", "run", "trust"])),
            v(&["run", "trust"])
        );
    }

    #[test]
    fn cargo_lifecycle_commands_route_to_cargo() {
        for cmd in CARGO_COMMANDS {
            assert_eq!(dispatch(&v(&[cmd])), Dispatch::Cargo, "{cmd}");
        }
    }

    #[test]
    fn trust_helpers_route_to_trust() {
        for cmd in ["lower", "index", "fix", "explain", "wat"] {
            assert_eq!(dispatch(&v(&[cmd])), Dispatch::Trust, "{cmd}");
        }
    }

    #[test]
    fn no_args_shows_usage() {
        assert_eq!(dispatch(&v(&[])), Dispatch::Usage);
    }
}
