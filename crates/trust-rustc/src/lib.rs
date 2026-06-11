//! Shared lowering/cache/mirror logic used by the `trust-rustc`
//! (`RUSTC_WRAPPER`) and `trust-rustdoc` (`RUSTDOC`) shims.
//!
//! Both wrappers do the same job: given a rustc/rustdoc invocation, find
//! the input `.rs` file, and — if it's strict-marked — lower the whole
//! source tree into a temp directory keyed by an FNV-1a content hash, then
//! rewrite the input path so the underlying tool sees plain Rust.
//!
//! The functions here are the parts that don't depend on whether we're
//! about to exec `rustc` or `rustdoc`. The two `main.rs` files differ only
//! in how they parse out the tool path / input arg.

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Version string mixed into the cache key. Bumps automatically with the
/// package version; bump the package whenever lowering output changes in
/// a way that would invalidate previously-cached files.
const LOWERING_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Fingerprint of the running wrapper binary (length ⊕ mtime), mixed into
/// the cache key (RT-86). The package version alone is constant across a
/// whole dev cycle, so a rebuilt wrapper with changed lowering code would
/// happily reuse lowered output produced by the previous build — which is
/// exactly the kind of stale-cache haunting that makes verification results
/// flip between runs. A new binary now always means a fresh cache namespace.
fn wrapper_fingerprint() -> u64 {
    use std::sync::OnceLock;
    static FP: OnceLock<u64> = OnceLock::new();
    *FP.get_or_init(|| {
        let Ok(exe) = env::current_exe() else {
            return 0;
        };
        let Ok(meta) = fs::metadata(&exe) else {
            return 0;
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        meta.len() ^ mtime
    })
}

/// FNV-1a 64-bit hash of the lowering-version string, the wrapper binary's
/// fingerprint, and the source bytes. Fast, no deps, deterministic per
/// wrapper build.
pub fn source_cache_key(source: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in LOWERING_VERSION
        .bytes()
        .chain(wrapper_fingerprint().to_le_bytes())
        .chain(source.bytes())
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// FNV-1a hash of the nearest `trust.toml` content at or above `input_path`,
/// mixed into the cache key (RT-113) so a config change re-triggers linting
/// even when the source is unchanged. Returns 0 when there is no config — the
/// common case, leaving the key unchanged. Walks the same way
/// [`trust_lints::TrustConfig::discover`] does, so the salt and the applied
/// config always agree on which file is in effect.
fn config_cache_salt(input_path: &Path) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut dir = input_path.parent();
    while let Some(d) = dir {
        if let Ok(text) = fs::read_to_string(d.join("trust.toml")) {
            let mut hash = FNV_OFFSET;
            for byte in text.bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
            return hash;
        }
        dir = d.parent();
    }
    0
}

/// Result of preparing a strict-source invocation: the path to the lowered
/// crate-root file, and a `--remap-path-prefix=<cache>=<orig>` flag the
/// caller should append to the tool args so diagnostics still point at the
/// user's source.
pub struct Prepared {
    pub lowered_root: PathBuf,
    pub remap_flag: String,
}

/// Walk `src_dir` recursively, parsing every `.rs` file with `syn` and
/// collecting `(fn_name, [param_names...])` for every module-level `fn`
/// definition (free fns, `pub` or otherwise; module-nested `fn`s
/// included). Used by [`prepare_strict_input`] to build a crate-wide
/// callee registry so cross-file named-arg call sites resolve (RT-40).
///
/// **What's covered:** plain free fns at module level, including inside
/// `mod foo { ... }` blocks within a single file. **What's not:** trait
/// methods, `impl` methods, and fns inside file-mod descendants that
/// `syn::parse_file` can't reach (those will still be picked up when the
/// file itself is parsed, because the recursive walk visits every `.rs`).
///
/// **Ambiguity policy:** if two files declare a fn with the same name
/// but different param lists, the name is dropped from the index — same
/// behaviour as the in-file collector. Dropping is safer than guessing
/// which signature the caller meant.
///
/// Parse errors and unreadable files are silently skipped — the wrapper
/// stays best-effort.  The downstream lowering pass will surface real
/// errors on the file that actually has the syntax problem.
pub fn collect_crate_callees(src_dir: &Path) -> Vec<(String, Vec<String>)> {
    use std::collections::{HashMap, HashSet};
    let mut sigs: HashMap<String, Vec<String>> = HashMap::new();
    let mut ambiguous: HashSet<String> = HashSet::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    collect_crate_callees_recursive(src_dir, &mut sigs, &mut ambiguous, &mut visited);
    let mut out: Vec<(String, Vec<String>)> = sigs.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect_crate_callees_recursive(
    dir: &Path,
    sigs: &mut std::collections::HashMap<String, Vec<String>>,
    ambiguous: &mut std::collections::HashSet<String>,
    visited: &mut std::collections::HashSet<PathBuf>,
) {
    if !dir.is_dir() {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !visited.insert(canonical) {
            continue;
        }
        if path.is_dir() {
            collect_crate_callees_recursive(&path, sigs, ambiguous, visited);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            if let Ok(source) = fs::read_to_string(&path) {
                // Pre-lower the source so syn can parse it. The crate-wide
                // index is derived from the *lowered* signatures — but
                // since fn signatures don't use named-arg call syntax,
                // syn::parse_file on the raw source usually works. Try
                // raw first; on failure, fall through to lowered.
                let file = syn::parse_file(&source).ok().or_else(|| {
                    trust_lower::lower(&source)
                        .ok()
                        .and_then(|lo| syn::parse_file(&lo.source).ok())
                });
                if let Some(file) = file {
                    walk_items_for_sigs(&file.items, sigs, ambiguous);
                }
            }
        }
    }
}

fn walk_items_for_sigs(
    items: &[syn::Item],
    sigs: &mut std::collections::HashMap<String, Vec<String>>,
    ambiguous: &mut std::collections::HashSet<String>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f) => record_fn_sig(&f.sig, sigs, ambiguous),
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    walk_items_for_sigs(inner, sigs, ambiguous);
                }
            }
            _ => {}
        }
    }
}

