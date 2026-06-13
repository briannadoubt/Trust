//! `cargo-trustc` — the cargo bridge for the Trust toolchain.
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
//!    cargo trustc build      # == RUSTC_WRAPPER=… RUSTDOC=… cargo build
//!    cargo trustc run
//!    cargo trustc test
//!    ```
//!
//!    replaces the old multi-step `export RUSTC_WRAPPER=$(realpath …)` dance.
//!
//! 2. **Trust-native helpers** (`lower`, `index`, `fix`, `explain`, and any
//!    other non-cargo subcommand): forwarded verbatim to the `trust` CLI,
//!    preserving `cargo trustc lower foo.rs`, `cargo trustc explain R0042`, etc.
//!
//! ## Disambiguation: `check`
//!
//! `check` exists in both worlds. Under `cargo trustc` it means **`cargo
//! check`** (whole-crate, project mode) — that is the useful thing in a cargo
//! workspace. For a single-file lint, call the `trust` CLI directly: `trust
//! check foo.rs`.
//!
//! ## How the shims are located
//!
//! In priority order:
//!   1. `TRUST_RUSTC` / `TRUST_RUSTDOC` env overrides, if set.
//!   2. A sibling of *this* binary (`cargo-trustc`). Covers both a
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
            eprintln!("cargo-trustc: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<i32> {
    // When invoked as `cargo trustc <args>`, cargo passes argv as
    // `cargo-trustc trust <args>`. Strip our own name (`args().skip(1)`) and the
    // literal `trust` cargo prepends.
    let rest = strip_trust_prefix(env::args().skip(1).collect());

    // RT-111/112: helper subcommands intercepted before dispatch.
    match rest.first().map(String::as_str) {
        Some("adopt") => return adopt(&rest[1..]),
        Some("doctor") => return doctor(),
        _ => {}
    }

    match dispatch(&rest) {
        Dispatch::Usage => {
            print_usage();
            Ok(0)
        }
        Dispatch::Cargo => run_cargo(&rest),
        Dispatch::Trust => forward_to_trust(&rest),
    }
}

/// Outcome of ensuring the strict opt-in is present in a manifest. The `String`
/// is the table the opt-in lives under (`workspace` or `package`).
enum MetadataOutcome {
    Added(String),
    AlreadyStrict,
    TableExistsNotStrict(String),
}

/// `cargo trustc adopt` (RT-111): turn an existing crate into a Trust dialect
/// crate in one command — opt into strict mode, migrate every positional call
/// to named-arg form (RT-110), then build through the gate and surface the
/// lints the mechanical migration can't fix for a human to finish.
fn adopt(args: &[String]) -> Result<i32> {
    let manifest = manifest_path(args)
        .context("no Cargo.toml found — run `cargo trustc adopt` inside a cargo package")?;
    let project_dir = manifest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();

    // 1. Opt in at the project level (invisible to stock cargo). A workspace
    // root opts in every member via [workspace.metadata.trust]; a plain package
    // uses [package.metadata.trust].
    match ensure_strict_metadata(&manifest)? {
        MetadataOutcome::Added(table) => {
            eprintln!(
                "✓ added [{table}.metadata.trust] strict = true to {}",
                manifest.display()
            );
        }
        MetadataOutcome::AlreadyStrict => {
            eprintln!("✓ already opted in ({})", manifest.display());
        }
        MetadataOutcome::TableExistsNotStrict(table) => {
            bail!(
                "{} already has a [{table}.metadata.trust] table but `strict` isn't `true` — \
                 set `strict = true` there, then re-run `cargo trustc adopt`",
                manifest.display()
            );
        }
    }

    // 2. Migrate positional calls → named-arg form across the tree (RT-110).
    let trust = locate_trust().context(
        "could not find the `trust` CLI, needed for the migration step.\n  \
         Install it with `cargo install trust-lang`, or place it on PATH \
         (it ships alongside cargo-trustc).",
    )?;
    eprintln!("→ migrating calls to named-argument form (`trust fix --write`)…");
    let status = Command::new(&trust)
        .arg("fix")
        .arg("--write")
        .arg(&project_dir)
        .status()
        .context("running `trust fix --write` for the migration")?;
    if !status.success() {
        bail!("the migration step failed; aborting before the build");
    }

    // 3. Build through the dialect so the remaining (non-mechanical) lints show.
    eprintln!("→ building through the dialect (`cargo trustc build`) to surface what's left…");
    let code = run_cargo(&["build".to_string()])?;
    if code == 0 {
        eprintln!(
            "\n✓ adopted — this crate builds clean under the Trust dialect. \
             Use `cargo trustc build|run|test` from here on."
        );
    } else {
        eprintln!(
            "\nMigration applied. The build above lists the lints the mechanical step \
             can't fix (e.g. R0017 newtypes, R0001 `.unwrap()`) — address those, then \
             `cargo trustc build` again. `trust explain <CODE>` details any rule."
        );
    }
    Ok(code)
}

