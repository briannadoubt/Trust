//! Shared lowering/cache/mirror logic used by the `rustricted-rustc`
//! (`RUSTC_WRAPPER`) and `rustricted-rustdoc` (`RUSTDOC`) shims.
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

    if !rustricted_lower::is_strict_source(&source) {
        return Ok(None);
    }

    let file_name = input_path
        .file_name()
        .context("input path has no file name")?;

    let cache_key = source_cache_key(&source);
    let cache_dir = env::temp_dir()
        .join("rustricted-cache")
        .join(format!("{cache_key:016x}"));
    let cached_file = cache_dir.join(file_name);

    if !cached_file.exists() {
        let src_dir = input_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let mut visited = std::collections::HashSet::new();
        mirror_module_tree(&src_dir, &cache_dir, &mut visited)
            .with_context(|| format!("mirroring src tree from {}", src_dir.display()))?;

        // Defensive: if the src_dir traversal somehow didn't write the
        // crate root (e.g. empty dir), do it directly.
        if !cached_file.exists() {
            let out = rustricted_lower::lower(&source)
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

fn emit_diagnostics(out: &rustricted_lower::LowerOutput, path: &Path) -> Result<()> {
    for diag in &out.diagnostics {
        eprintln!(
            "[{}] {}: {}",
            diag.rule,
            if diag.is_error() { "error" } else { "warning" },
            diag.message
        );
    }
    if out.diagnostics.iter().any(|d| d.is_error()) {
        bail!("rustricted check failed on {}", path.display());
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
            mirror_module_tree(&path, &dest, already_done)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let source =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            if rustricted_lower::is_strict_source(&source) {
                let out = rustricted_lower::lower(&source)
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

/// Lower Rustricted syntax inside doc-test code blocks (`/// ```...```` ` and
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
/// doc-test snippets through `rustricted_lower::lower`.
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
    if let Ok(out) = rustricted_lower::lower(snippet) {
        if !out.diagnostics.iter().any(|d| d.is_error()) {
            return Some(strip_hidden_doctest_prefix(out.source));
        }
    }
    // Strategy 2: wrap as `fn __d() { … }` (snippet is a stmt sequence).
    let wrapped = format!("fn __rustricted_doctest() {{\n{snippet}\n}}\n");
    let out = rustricted_lower::lower(&wrapped).ok()?;
    if out.diagnostics.iter().any(|d| d.is_error()) {
        return None;
    }
    // Strip the wrapper. prettyplease emits a stable shape:
    //     fn __rustricted_doctest() {
    //         <body>
    //     }
    let unwrapped = unwrap_doctest_fn(&out.source)?;
    Some(unwrapped)
}

fn unwrap_doctest_fn(source: &str) -> Option<String> {
    let start = source.find("fn __rustricted_doctest()")?;
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