fn record_fn_sig(
    sig: &syn::Signature,
    sigs: &mut std::collections::HashMap<String, Vec<String>>,
    ambiguous: &mut std::collections::HashSet<String>,
) {
    let name = sig.ident.to_string();
    if ambiguous.contains(&name) {
        return;
    }
    let mut params: Vec<String> = Vec::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(_) => {} // skip self
            syn::FnArg::Typed(pat_type) => match &*pat_type.pat {
                syn::Pat::Ident(pi) => params.push(pi.ident.to_string()),
                _ => {
                    // Non-ident pattern (destructure) — can't bind by name.
                    sigs.remove(&name);
                    ambiguous.insert(name);
                    return;
                }
            },
        }
    }
    match sigs.get(&name) {
        Some(existing) if existing != &params => {
            sigs.remove(&name);
            ambiguous.insert(name);
        }
        Some(_) => {}
        None => {
            sigs.insert(name, params);
        }
    }
}

/// Find the input `.rs` file argument in a rustc/rustdoc arg list.
///
/// Cargo passes exactly one `.rs` crate-root per invocation. Flag args
/// start with `-`, and a bare `-` means "read from stdin" (skip).
pub fn find_input_rs(args: &[String]) -> Option<usize> {
    args.iter().enumerate().find_map(|(i, a)| {
        if a == "-" {
            return None;
        }
        if a.ends_with(".rs") && !a.starts_with('-') {
            Some(i)
        } else {
            None
        }
    })
}

/// Whether the crate currently being compiled was opted into strict mode at
/// the *project* level (`[package.metadata.trust] strict = true`), rather than
/// per-file with a `#![strict]` marker.
///
/// `cargo-trustc` passes the set of strict package names in
/// `TRUST_STRICT_PACKAGES` (comma-separated). Cargo sets `CARGO_PKG_NAME` for
/// every rustc invocation — including dependencies — so gating on membership
/// scopes forced lowering to exactly the user's own opted-in package(s) and
/// never touches third-party crates compiled in the same build.
pub fn crate_is_force_strict() -> bool {
    force_strict_for(
        env::var("TRUST_STRICT_PACKAGES").ok().as_deref(),
        env::var("CARGO_PKG_NAME").ok().as_deref(),
    )
}

/// Pure membership check behind [`crate_is_force_strict`]: is `name` listed in
/// the comma-separated `pkgs` set? An empty/absent name or list is never a
/// match — this is what keeps dependencies (which carry their own
/// `CARGO_PKG_NAME`, not in the user's set) out of forced lowering.
fn force_strict_for(pkgs: Option<&str>, name: Option<&str>) -> bool {
    let (Some(pkgs), Some(name)) = (pkgs, name) else {
        return false;
    };
    let name = name.trim();
    !name.is_empty() && pkgs.split(',').any(|p| p.trim() == name)
}

/// True if a file should be lowered: either it carries an explicit strict
/// marker, or its crate was opted in at the project level.
fn should_lower(source: &str) -> bool {
    trust_lower::is_strict_source(source) || crate_is_force_strict()
}

