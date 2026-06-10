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

    let mut cmd = Command::new(&cargo);
    cmd.args(args)
        // Respect a wrapper the user already set, but default to ours.
        .env("RUSTC_WRAPPER", &rustc)
        .env("RUSTDOC", &rustdoc);

    // Project-level strict opt-in: if the manifest declares
    // `[package.metadata.trust] strict = true`, tell the shims to lower the
    // whole crate even when individual files carry no `#![strict]` marker.
    // Scoped by package name so dependencies (compiled by the same wrapper)
    // are never force-lowered — see `crate_is_force_strict` in trust-rustc.
    let strict = strict_packages(args);
    if !strict.is_empty() {
        cmd.env("TRUST_STRICT_PACKAGES", strict.join(","));
    }

    let status = cmd
        .status()
        .with_context(|| format!("invoking {}", cargo.to_string_lossy()))?;
    Ok(status.code().unwrap_or(1))
}

/// Names of packages that opted into strict mode at the project level via
/// `[package.metadata.trust] strict = true`. Reads the manifest cargo would
/// use: `--manifest-path` if given, else the nearest `Cargo.toml` walking up
/// from the current directory. For a workspace manifest, every member's
/// manifest is read too (glob entries like `crates/*` are expanded), so the
/// opt-in works from the workspace root — and
/// `[workspace.metadata.trust] strict = true` opts in every member at once.
/// Best-effort — a missing/unparseable manifest yields an empty set
/// (per-file `#![strict]` markers still work).
fn strict_packages(args: &[String]) -> Vec<String> {
    let Some(manifest) = manifest_path(args) else {
        return Vec::new();
    };
    let Some(value) = read_manifest(&manifest) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    out.extend(parse_strict_package(&value));

    if let Some(workspace) = value.get("workspace") {
        let all_members_strict = metadata_trust_strict(workspace);
        let root = manifest.parent().unwrap_or(Path::new("."));
        for member_dir in workspace_member_dirs(workspace, root) {
            let Some(member) = read_manifest(&member_dir.join("Cargo.toml")) else {
                continue;
            };
            if all_members_strict {
                // Workspace-wide opt-in: every member with a [package] name.
                if let Some(name) = member
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                {
                    out.push(name.to_string());
                }
            } else {
                out.extend(parse_strict_package(&member));
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

fn read_manifest(path: &Path) -> Option<toml::Value> {
    std::fs::read_to_string(path)
        .ok()?
        .parse::<toml::Value>()
        .ok()
}

/// Extract the package name from a parsed manifest iff it declares
/// `[package.metadata.trust] strict = true`.
fn parse_strict_package(manifest: &toml::Value) -> Option<String> {
    let package = manifest.get("package")?;
    if !metadata_trust_strict(package) {
        return None;
    }
    package.get("name")?.as_str().map(str::to_string)
}

/// `<table>.metadata.trust.strict == true` — shared between the package and
/// workspace forms of the opt-in.
fn metadata_trust_strict(table: &toml::Value) -> bool {
    table
        .get("metadata")
        .and_then(|m| m.get("trust"))
        .and_then(|t| t.get("strict"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

/// Resolve a workspace's `members` list to directories, expanding a trailing
/// `/*` glob (the only glob form cargo commonly uses) via read_dir. Members
/// listed in `exclude` are skipped.
fn workspace_member_dirs(workspace: &toml::Value, root: &Path) -> Vec<PathBuf> {
    let list = |key: &str| -> Vec<String> {
        workspace
            .get(key)
            .and_then(|m| m.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let excluded: Vec<PathBuf> = list("exclude").iter().map(|e| root.join(e)).collect();

    let mut dirs = Vec::new();
    for entry in list("members") {
        if let Some(prefix) = entry.strip_suffix("/*") {
            let Ok(read) = std::fs::read_dir(root.join(prefix)) else {
                continue;
            };
            for e in read.flatten() {
                let p = e.path();
                if p.is_dir() && p.join("Cargo.toml").is_file() {
                    dirs.push(p);
                }
            }
        } else {
            dirs.push(root.join(entry));
        }
    }
    dirs.retain(|d| !excluded.contains(d));
    dirs
}

/// Resolve the manifest path: `--manifest-path <p>` / `--manifest-path=<p>` if
/// present in args, otherwise the nearest `Cargo.toml` walking up from cwd.
fn manifest_path(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--manifest-path" {
            return it.next().map(PathBuf::from);
        }
        if let Some(p) = a.strip_prefix("--manifest-path=") {
            return Some(PathBuf::from(p));
        }
    }
    let mut dir = env::current_dir().ok()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
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

    fn parse(s: &str) -> Option<String> {
        parse_strict_package(&s.parse::<toml::Value>().unwrap())
    }

    #[test]
    fn strict_true_yields_package_name() {
        let m = r#"
            [package]
            name = "my-app"
            version = "0.1.0"
            [package.metadata.trust]
            strict = true
        "#;
        assert_eq!(parse(m), Some("my-app".to_string()));
    }

    #[test]
    fn strict_false_or_absent_yields_nothing() {
        let false_ = r#"
            [package]
            name = "my-app"
            [package.metadata.trust]
            strict = false
        "#;
        assert_eq!(parse(false_), None);
        let absent = r#"
            [package]
            name = "my-app"
        "#;
        assert_eq!(parse(absent), None);
        // A virtual workspace manifest has no [package] at all.
        let virtual_ws = r#"
            [workspace]
            members = ["a", "b"]
        "#;
        assert_eq!(parse(virtual_ws), None);
    }

    #[test]
    fn manifest_path_reads_explicit_flag() {
        assert_eq!(
            manifest_path(&v(&["build", "--manifest-path", "/x/Cargo.toml"])),
            Some(PathBuf::from("/x/Cargo.toml"))
        );
        assert_eq!(
            manifest_path(&v(&["build", "--manifest-path=/y/Cargo.toml"])),
            Some(PathBuf::from("/y/Cargo.toml"))
        );
    }

    /// Build a throwaway workspace on disk: root manifest + one member per
    /// (name, strict) pair under `crates/`, listed via the `crates/*` glob.
    fn temp_workspace(tag: &str, root_extra: &str, members: &[(&str, bool)]) -> PathBuf {
        let root = env::temp_dir().join(format!("cargo-trust-ws-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (name, strict) in members {
            let dir = root.join("crates").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let meta = if *strict {
                "\n[package.metadata.trust]\nstrict = true\n"
            } else {
                ""
            };
            std::fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n{meta}"),
            )
            .unwrap();
        }
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[workspace]\nmembers = [\"crates/*\"]\n{root_extra}"),
        )
        .unwrap();
        root
    }

    #[test]
    fn workspace_glob_members_with_package_optins() {
        let root = temp_workspace(
            "pkg",
            "",
            &[("alpha", true), ("beta", false), ("gamma", true)],
        );
        let strict = strict_packages(&v(&[
            "build",
            "--manifest-path",
            root.join("Cargo.toml").to_str().unwrap(),
        ]));
        assert_eq!(strict, vec!["alpha".to_string(), "gamma".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_metadata_opts_in_every_member() {
        let root = temp_workspace(
            "ws",
            "[workspace.metadata.trust]\nstrict = true\n",
            &[("alpha", false), ("beta", false)],
        );
        let strict = strict_packages(&v(&[
            "build",
            "--manifest-path",
            root.join("Cargo.toml").to_str().unwrap(),
        ]));
        assert_eq!(strict, vec!["alpha".to_string(), "beta".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