/// Ensure the strict opt-in is present in `manifest`, appending it when absent.
/// A workspace root (has a `[workspace]` table) opts in every member via
/// `[workspace.metadata.trust]`; a plain package uses `[package.metadata.trust]`
/// (RT-118). Preserves existing formatting (text append, not a TOML
/// re-serialise). Refuses to touch a manifest that already has the table with a
/// non-true value — that's the user's to resolve.
fn ensure_strict_metadata(manifest: &Path) -> Result<MetadataOutcome> {
    let text = std::fs::read_to_string(manifest)
        .with_context(|| format!("reading {}", manifest.display()))?;

    let parsed = toml::from_str::<toml::Value>(&text).ok();
    // A workspace root opts in all members; otherwise it's a package.
    let table = if parsed
        .as_ref()
        .is_some_and(|v| v.get("workspace").is_some())
    {
        "workspace"
    } else {
        "package"
    };

    if let Some(value) = &parsed {
        if value.get(table).is_some_and(metadata_trust_strict) {
            return Ok(MetadataOutcome::AlreadyStrict);
        }
    }
    if text.contains(&format!("[{table}.metadata.trust]")) {
        return Ok(MetadataOutcome::TableExistsNotStrict(table.to_string()));
    }

    let mut new = text;
    if !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(&format!(
        "\n# Strict mode is enforced by `cargo trustc` (build/run/test); stock cargo\n\
         # ignores this metadata table entirely.\n\
         [{table}.metadata.trust]\n\
         strict = true\n",
    ));
    std::fs::write(manifest, new).with_context(|| format!("writing {}", manifest.display()))?;
    Ok(MetadataOutcome::Added(table.to_string()))
}

/// Locate the `trust` CLI: next to this binary first, then on PATH.
fn locate_trust() -> Option<PathBuf> {
    sibling_of_self("trust").or_else(|| locate_on_path("trust"))
}