/// If `input_path` is strict (per-file marker or project-level opt-in), lower
/// the whole source tree into the cache and return the new root path +
/// a `--remap-path-prefix` flag.
///
/// Returns `Ok(None)` for non-strict sources — the caller should pass the
/// original args through to the underlying tool unchanged.
pub fn prepare_strict_input(input_path: &Path) -> Result<Option<Prepared>> {
    let source = match fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };

    if !should_lower(&source) {
        return Ok(None);
    }
    let force_strict = crate_is_force_strict();

    let file_name = input_path
        .file_name()
        .context("input path has no file name")?;

    // RT-113: fold the nearest trust.toml into the cache key so editing the
    // config (e.g. adding `warn = [...]`) re-triggers the gate even when the
    // source is unchanged — otherwise a cache hit would skip re-linting and the
    // new config would silently not apply.
    let cache_key = source_cache_key(&source) ^ config_cache_salt(input_path);
    let cache_root = env::temp_dir().join("trust-cache");
    let cache_dir = cache_root.join(format!("{cache_key:016x}"));
    let cached_file = cache_dir.join(file_name);

    // RT-86: the cache directory's EXISTENCE is the validity marker, so it
    // must appear atomically. A failed mirror used to leave a partial dir
    // behind, and the old per-file `cached_file.exists()` check then treated
    // it as complete on the next run — phantom passes/failures that flip
    // depending on which run came first. Populate a private staging dir and
    // rename it into place only after every file lowered clean.
    if !cache_dir.exists() {
        let staging = cache_root.join(format!(".staging-{cache_key:016x}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);

        let result = (|| -> Result<()> {
            let src_dir = input_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));

            // RT-40: pre-scan the whole `src/` tree for `fn` definitions so
            // cross-file named-arg call sites resolve. The wrapper is the
            // first place that has a crate-wide view; individual `lower()`
            // calls only see one file at a time.
            let crate_extras = collect_crate_callees(&src_dir);

            // RT-66: seed the registry with the public-fn signatures of
            // dependencies, discovered from the `TRUST_SIGNATURE_PATH`
            // manifests (`trust index <dep> -o …` produces them). This is
            // what lets R0042 fire — and named args reorder — on a
            // positional swap into a *third-party* crate. `merge` drops any
            // name that conflicts between the crate and a dependency, so a
            // shadowed name degrades to the positional fallback rather than
            // a wrong reorder.
            let dep_extras = trust_lower::sig_index::load_from_env();
            let extras = trust_lower::sig_index::merge(&[crate_extras, dep_extras]);

            let mut visited = std::collections::HashSet::new();
            mirror_module_tree_with_extras(&src_dir, &staging, &mut visited, &extras)
                .with_context(|| format!("mirroring src tree from {}", src_dir.display()))?;

            // Defensive: if the src_dir traversal somehow didn't write the
            // crate root (e.g. empty dir), do it directly.
            if !staging.join(file_name).exists() {
                let out =
                    trust_lower::lower_with_extra_callees_forced(&source, &extras, force_strict)
                        .with_context(|| format!("lowering {}", input_path.display()))?;
                emit_diagnostics(&out, &source, input_path)?;
                fs::create_dir_all(&staging)?;
                fs::write(staging.join(file_name), &out.source)?;
            }
            Ok(())
        })();

        if let Err(e) = result {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }

        // Atomic publish. If another process won the race, its complete dir
        // is just as good — discard ours.
        if fs::rename(&staging, &cache_dir).is_err() {
            let _ = fs::remove_dir_all(&staging);
            if !cache_dir.exists() {
                bail!(
                    "could not publish lowering cache at {}",
                    cache_dir.display()
                );
            }
        }
    }

    let parent = input_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    Ok(Some(Prepared {
        lowered_root: cached_file,
        remap_flag: format!(
            "--remap-path-prefix={}={}",
            cache_dir.display(),
            parent.display()
        ),
    }))
}

fn emit_diagnostics(
    out: &trust_lower::LowerOutput,
    original_source: &str,
    path: &Path,
) -> Result<()> {
    emit_diagnostics_to(out, original_source, path, &mut std::io::stderr())
}

/// True when the caller asked for machine-readable diagnostics (RT-96).
///
/// `cargo trustc build --message-format json` sets this env var on the spawned
/// cargo process; cargo passes its environment through to every rustc/rustdoc
/// (and therefore wrapper) invocation. Users may also set
/// `TRUST_MESSAGE_FORMAT=json` directly — same effect, no flag needed.
fn message_format_is_json() -> bool {
    env::var("TRUST_MESSAGE_FORMAT").is_ok_and(|v| v == "json")
}

/// Testable core of [`emit_diagnostics`]: collects the full `trust check`
/// rule set for one file and writes it to `writer` — as human `[R0001]`
/// lines by default, or as one `trust_diag::to_json` document (newline
/// terminated) when `TRUST_MESSAGE_FORMAT=json` (RT-96). Either way, bails
/// when any diagnostic is an error.
fn emit_diagnostics_to(
    out: &trust_lower::LowerOutput,
    original_source: &str,
    path: &Path,
    writer: &mut impl std::io::Write,
) -> Result<()> {
    // RT-89: the wrapper enforces the same rule set as `trust check` — the
    // lowering diagnostics (R0042 et al) collected in `out`, plus the
    // AST-level strict lints (R0001 unwrap, R0003 as-cast, ...). The AST
    // comes from the LOWERED source (plain Rust, always parses); the
    // ORIGINAL source string is what the linter needs for comment-window
    // rules (R0005/R0006 justifications) — prettyplease strips comments
    // from the lowered output. Mirrors `run_pipeline` in the trust CLI.
    let mut diagnostics = out.diagnostics.clone();
    if out.strict_mode {
        // lint_source, not source: the allow map comes from the
        // `#[allow(trust::…)]` attributes, which are stripped from the
        // rustc-facing `source`.
        let file: syn::File = syn::parse_str(&out.lint_source)
            .with_context(|| format!("re-parsing lowered source from {}", path.display()))?;
        diagnostics.extend(trust_lints::lint_strict(&file, original_source, true).diagnostics);
    }

    // RT-113: honor a project `trust.toml` in the build gate, same as
    // `trust check` — drop `allow`-listed codes and downgrade `warn`-listed
    // ones to non-failing warnings. A malformed config fails the build loudly.
    let config = trust_lints::TrustConfig::discover(path)
        .with_context(|| format!("loading trust.toml for {}", path.display()))?;
    config.apply(&mut diagnostics);

    if message_format_is_json() {
        // RT-96: one JSON document per file, same shape as
        // `trust check --format json` (spans index the ORIGINAL source).
        let name = path.display().to_string();
        let doc = trust_diag::to_json(
            &diagnostics,
            trust_diag::NamedSource {
                name: &name,
                text: original_source,
            },
        );
        write!(writer, "{doc}")?;
        if !doc.ends_with('\n') {
            writeln!(writer)?;
        }
    } else {
        for diag in &diagnostics {
            writeln!(
                writer,
                "[{}] {}: {}",
                diag.rule,
                if diag.is_error() { "error" } else { "warning" },
                diag.message
            )?;
        }
    }
    if diagnostics.iter().any(|d| d.is_error()) {
        bail!("trust check failed on {}", path.display());
    }
    Ok(())
}

