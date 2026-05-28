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

/// FNV-1a 64-bit hash of the lowering-version string concatenated with the
/// source bytes. Fast, no deps, deterministic across processes and OSes.
pub fn source_cache_key(source: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in LOWERING_VERSION.bytes().chain(source.bytes()) {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
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

/// If `input_path` is strict-marked, lower the whole source tree into the
/// cache and return the new root path + a `--remap-path-prefix` flag.
///
/// Returns `Ok(None)` for non-strict sources — the caller should pass the
/// original args through to the underlying tool unchanged.
pub fn prepare_strict_input(input_path: &Path) -> Result<Option<Prepared>> {
    let source = match fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };

    if !trust_lower::is_strict_source(&source) {
        return Ok(None);
    }

    let file_name = input_path
        .file_name()
        .context("input path has no file name")?;

    let cache_key = source_cache_key(&source);
    let cache_dir = env::temp_dir()
        .join("trust-cache")
        .join(format!("{cache_key:016x}"));
    let cached_file = cache_dir.join(file_name);

    if !cached_file.exists() {
        let src_dir = input_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        // RT-40: pre-scan the whole `src/` tree for `fn` definitions so
        // cross-file named-arg call sites resolve. The wrapper is the
        // first place that has a crate-wide view; individual `lower()`
        // calls only see one file at a time.
        let extras = collect_crate_callees(&src_dir);

        let mut visited = std::collections::HashSet::new();
        mirror_module_tree_with_extras(&src_dir, &cache_dir, &mut visited, &extras)
            .with_context(|| format!("mirroring src tree from {}", src_dir.display()))?;

        // Defensive: if the src_dir traversal somehow didn't write the
        // crate root (e.g. empty dir), do it directly.
        if !cached_file.exists() {
            let out = trust_lower::lower_with_extra_callees(&source, &extras)
                .with_context(|| format!("lowering {}", input_path.display()))?;
            emit_diagnostics(&out, input_path)?;
            fs::create_dir_all(&cache_dir)?;
            fs::write(&cached_file, &out.source)?;
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

fn emit_diagnostics(out: &trust_lower::LowerOutput, path: &Path) -> Result<()> {
    for diag in &out.diagnostics {
        eprintln!(
            "[{}] {}: {}",
            diag.rule,
            if diag.is_error() { "error" } else { "warning" },
            diag.message
        );
    }
    if out.diagnostics.iter().any(|d| d.is_error()) {
        bail!("trust check failed on {}", path.display());
    }
    Ok(())
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
        if !already_done.insert(canonical) {
            continue;
        }

        if path.is_dir() {
            mirror_module_tree_with_extras(&path, &dest, already_done, extras)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let source =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            if trust_lower::is_strict_source(&source) {
                let out = trust_lower::lower_with_extra_callees(&source, extras)
                    .with_context(|| format!("lowering {}", path.display()))?;
                emit_diagnostics(&out, &path)?;
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
            } else if fs::hard_link(&path, &dest).is_err() {
                fs::copy(&path, &dest).with_context(|| format!("copying {}", path.display()))?;
            }
        } else if fs::hard_link(&path, &dest).is_err() {
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