/// Drop the literal `trustc` token cargo prepends when invoked as a subcommand.
/// Idempotent for direct `cargo-trustc <args>` invocations (no leading `trust`).
fn strip_trust_prefix(argv: Vec<String>) -> Vec<String> {
    match argv.split_first() {
        Some((first, rest)) if first == "trustc" => rest.to_vec(),
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

    // RT-96: `--message-format <fmt>` is OURS, not cargo's — cargo's own
    // flag takes different values (human, json-render-diagnostics, …), so
    // it must never see ours. Strip it from the forwarded args and turn it
    // into the TRUST_MESSAGE_FORMAT env var the shims read.
    let (args, message_format) = extract_message_format(args)?;

    let mut cmd = Command::new(&cargo);
    cmd.args(&args)
        // Respect a wrapper the user already set, but default to ours.
        .env("RUSTC_WRAPPER", &rustc)
        .env("RUSTDOC", &rustdoc);

    // The spawned cargo inherits our environment, so a user-set
    // `TRUST_MESSAGE_FORMAT=json` (no flag) keeps working with no extra code
    // here — the flag form below just sets the same var explicitly.
    if message_format.is_some() {
        cmd.env("TRUST_MESSAGE_FORMAT", "json");
    }

    // Project-level strict opt-in: if the manifest declares
    // `[package.metadata.trust] strict = true`, tell the shims to lower the
    // whole crate even when individual files carry no `#![strict]` marker.
    // Scoped by package name so dependencies (compiled by the same wrapper)
    // are never force-lowered — see `crate_is_force_strict` in trust-rustc.
    let strict = strict_packages(&args);
    if !strict.is_empty() {
        cmd.env("TRUST_STRICT_PACKAGES", strict.join(","));
    }

    // RT-114: auto-build a workspace signature index so cross-crate named-arg
    // (R0042) resolution works with no manual `trust index` / TRUST_SIGNATURE_PATH.
    // RT-121: the index is cached (keyed on the src trees' fingerprint), so it
    // persists across invocations — do not delete it after the build.
    if let Some(path) = auto_signature_index(&args) {
        cmd.env("TRUST_SIGNATURE_PATH", path);
    }

    let status = cmd
        .status()
        .with_context(|| format!("invoking {}", cargo.to_string_lossy()))?;
    Ok(status.code().unwrap_or(1))
}

/// Build a signature index of the whole workspace's `src/` trees and write it
/// to a temp manifest, returning its path — so the build-gate wrapper resolves
/// cross-crate named-arg calls (R0042) with no manual `trust index` /
/// `TRUST_SIGNATURE_PATH` (RT-114). Returns `None` when the user already set
/// `TRUST_SIGNATURE_PATH`, when there's no manifest, or when the index is empty.
fn auto_signature_index(args: &[String]) -> Option<PathBuf> {
    if env::var_os("TRUST_SIGNATURE_PATH").is_some() {
        return None; // respect an explicit index
    }
    let manifest = manifest_path(args)?;
    let src_dirs = workspace_src_dirs(&manifest);
    if src_dirs.is_empty() {
        return None;
    }

    // RT-121: reuse a cached index when the src trees are unchanged. The
    // fingerprint walk (readdir + stat) is far cheaper than re-parsing every
    // file with syn, so incremental rebuilds skip the parse entirely.
    let fp = src_fingerprint(&src_dirs);
    let cache_dir = env::temp_dir().join("trust-sigindex");
    let cache_path = cache_dir.join(format!("{fp:016x}.txt"));
    if cache_path.is_file() {
        return Some(cache_path);
    }

    let indices: Vec<_> = src_dirs
        .iter()
        .map(|d| trust_lower::sig_index::extract_from_dir(d))
        .collect();
    let merged = trust_lower::sig_index::merge(&indices);
    if merged.is_empty() {
        return None;
    }
    let text = trust_lower::sig_index::render_manifest(
        &merged,
        "# @generated by `cargo trustc` (RT-114/121): workspace public-fn signature index.",
    );
    let _ = std::fs::create_dir_all(&cache_dir);
    std::fs::write(&cache_path, text).ok()?;
    Some(cache_path)
}

/// FNV-1a fingerprint of every `.rs` file under `src_dirs` (path + mtime + len),
/// plus this crate's version, used as the auto-index cache key (RT-121). Any
/// file added, removed, or modified changes the fingerprint; a version bump
/// busts stale caches if the index format ever changes.
fn src_fingerprint(src_dirs: &[PathBuf]) -> u64 {
    let mut entries: Vec<(PathBuf, u64, u64)> = Vec::new();
    for dir in src_dirs {
        collect_rs_meta(dir, &mut entries);
    }
    entries.sort();

    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };
    mix(env!("CARGO_PKG_VERSION").as_bytes());
    for (path, mtime, len) in &entries {
        mix(path.to_string_lossy().as_bytes());
        mix(&mtime.to_le_bytes());
        mix(&len.to_le_bytes());
    }
    hash
}

/// Recursively collect `(path, mtime_nanos, len)` for every `.rs` file under
/// `dir`. Cheap (stat only, no parse) — the basis of the cache fingerprint.
fn collect_rs_meta(dir: &Path, out: &mut Vec<(PathBuf, u64, u64)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_meta(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(meta) = entry.metadata() {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| u64::try_from(d.as_nanos()).ok())
                    .unwrap_or(0);
                out.push((path, mtime, meta.len()));
            }
        }
    }
}