/// Files reachable only through a `#[cfg(test)] mod x;` declaration (RT-88).
///
/// Project-level force-strict must not apply to these: a stock-buildable
/// library's tests routinely call its own multi-arg fns positionally, and
/// the R0042 fix — named-arg syntax — is exactly what stock `cargo test`
/// cannot parse. Skipping cfg(test)-only files lets such crates opt their
/// *shipping* code into whole-package strict (trust-diag, trust-std) without
/// rewriting their test suites in a dialect stock rustc rejects. A file that
/// carries its own `#![strict]` marker is still lowered — explicit wins.
///
/// Detection is token-level (Trust syntax doesn't parse with syn): a file
/// declares `NAME` test-only via `#[cfg(test)] (pub)? mod NAME ;`, mapping
/// to `NAME.rs` or `NAME/mod.rs` beside it — and test-only-ness is
/// transitive through plain `mod` declarations inside test-only files.
pub fn collect_test_only_files(src_dir: &Path) -> std::collections::HashSet<PathBuf> {
    use std::collections::HashSet;
    let mut all_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(src_dir, &mut all_files);

    // (declaring file, declared name, is_cfg_test)
    let mut decls: Vec<(PathBuf, String, bool)> = Vec::new();
    for file in &all_files {
        let Ok(source) = fs::read_to_string(file) else {
            continue;
        };
        let Ok(tokens) = source.parse::<proc_macro2::TokenStream>() else {
            continue;
        };
        for (name, is_test) in file_mod_declarations(&tokens) {
            decls.push((file.clone(), name, is_test));
        }
    }

    let resolve = |declaring: &Path, name: &str| -> Option<PathBuf> {
        let dir = declaring.parent()?;
        let flat = dir.join(format!("{name}.rs"));
        if flat.is_file() {
            return flat.canonicalize().ok();
        }
        let nested = dir.join(name).join("mod.rs");
        if nested.is_file() {
            return nested.canonicalize().ok();
        }
        None
    };

    let mut test_only: HashSet<PathBuf> = HashSet::new();
    // Seed with direct #[cfg(test)] declarations, then close transitively
    // over plain mod declarations made from already-test-only files.
    loop {
        let mut grew = false;
        for (declaring, name, is_test) in &decls {
            let from_test_file = declaring
                .canonicalize()
                .map(|c| test_only.contains(&c))
                .unwrap_or(false);
            if !is_test && !from_test_file {
                continue;
            }
            if let Some(target) = resolve(declaring, name) {
                grew |= test_only.insert(target);
            }
        }
        if !grew {
            break;
        }
    }
    test_only
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Does a `cfg(...)` argument list make the item test-only — i.e. is `test`
/// present as a POSITIVE predicate? `test` and `all(unix, test)` qualify;
/// `not(test)` and `all(unix, not(test))` do not (those select NON-test
/// builds, so exempting them would skip lowering/linting in production
/// compiles — PR #1 review finding). `not(...)` subtrees are never recursed
/// into; `any(...)`/`all(...)` are.
fn cfg_args_positively_test(tokens: &proc_macro2::TokenStream) -> bool {
    use proc_macro2::{Delimiter, TokenTree};
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let mut i = 0;
    while i < trees.len() {
        match &trees[i] {
            TokenTree::Ident(id) if *id == "not" => {
                // Skip the negated group entirely.
                if matches!(trees.get(i + 1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            TokenTree::Ident(id) if *id == "any" || *id == "all" => {
                if let Some(TokenTree::Group(g)) = trees.get(i + 1) {
                    if g.delimiter() == Delimiter::Parenthesis
                        && cfg_args_positively_test(&g.stream())
                    {
                        return true;
                    }
                    i += 2;
                    continue;
                }
                i += 1;
            }
            // Bare `test` predicate — not the LHS of `name = "value"` (the
            // RHS of those is a Literal, so an Ident named test here is the
            // predicate form).
            TokenTree::Ident(id) if *id == "test" => {
                let followed_by_eq = matches!(
                    trees.get(i + 1),
                    Some(TokenTree::Punct(p)) if p.as_char() == '='
                );
                if !followed_by_eq {
                    return true;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
}

/// Top-level `mod NAME ;` declarations in a token stream, with whether the
/// directly-preceding attribute run contains a positively-`test` cfg.
fn file_mod_declarations(tokens: &proc_macro2::TokenStream) -> Vec<(String, bool)> {
    use proc_macro2::{Delimiter, TokenTree};
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut pending_cfg_test = false;
    while i < trees.len() {
        match &trees[i] {
            // Attribute: `#` `[ ... ]` — note whether it's cfg(test).
            TokenTree::Punct(p) if p.as_char() == '#' => {
                if let Some(TokenTree::Group(g)) = trees.get(i + 1) {
                    if g.delimiter() == Delimiter::Bracket {
                        let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                        if let [TokenTree::Ident(name), TokenTree::Group(args)] = inner.as_slice() {
                            if *name == "cfg" {
                                pending_cfg_test |= cfg_args_positively_test(&args.stream());
                            }
                        }
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            }
            // `pub` (and `pub(...)`) between attrs and `mod` — skip.
            TokenTree::Ident(id) if *id == "pub" => {
                i += 1;
                if let Some(TokenTree::Group(g)) = trees.get(i) {
                    if g.delimiter() == Delimiter::Parenthesis {
                        i += 1;
                    }
                }
            }
            TokenTree::Ident(id) if *id == "mod" => {
                if let (Some(TokenTree::Ident(name)), Some(TokenTree::Punct(semi))) =
                    (trees.get(i + 1), trees.get(i + 2))
                {
                    if semi.as_char() == ';' {
                        out.push((name.to_string(), pending_cfg_test));
                    }
                }
                pending_cfg_test = false;
                i += 1;
            }
            _ => {
                pending_cfg_test = false;
                i += 1;
            }
        }
    }
    out
}

/// Recursively mirror the source tree rooted at `src_dir` into `dest_dir`,
/// lowering strict-marked `.rs` files and hard-linking/copying others.
pub fn mirror_module_tree(
    src_dir: &Path,
    dest_dir: &Path,
    already_done: &mut std::collections::HashSet<PathBuf>,
) -> Result<()> {
    mirror_module_tree_with_extras(src_dir, dest_dir, already_done, &[])
}

/// Variant of [`mirror_module_tree`] that threads a crate-wide list of
/// `(fn_name, params)` entries into every per-file lowering call. Used by
/// `prepare_strict_input` to resolve cross-file named-arg call sites
/// (RT-40).
pub fn mirror_module_tree_with_extras(
    src_dir: &Path,
    dest_dir: &Path,
    already_done: &mut std::collections::HashSet<PathBuf>,
    extras: &[(String, Vec<String>)],
) -> Result<()> {
    // RT-88: under project-level force-strict, cfg(test)-only files keep
    // their plain-Rust form (see collect_test_only_files). Computed once per
    // mirror at the root call.
    let test_only = if crate_is_force_strict() {
        collect_test_only_files(src_dir)
    } else {
        std::collections::HashSet::new()
    };
    mirror_inner(src_dir, dest_dir, already_done, extras, &test_only)
}

fn mirror_inner(
    src_dir: &Path,
    dest_dir: &Path,
    already_done: &mut std::collections::HashSet<PathBuf>,
    extras: &[(String, Vec<String>)],
    test_only: &std::collections::HashSet<PathBuf>,
) -> Result<()> {
    if !src_dir.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dest_dir).with_context(|| format!("creating {}", dest_dir.display()))?;

    for entry in
        fs::read_dir(src_dir).with_context(|| format!("reading dir {}", src_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let dest = dest_dir.join(entry.file_name());

        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        let is_test_only = test_only.contains(&canonical);
        if !already_done.insert(canonical) {
            continue;
        }

        if path.is_dir() {
            mirror_inner(&path, &dest, already_done, extras, test_only)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let source =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            // Explicit #![strict] always lowers; force-strict lowers
            // everything except cfg(test)-only files (RT-88).
            let lower_this = trust_lower::is_strict_source(&source)
                || (crate_is_force_strict() && !is_test_only);
            if lower_this {
                let out = trust_lower::lower_with_extra_callees_forced(
                    &source,
                    extras,
                    crate_is_force_strict(),
                )
                .with_context(|| format!("lowering {}", path.display()))?;
                emit_diagnostics(&out, &source, &path)?;
                // Also lower any doc-test code blocks embedded in `///` /
                // `//!` comments. rustdoc extracts these snippets verbatim
                // and submits them to rustc; if they contain named-arg
                // syntax they'd fail on stable. Best-effort: leave blocks
                // we can't parse untouched (e.g. `ignore`/`text` fences,
                // or partial snippets that don't parse standalone).
                let rewritten = lower_doctests_in_source(&out.source);
                let tmp = dest_dir.join(format!(
                    ".{}.{}.tmp",
                    entry.file_name().to_string_lossy(),
                    std::process::id()
                ));
                fs::write(&tmp, &rewritten)?;
                fs::rename(&tmp, &dest)?;
            } else {
                // RT-75: COPY, never hard-link. A hard link shares the inode
                // with the source file, so any later write/truncate of the
                // cached copy would destroy the user's original `.rs`.
                fs::copy(&path, &dest).with_context(|| format!("copying {}", path.display()))?;
            }
        } else {
            // RT-75: non-`.rs` sibling files — copy (best-effort), never
            // hard-link, for the same inode-sharing reason as above.
            let _ = fs::copy(&path, &dest);
        }
    }
    Ok(())
}

/// Lower Trust syntax inside doc-test code blocks (`/// ```...```` ` and
/// `//! ```...```` `) so `rustdoc --test` doesn't choke when rustc compiles
/// each snippet on stable. Used by the mirror pass after the file itself
/// has been lowered.
///
/// Strategy: walk the source line-by-line, find runs of doc-comment lines
/// (`///` or `//!`), then within each run locate ```` ``` ```` fences. The
/// fence info-string is treated as a doc-test if it's empty or starts with
/// `rust` (mirroring rustdoc's own classification). Non-test fences
/// (`text`, `ignore`, `compile_fail`, …) are left alone — rustdoc won't
/// hand them to rustc anyway, and `compile_fail` tests intentionally don't
/// compile, so re-lowering them could hide the intended failure.
///
/// For each test snippet we try two parse strategies:
///   1. Lower the snippet as-is (it's already a valid Rust file).
///   2. If that fails, wrap in `fn __doctest() { … }` and lower; on
///      success, strip the wrapper.
///
/// If both fail (snippet doesn't parse standalone — e.g. it's only an
/// expression, or has hidden `#`-prefixed lines), we leave the block
/// unchanged. The doc-test will fail at rustc time with a clearer error
/// than anything we could produce.
pub fn lower_doctests_in_source(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let (Some(prefix), Some(_)) = (doc_prefix(lines[i]), doc_body(lines[i])) else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
            continue;
        };
        // Collect this doc-comment block (consecutive lines with the same prefix).
        let block_start = i;
        while i < lines.len() && doc_prefix(lines[i]) == Some(prefix) {
            i += 1;
        }
        let block_end = i;
        let block = rewrite_doc_block(&lines[block_start..block_end], prefix);
        out.push_str(&block);
        // `rewrite_doc_block` always ends with a newline-per-line layout.
    }
    out
}

fn doc_prefix(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("///") {
        Some("///")
    } else if trimmed.starts_with("//!") {
        Some("//!")
    } else {
        None
    }
}

fn doc_body(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix("///")
        .or_else(|| trimmed.strip_prefix("//!"))?;
    Some(body.strip_prefix(' ').unwrap_or(body))
}

/// Rewrite a contiguous doc-comment block, transforming code-fenced
/// doc-test snippets through `trust_lower::lower`.
fn rewrite_doc_block(lines: &[&str], prefix: &str) -> String {
    // Extract the indent of the first line so we can reproduce it.
    let first = lines[0];
    let indent_len = first.len() - first.trim_start().len();
    let indent = &first[..indent_len];

    // Walk lines; when we hit a fence inside a doc-test block, buffer
    // the code lines, lower the buffer, then splice the lowered text
    // back as new doc-comment lines.
    let mut out = String::new();
    let mut in_block = false;
    let mut is_test_block = false;
    let mut code_buf = String::new();
    let mut block_indent_after_prefix = String::new();

    for line in lines {
        let body = doc_body(line).unwrap_or("");
        let body_trim = body.trim_start();

        if body_trim.starts_with("```") {
            if !in_block {
                // Opening fence. Decide if this is a doc-test fence.
                let info = body_trim.trim_start_matches('`').trim();
                is_test_block = info.is_empty()
                    || info == "rust"
                    || info.starts_with("rust,")
                    || info.starts_with("rust ");
                in_block = true;
                code_buf.clear();
                block_indent_after_prefix.clear();
                // Capture the indentation that lives *between* `///` and
                // the visible body, so we can reproduce it on output.
                if let Some(stripped) = line.trim_start().strip_prefix(prefix) {
                    let after = stripped;
                    let extra_indent_len = after.len() - after.trim_start().len();
                    block_indent_after_prefix = after[..extra_indent_len].to_string();
                }
                out.push_str(line);
                out.push('\n');
                continue;
            }
            // Closing fence: flush the buffered code (lowered if possible).
            let lowered = if is_test_block {
                try_lower_doctest(&code_buf).unwrap_or_else(|| code_buf.clone())
            } else {
                code_buf.clone()
            };
            for code_line in lowered.lines() {
                out.push_str(indent);
                out.push_str(prefix);
                if !code_line.is_empty() {
                    if block_indent_after_prefix.is_empty() {
                        out.push(' ');
                    } else {
                        out.push_str(&block_indent_after_prefix);
                    }
                }
                out.push_str(code_line);
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
            in_block = false;
            code_buf.clear();
            continue;
        }

        if in_block {
            // Accumulate the raw body (minus the doc prefix + one space).
            code_buf.push_str(body);
            code_buf.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    // Unclosed fence — emit the buffer verbatim to avoid losing content.
    if in_block {
        for code_line in code_buf.lines() {
            out.push_str(indent);
            out.push_str(prefix);
            out.push(' ');
            out.push_str(code_line);
            out.push('\n');
        }
    }
    out
}

/// Try to lower a doc-test snippet. Returns `Some(lowered)` if the
/// rewriter produced new source; `None` if the snippet doesn't parse
/// standalone (leave unchanged in that case).
fn try_lower_doctest(snippet: &str) -> Option<String> {
    // Strategy 1: snippet is a full Rust file (contains `fn main`, items, etc.).
    if let Ok(out) = trust_lower::lower(snippet) {
        if !out.diagnostics.iter().any(|d| d.is_error()) {
            return Some(strip_hidden_doctest_prefix(out.source));
        }
    }
    // Strategy 2: wrap as `fn __d() { … }` (snippet is a stmt sequence).
    let wrapped = format!("fn __trust_doctest() {{\n{snippet}\n}}\n");
    let out = trust_lower::lower(&wrapped).ok()?;
    if out.diagnostics.iter().any(|d| d.is_error()) {
        return None;
    }
    // Strip the wrapper. prettyplease emits a stable shape:
    //     fn __trust_doctest() {
    //         <body>
    //     }
    let unwrapped = unwrap_doctest_fn(&out.source)?;
    Some(unwrapped)
}

fn unwrap_doctest_fn(source: &str) -> Option<String> {
    let start = source.find("fn __trust_doctest()")?;
    let open = source[start..].find('{')? + start;
    // Find the matching close brace.
    let bytes = source.as_bytes();
    let mut depth = 0i32;
    let mut close = None;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let body = &source[open + 1..close];
    // Strip leading/trailing blank lines and dedent four-space indent
    // (prettyplease default).
    let mut lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let dedent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let out: String = lines
        .iter()
        .map(|l| {
            if l.len() >= dedent {
                format!("{}\n", &l[dedent..])
            } else {
                "\n".to_string()
            }
        })
        .collect();
    Some(out)
}

/// rustdoc treats lines beginning with `# ` (after the doc-comment prefix)
/// as hidden setup. Our lowering loses that distinction because we feed
/// the raw body to syn. After lowering, restore the `# ` markers wouldn't
/// be possible — so for now we just pass through (Rust file strategy
/// already drops `#`-prefixed lines silently if they aren't syntax).
fn strip_hidden_doctest_prefix(s: String) -> String {
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises tests that read or write `TRUST_MESSAGE_FORMAT` — the
    /// process env is shared across parallel test threads.
    static MESSAGE_FORMAT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Scoped env guard: sets (or clears) `TRUST_MESSAGE_FORMAT` and restores
    /// the previous value on drop, holding [`MESSAGE_FORMAT_LOCK`] throughout
    /// so the env mutation can't leak into a concurrently-running test.
    struct MessageFormatGuard<'a> {
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'a, ()>,
    }

    impl MessageFormatGuard<'_> {
        fn set(value: Option<&str>) -> Self {
            let lock = MESSAGE_FORMAT_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let prev = env::var("TRUST_MESSAGE_FORMAT").ok();
            match value {
                Some(v) => env::set_var("TRUST_MESSAGE_FORMAT", v),
                None => env::remove_var("TRUST_MESSAGE_FORMAT"),
            }
            MessageFormatGuard { prev, _lock: lock }
        }
    }

    impl Drop for MessageFormatGuard<'_> {
        fn drop(&mut self) {
            match &self.prev {
                Some(prev) => env::set_var("TRUST_MESSAGE_FORMAT", prev),
                None => env::remove_var("TRUST_MESSAGE_FORMAT"),
            }
        }
    }

    /// RT-96: with `TRUST_MESSAGE_FORMAT=json`, the wrapper emits one
    /// machine-parseable JSON document per file (same shape as
    /// `trust check --format json`) instead of human `[R0001]` lines — and
    /// still bails because the diagnostic is an error.
    #[test]
    fn json_message_format_emits_parseable_document() {
        let _guard = MessageFormatGuard::set(Some("json"));

        let source =
            "#![strict]\nfn main() { let v: Option<i32> = Some(1); let _ = v.unwrap(); }\n";
        let out = trust_lower::lower(source).expect("lowering strict source");
        let mut buf: Vec<u8> = Vec::new();
        let result = emit_diagnostics_to(&out, source, Path::new("src/main.rs"), &mut buf);
        assert!(result.is_err(), "R0001 is an error — must still bail");

        let text = String::from_utf8(buf).expect("utf8 output");
        let doc: serde_json::Value =
            serde_json::from_str(text.trim()).expect("output must be valid JSON");
        assert_eq!(doc["file"], "src/main.rs");
        let rules: Vec<&str> = doc["diagnostics"]
            .as_array()
            .expect("diagnostics array")
            .iter()
            .filter_map(|d| d["rule"].as_str())
            .collect();
        assert!(rules.contains(&"R0001"), "expected R0001 in {rules:?}");
    }

    /// Without the env var, output stays in today's human form.
    #[test]
    fn default_message_format_is_human_lines() {
        let _guard = MessageFormatGuard::set(None);
        let source =
            "#![strict]\nfn main() { let v: Option<i32> = Some(1); let _ = v.unwrap(); }\n";
        let out = trust_lower::lower(source).expect("lowering strict source");
        let mut buf: Vec<u8> = Vec::new();
        let result = emit_diagnostics_to(&out, source, Path::new("src/main.rs"), &mut buf);
        assert!(result.is_err());
        let text = String::from_utf8(buf).expect("utf8 output");
        assert!(
            text.contains("[R0001] error:"),
            "expected human line, got: {text}"
        );
    }

    /// RT-88: files reachable only via `#[cfg(test)] mod x;` are exempt from
    /// force-strict — including transitively through plain `mod` decls in
    /// test-only files. Explicitly-marked or normally-declared files are not.
    #[test]
    fn cfg_test_mod_files_are_detected_transitively() {
        let base = std::env::temp_dir().join(format!("trust-rt88-{}", std::process::id()));
        let src = base.join("src");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("main.rs"),
            "mod shipping;\n#[cfg(test)]\nmod tests;\nfn main() {}\n",
        )
        .unwrap();
        fs::write(src.join("shipping.rs"), "pub fn ship() {}\n").unwrap();
        fs::write(src.join("tests.rs"), "mod helpers;\nfn t() {}\n").unwrap();
        fs::write(src.join("helpers.rs"), "pub fn helper() {}\n").unwrap();

        let test_only = collect_test_only_files(&src);
        let has = |name: &str| {
            test_only
                .iter()
                .any(|p| p.file_name().and_then(|f| f.to_str()) == Some(name))
        };
        assert!(has("tests.rs"), "directly cfg(test)-declared file");
        assert!(has("helpers.rs"), "transitively reached through tests.rs");
        assert!(!has("shipping.rs"), "normal mod stays enforced");
        assert!(!has("main.rs"), "the crate root is never test-only");

        let _ = fs::remove_dir_all(&base);
    }

    /// PR #1 review regression: `#[cfg(not(test))]` (and other negated test
    /// predicates) select PRODUCTION builds and must never be exempted from
    /// force-strict; positive `test` predicates (bare or inside any/all) are.
    #[test]
    fn negated_test_cfgs_are_not_test_only() {
        let base = std::env::temp_dir().join(format!("trust-pr1-{}", std::process::id()));
        let src = base.join("src");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("main.rs"),
            "#[cfg(not(test))]\nmod prod;\n\
             #[cfg(all(unix, not(test)))]\nmod prod_unix;\n\
             #[cfg(all(unix, test))]\nmod unix_tests;\n\
             #[cfg(test)]\nmod tests;\n\
             #[cfg(feature = \"test\")]\nmod feature_named_test;\n\
             fn main() {}\n",
        )
        .unwrap();
        for name in [
            "prod.rs",
            "prod_unix.rs",
            "unix_tests.rs",
            "tests.rs",
            "feature_named_test.rs",
        ] {
            fs::write(src.join(name), "pub fn x() {}\n").unwrap();
        }

        let test_only = collect_test_only_files(&src);
        let has = |name: &str| {
            test_only
                .iter()
                .any(|p| p.file_name().and_then(|f| f.to_str()) == Some(name))
        };
        assert!(!has("prod.rs"), "cfg(not(test)) is a production module");
        assert!(!has("prod_unix.rs"), "all(unix, not(test)) is production");
        assert!(has("unix_tests.rs"), "all(unix, test) is test-only");
        assert!(has("tests.rs"), "plain cfg(test) is test-only");
        assert!(
            !has("feature_named_test.rs"),
            "feature = \"test\" is a feature gate, not the test predicate"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// RT-81: project-level strict applies only to packages the user opted in,
    /// never to dependencies compiled by the same wrapper.
    #[test]
    fn force_strict_is_scoped_by_package_name() {
        // The user's own crate is in the set → forced strict.
        assert!(force_strict_for(Some("my-app"), Some("my-app")));
        // A dependency built in the same `cargo trustc build` carries its own
        // CARGO_PKG_NAME, which is NOT in the set → never force-lowered.
        assert!(!force_strict_for(Some("my-app"), Some("serde")));
        // Multi-package set, with whitespace.
        assert!(force_strict_for(Some("a, b ,c"), Some("b")));
        // Absent set or name is never a match.
        assert!(!force_strict_for(None, Some("my-app")));
        assert!(!force_strict_for(Some("my-app"), None));
        // Empty name must not match an empty element from a trailing comma.
        assert!(!force_strict_for(Some("a,"), Some("")));
    }

    /// RT-75 regression: the cache mirror must COPY non-strict files, not
    /// hard-link them. A hard link shares the inode, so clobbering the cached
    /// copy would truncate the user's original source. This test mirrors a
    /// plain file, clobbers the cached copy, and asserts the source survives.
    #[test]
    fn mirror_copies_rather_than_hardlinks_source() {
        let base = std::env::temp_dir().join(format!("trust-rt75-{}", std::process::id()));
        let src = base.join("src");
        let dest = base.join("cache");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&src).expect("create src");
        let src_file = src.join("plain.rs");
        fs::write(&src_file, "pub fn keep() {}\n").expect("write src");

        let mut visited = std::collections::HashSet::new();
        mirror_module_tree(&src, &dest, &mut visited).expect("mirror");

        // Clobber the cached copy to zero length.
        fs::write(dest.join("plain.rs"), "").expect("clobber cache");

        // The original must be untouched — proving a copy, not a hard link.
        let after = fs::read_to_string(&src_file).expect("read src after");
        assert_eq!(
            after, "pub fn keep() {}\n",
            "source file was corrupted — cache shares an inode with it"
        );
        let _ = fs::remove_dir_all(&base);
    }
}