/// The existing `src/` directories of the package and its workspace members,
/// for the auto signature index. Reuses the same workspace discovery as the
/// strict opt-in. Indexing `src/` (not the crate root) keeps `target/` out.
fn workspace_src_dirs(manifest: &Path) -> Vec<PathBuf> {
    let Some(value) = read_manifest(manifest) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    if value.get("package").is_some() {
        if let Some(p) = manifest.parent() {
            dirs.push(p.join("src"));
        }
    }
    // Resolve the workspace table + its root directory (whether this manifest
    // is the root or a member).
    let workspace_info: Option<(toml::Value, PathBuf)> = if value.get("workspace").is_some() {
        manifest.parent().map(|p| (value.clone(), p.to_path_buf()))
    } else {
        find_workspace_root(manifest).and_then(|(root_manifest, root_value)| {
            root_manifest
                .parent()
                .map(|p| (root_value, p.to_path_buf()))
        })
    };
    if let Some((root_value, root_dir)) = workspace_info {
        if let Some(workspace) = root_value.get("workspace") {
            for member in workspace_member_dirs(workspace, &root_dir) {
                dirs.push(member.join("src"));
            }
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs
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

    // PR #1 review: when invoked from inside a workspace MEMBER, the nearest
    // manifest has no [workspace] table, so a root-level
    // [workspace.metadata.trust] opt-in was silently ignored unless the
    // command ran from the root. Mirror cargo's root discovery: a manifest
    // with a [workspace] table is its own root (including the empty
    // `[workspace]` opt-out our fixtures use); otherwise walk ancestors for
    // the nearest manifest that has one and process that root too.
    let value = if value.get("workspace").is_some() {
        value
    } else if let Some((_root_path, root_value)) = find_workspace_root(&manifest) {
        out.extend(parse_strict_package(&root_value));
        // Re-anchor member resolution at the discovered root below.
        return collect_workspace_strict(out, &root_value, _root_path.parent());
    } else {
        value
    };

    collect_workspace_strict(out, &value, manifest.parent())
}

/// Fold the workspace-level opt-ins of `value` (a manifest that may carry a
/// `[workspace]` table rooted at `root_dir`) into `out`, then sort/dedup.
fn collect_workspace_strict(
    mut out: Vec<String>,
    value: &toml::Value,
    root_dir: Option<&Path>,
) -> Vec<String> {
    if let Some(workspace) = value.get("workspace") {
        let all_members_strict = metadata_trust_strict(workspace);
        let root = root_dir.unwrap_or(Path::new("."));
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

/// Walk ancestor directories of `member_manifest` for the nearest
/// `Cargo.toml` that carries a `[workspace]` table — cargo's workspace-root
/// rule, minus the rare `package.workspace = "…"` explicit override.
fn find_workspace_root(member_manifest: &Path) -> Option<(PathBuf, toml::Value)> {
    let mut dir = member_manifest.parent()?.parent();
    while let Some(d) = dir {
        let candidate = d.join("Cargo.toml");
        if let Some(value) = read_manifest(&candidate) {
            if value.get("workspace").is_some() {
                return Some((candidate, value));
            }
        }
        dir = d.parent();
    }
    None
}

fn read_manifest(path: &Path) -> Option<toml::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str::<toml::Value>(&text).ok()
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

/// Pull our `--message-format <fmt>` / `--message-format=<fmt>` option out of
/// a cargo-lifecycle arg list (RT-96). Returns the args with the flag (and its
/// value) removed, plus the requested format if present. The flag may appear
/// anywhere in the invocation. `json` is the only supported value; anything
/// else (or a trailing flag with no value) is an error naming the supported
/// set, so a typo never silently reaches cargo's same-named flag.
fn extract_message_format(args: &[String]) -> Result<(Vec<String>, Option<String>)> {
    let mut out = Vec::with_capacity(args.len());
    let mut format = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = if a == "--message-format" {
            let Some(value) = it.next() else {
                bail!("--message-format requires a value; supported values: json");
            };
            value.as_str()
        } else if let Some(value) = a.strip_prefix("--message-format=") {
            value
        } else {
            out.push(a.clone());
            continue;
        };
        if value != "json" {
            bail!("unsupported --message-format `{value}`; supported values: json");
        }
        format = Some(value.to_string());
    }
    Ok((out, format))
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
    let trust = locate_trust().context(
        "could not find the `trust` CLI. Install it with `cargo install trust-lang`, \
         or place it on PATH (it ships alongside cargo-trustc). \
         Run `cargo trustc doctor` to check your setup.",
    )?;
    let status = Command::new(&trust)
        .args(args)
        .status()
        .context("running the `trust` CLI")?;
    Ok(status.code().unwrap_or(1))
}

/// `cargo trustc doctor` (RT-112): diagnose the toolchain setup and print
/// actionable fixes, so a first-time user isn't left with a cryptic stock-rustc
/// error when a piece is missing or the project isn't opted in.
fn doctor() -> Result<i32> {
    eprintln!("cargo trustc doctor — checking your Trust setup\n");

    let mut missing = false;
    // (binary, env override). `trust` is the CLI; the other two are the shims
    // cargo trustc wires in as RUSTC_WRAPPER / RUSTDOC.
    let checks: [(&str, Option<&str>); 3] = [
        ("trust", None),
        ("trust-rustc", Some("TRUST_RUSTC")),
        ("trust-rustdoc", Some("TRUST_RUSTDOC")),
    ];
    for (bin, env_override) in checks {
        match probe_bin(bin, env_override) {
            Some(p) => eprintln!("  ✓ {bin:<14} {}", p.display()),
            None => {
                missing = true;
                eprintln!("  ✗ {bin:<14} not found");
            }
        }
    }

    eprintln!();
    match manifest_path(&[]) {
        Some(m) if !strict_packages(&[]).is_empty() => {
            eprintln!("  ✓ {} opts into strict mode", m.display());
        }
        Some(m) => {
            eprintln!(
                "  • {} isn't strict yet — run `cargo trustc adopt`, or add\n    \
                 [package.metadata.trust] strict = true",
                m.display()
            );
        }
        None => {
            eprintln!(
                "  • no Cargo.toml here — run inside a cargo package, or use the \
                 `trust` CLI on single files (`trust check foo.rs`)"
            );
        }
    }

    if missing {
        eprintln!(
            "\nInstall the missing pieces:\n  \
             cargo install trust-lang cargo-trustc trust-rustc trust-rustdoc"
        );
        Ok(1)
    } else {
        eprintln!(
            "\nReady: build with `cargo trustc build` \
             (or `cargo trustc adopt` to convert this crate into the dialect)."
        );
        Ok(0)
    }
}

/// Probe for a binary the way the wrapper resolution does, but leniently
/// (returns `None` instead of failing): an existing env-override path first,
/// then a sibling of this binary, then `PATH`.
fn probe_bin(bin: &str, env_override: Option<&str>) -> Option<PathBuf> {
    if let Some(ev) = env_override {
        if let Some(p) = env::var_os(ev) {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    sibling_of_self(bin).or_else(|| locate_on_path(bin))
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
         Looked next to `cargo-trustc` and on PATH. Fix by either:\n  \
         • installing it alongside cargo-trustc (`cargo install --path crates/{bin}`), or\n  \
         • pointing {env_override} at the binary (e.g. \
         `{env_override}=$(realpath target/debug/{bin})`)."
    )
}

/// Look for `bin` (with the platform executable extension) in the same
/// directory as the currently-running `cargo-trustc` binary.
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
        "cargo-trustc — the cargo bridge for the Trust toolchain\n\
         \n\
         USAGE:\n  \
         cargo trustc <command> [args...]\n\
         \n\
         CARGO COMMANDS (run with Trust lowering wired in automatically):\n  \
         build, run, test, check, clippy, doc, bench, install\n  \
         e.g.  cargo trustc build      # == RUSTC_WRAPPER/RUSTDOC set, then cargo build\n\
         \n\
         ADOPT (one-command migration of an existing crate into the dialect):\n  \
         cargo trustc adopt            # opt in + migrate calls + build, report what's left\n  \
         cargo trustc doctor           # check the toolchain setup and print fixes\n\
         \n\
         TRUST HELPERS (forwarded to the `trust` CLI):\n  \
         lower, index, fix, explain, …\n  \
         e.g.  cargo trustc explain R0042\n\
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
        assert_eq!(strip_trust_prefix(v(&["trustc", "build"])), v(&["build"]));
        assert_eq!(
            strip_trust_prefix(v(&["trustc", "explain", "R0042"])),
            v(&["explain", "R0042"])
        );
    }

    #[test]
    fn strip_is_idempotent_for_direct_invocation() {
        // `cargo-trustc build` (no cargo prefix) must not lose `build`.
        assert_eq!(strip_trust_prefix(v(&["build"])), v(&["build"]));
        assert_eq!(strip_trust_prefix(v(&[])), v(&[]));
    }

    #[test]
    fn only_strips_a_leading_trust_not_a_later_one() {
        // `cargo trustc run trustc` → after prefix strip, `run trustc` is intact.
        assert_eq!(
            strip_trust_prefix(v(&["trustc", "run", "trustc"])),
            v(&["run", "trustc"])
        );
    }

    #[test]
    fn cargo_lifecycle_commands_route_to_cargo() {
        for cmd in CARGO_COMMANDS {
            assert_eq!(dispatch(&v(&[cmd])), Dispatch::Cargo, "{cmd}");
        }
    }

    // RT-111/112: `adopt` and `doctor` are intercepted in run(), so they must
    // NOT route to the cargo path (which would try to run `cargo adopt`).
    #[test]
    fn helper_subcommands_are_not_cargo_commands() {
        for cmd in ["adopt", "doctor"] {
            assert!(!CARGO_COMMANDS.contains(&cmd), "{cmd}");
        }
    }

    // RT-112: probe_bin finds this very test binary by its own name (it lives
    // next to no `trust` shim, but a sibling lookup of an existing file works).
    #[test]
    fn probe_bin_honors_env_override() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("trust-rustc");
        std::fs::write(&fake, "x").unwrap();
        std::env::set_var("TRUST_RUSTC_PROBE_TEST", &fake);
        assert_eq!(
            super::probe_bin("trust-rustc", Some("TRUST_RUSTC_PROBE_TEST")),
            Some(fake)
        );
        std::env::remove_var("TRUST_RUSTC_PROBE_TEST");
    }

    // RT-111: ensure_strict_metadata appends the opt-in once, is idempotent,
    // and refuses to clobber a table that's present with a non-true value.
    #[test]
    fn ensure_strict_metadata_appends_then_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();

        assert!(matches!(
            super::ensure_strict_metadata(&manifest).unwrap(),
            super::MetadataOutcome::Added(t) if t == "package"
        ));
        let after = std::fs::read_to_string(&manifest).unwrap();
        assert!(after.contains("[package.metadata.trust]"));
        assert!(after.contains("strict = true"));
        // Parses, and a second run is a no-op (AlreadyStrict).
        assert!(toml::from_str::<toml::Value>(&after).is_ok());
        assert!(matches!(
            super::ensure_strict_metadata(&manifest).unwrap(),
            super::MetadataOutcome::AlreadyStrict
        ));
    }

    // RT-118: a workspace root opts in via [workspace.metadata.trust].
    #[test]
    fn ensure_strict_metadata_uses_workspace_table_for_a_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(&manifest, "[workspace]\nmembers = [\"a\"]\n").unwrap();
        assert!(matches!(
            super::ensure_strict_metadata(&manifest).unwrap(),
            super::MetadataOutcome::Added(t) if t == "workspace"
        ));
        let after = std::fs::read_to_string(&manifest).unwrap();
        assert!(after.contains("[workspace.metadata.trust]"));
        assert!(toml::from_str::<toml::Value>(&after).is_ok());
    }

    // RT-121: the cache fingerprint changes when the src set changes, so a new
    // or modified file busts the cached index.
    #[test]
    fn src_fingerprint_changes_with_src_set() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.rs"), "fn a() {}").unwrap();
        let dirs = vec![src.clone()];
        let fp1 = super::src_fingerprint(&dirs);
        std::fs::write(src.join("b.rs"), "fn b() {}").unwrap();
        assert_ne!(
            fp1,
            super::src_fingerprint(&dirs),
            "new file must change fp"
        );
    }

    // RT-114: the auto signature index gathers every member's src/ dir.
    #[test]
    fn workspace_src_dirs_finds_member_srcs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\"]\n",
        )
        .unwrap();
        for m in ["a", "b"] {
            std::fs::create_dir_all(root.join(m).join("src")).unwrap();
            std::fs::write(
                root.join(m).join("Cargo.toml"),
                format!("[package]\nname = \"{m}\"\nversion = \"0.1.0\"\n"),
            )
            .unwrap();
        }
        let dirs = super::workspace_src_dirs(&root.join("Cargo.toml"));
        let members: Vec<String> = dirs
            .iter()
            .filter_map(|d| {
                d.parent()?
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .collect();
        assert!(members.contains(&"a".to_string()), "{members:?}");
        assert!(members.contains(&"b".to_string()), "{members:?}");
    }

    #[test]
    fn ensure_strict_metadata_refuses_non_true_table() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[package.metadata.trust]\nstrict = false\n",
        )
        .unwrap();
        assert!(matches!(
            super::ensure_strict_metadata(&manifest).unwrap(),
            super::MetadataOutcome::TableExistsNotStrict(t) if t == "package"
        ));
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
        parse_strict_package(&toml::from_str::<toml::Value>(s).unwrap())
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

    /// RT-96: both flag forms are stripped from the args cargo sees and the
    /// value is surfaced for the env-var translation.
    #[test]
    fn message_format_space_form_is_extracted_and_stripped() {
        let (rest, fmt) =
            extract_message_format(&v(&["build", "--message-format", "json", "--release"]))
                .unwrap();
        assert_eq!(rest, v(&["build", "--release"]));
        assert_eq!(fmt, Some("json".to_string()));
    }

    #[test]
    fn message_format_equals_form_is_extracted_and_stripped() {
        let (rest, fmt) = extract_message_format(&v(&["build", "--message-format=json"])).unwrap();
        assert_eq!(rest, v(&["build"]));
        assert_eq!(fmt, Some("json".to_string()));
    }

    #[test]
    fn message_format_absent_passes_args_through() {
        let (rest, fmt) = extract_message_format(&v(&["build", "--release"])).unwrap();
        assert_eq!(rest, v(&["build", "--release"]));
        assert_eq!(fmt, None);
    }

    /// An unsupported value or a dangling flag with no value is a clear error
    /// naming the supported set — never silently forwarded to cargo.
    #[test]
    fn message_format_unsupported_value_and_missing_value_are_errors() {
        let err = extract_message_format(&v(&["build", "--message-format", "short"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("supported values: json"), "{err}");
        let err = extract_message_format(&v(&["build", "--message-format=human"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("supported values: json"), "{err}");
        assert!(extract_message_format(&v(&["build", "--message-format"])).is_err());
    }

    /// PR #1 review regression: a root-level [workspace.metadata.trust]
    /// opt-in must apply when cargo trustc is invoked from a MEMBER directory
    /// (whose manifest has no [workspace] table).
    #[test]
    fn workspace_opt_in_found_from_member_manifest() {
        let base = std::env::temp_dir().join(format!("cargo-trustc-pr1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("m/src")).unwrap();
        std::fs::write(
            base.join("Cargo.toml"),
            "[workspace]\nmembers = [\"m\"]\n[workspace.metadata.trust]\nstrict = true\n",
        )
        .unwrap();
        std::fs::write(
            base.join("m/Cargo.toml"),
            "[package]\nname = \"member-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();

        let args = v(&[
            "build",
            "--manifest-path",
            base.join("m/Cargo.toml").to_str().unwrap(),
        ]);
        assert_eq!(strict_packages(&args), vec!["member-crate".to_string()]);

        // A member that IS its own root (empty [workspace] opt-out) must not
        // inherit the ancestor's opt-in.
        std::fs::create_dir_all(base.join("standalone/src")).unwrap();
        std::fs::write(
            base.join("standalone/Cargo.toml"),
            "[workspace]\n[package]\nname = \"standalone\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let args = v(&[
            "build",
            "--manifest-path",
            base.join("standalone/Cargo.toml").to_str().unwrap(),
        ]);
        assert!(strict_packages(&args).is_empty());

        let _ = std::fs::remove_dir_all(&base);
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
        let root = env::temp_dir().join(format!("cargo-trustc-ws-{tag}-{}", std::process::id()));
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
