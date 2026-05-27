//! Developer tooling. Subcommands:
//!
//! - `cargo xtask gen-docs [--check]` — regenerate auto-marked sections of
//!   `docs/SPEC.md` (or, with `--check`, fail if they would change).
//! - `cargo xtask check-emissions` — verify every implemented `Rule` variant
//!   has at least one emission site in the workspace.

rustricted_attrs::strict! {}

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const LINTS_BEGIN: &str = "<!-- BEGIN auto-generated: lints-table -->";
const LINTS_END: &str = "<!-- END auto-generated: lints-table -->";
const LOWER_BEGIN: &str = "<!-- BEGIN auto-generated: lowering-diagnostics-table -->";
const LOWER_END: &str = "<!-- END auto-generated: lowering-diagnostics-table -->";

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let cmd = args.next().context("usage: cargo xtask <cmd> [--check]")?;
    let rest: Vec<String> = args.collect();
    let check_only = rest.iter().any(|a| a == "--check");

    match cmd.as_str() {
        "gen-docs" => gen_docs(check_only),
        "check-emissions" => check_emissions(),
        other => bail!("unknown subcommand: {other}"),
    }
}

/// Verify that every implemented `Rule` variant across all catalogues has at
/// least one emission site somewhere in `crates/`. Catches "I added a Rule
/// variant and forgot to wire it up."
///
/// An emission is recognised by either form:
/// - typed: `Rule::VariantName.code()` (preferred, type-safe)
/// - literal: `"RXXXX"` (the raw code as a string literal)
///
/// The variant's own catalogue file is excluded from the search so the
/// catalogue entry doesn't count as its own emission.
fn check_emissions() -> Result<()> {
    let root = repo_root()?;
    let crates_dir = root.join("crates");
    let files = collect_rust_files(&crates_dir)?;
    let texts: Vec<(PathBuf, String)> = files
        .into_iter()
        .map(|p| {
            let t = fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            Ok::<_, anyhow::Error>((p, t))
        })
        .collect::<Result<_>>()?;

    let mut failures: Vec<String> = Vec::new();
    let mut checked: usize = 0;

    for r in rustricted_lints::all_rules() {
        if !r.is_implemented() {
            continue;
        }
        check_one(
            code: r.code(),
            variant: &format!("{r:?}"),
            catalogue_suffix: "rustricted-lints/src/rules.rs",
            texts: &texts,
            failures: &mut failures,
        );
        checked += 1;
    }
    for r in rustricted_lower::rule::ALL {
        check_one(
            code: r.code(),
            variant: &format!("{r:?}"),
            catalogue_suffix: "rustricted-lower/src/rule.rs",
            texts: &texts,
            failures: &mut failures,
        );
        checked += 1;
    }
    if !failures.is_empty() {
        for f in &failures {
            eprintln!("error: {f}");
        }
        bail!("{} rule(s) have no emission site", failures.len());
    }

    println!("emissions: all {checked} implemented rules have at least one emission site");
    Ok(())
}

fn check_one(
    code: &str,
    variant: &str,
    catalogue_suffix: &str,
    texts: &[(PathBuf, String)],
    failures: &mut Vec<String>,
) {
    let typed = format!("Rule::{variant}.code()");
    let literal = format!("\"{code}\"");
    for (path, text) in texts {
        if path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with(catalogue_suffix)
        {
            continue;
        }
        if text.contains(&typed) || text.contains(&literal) {
            return;
        }
    }
    failures.push(format!(
        "{code} ({variant}): no emission site found — add `Diagnostic::error(Rule::{variant}.code(), …)` somewhere, or remove the variant"
    ));
}

fn collect_rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_rs(dir: dir, out: &mut out)?;
    Ok(out)
}

fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            walk_rs(dir: &p, out: out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
    Ok(())
}

fn repo_root() -> Result<PathBuf> {
    // CARGO_MANIFEST_DIR is crates/xtask; parent twice is the workspace root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .context("could not derive workspace root from CARGO_MANIFEST_DIR")?
        .to_path_buf();
    Ok(root)
}

fn gen_docs(check_only: bool) -> Result<()> {
    let root = repo_root()?;
    let spec_path = root.join("docs/SPEC.md");
    let original = fs::read_to_string(&spec_path)
        .with_context(|| format!("reading {}", spec_path.display()))?;

    let lints_table = build_lints_table();
    let lowering_table = build_lowering_diags_table();
    let updated = replace_section(input: &original, begin: LINTS_BEGIN, end: LINTS_END, content: &lints_table)?;
    let updated = replace_section(input: &updated, begin: LOWER_BEGIN, end: LOWER_END, content: &lowering_table)?;

    if check_only {
        if updated != original {
            eprintln!("docs/SPEC.md is stale — run `cargo xtask gen-docs` to regenerate",);
            std::process::exit(1);
        }
        println!("docs/SPEC.md is up to date");
        return Ok(());
    }

    if updated == original {
        println!("docs/SPEC.md already up to date");
        return Ok(());
    }

    fs::write(&spec_path, &updated).with_context(|| format!("writing {}", spec_path.display()))?;
    println!("regenerated {}", spec_path.display());
    Ok(())
}

/// Build the non-strict lowering diagnostics table from the `Rule` enum in
/// `rustricted-lower`. Output is sorted by code.
fn build_lowering_diags_table() -> String {
    let mut rows: Vec<(&'static str, &'static str, &'static str, &'static str)> = Vec::new();
    for r in rustricted_lower::rule::ALL {
        rows.push((r.code(), r.pass(), "rustricted-lower", r.message_shape()));
    }
    rows.sort_by_key(|row| row.0);

    let mut out = String::new();
    out.push_str("| Code  | Pass                | Crate                  | Message shape                                       |\n");
    out.push_str("| ----- | ------------------- | ---------------------- | --------------------------------------------------- |\n");
    for (code, pass, crate_name, msg) in rows {
        let crate_quoted = format!("`{crate_name}`");
        out.push_str(&format!(
            "| {code:<5} | {pass:<19} | {crate_quoted:<22} | {msg:<51} |\n",
        ));
    }
    out
}

/// Build the lints catalogue markdown table from `rustricted_lints::all_rules()`.
///
/// Column widths are fixed to keep diffs minimal across runs.
fn build_lints_table() -> String {
    let rules = rustricted_lints::all_rules();
    let mut out = String::new();
    out.push_str("| Code  | Name                  | Phase | Severity |\n");
    out.push_str("| ----- | --------------------- | ----- | -------- |\n");
    for rule in rules {
        out.push_str(&format!(
            "| {:<5} | {:<21} | 1     | error    |\n",
            rule.code(),
            rule.name(),
        ));
    }
    out
}

/// Replace the text between `begin` and `end` marker lines with `content`.
/// Both markers are preserved; the content is sandwiched between them with
/// a leading and trailing blank line so the result is still valid markdown.
fn replace_section(input: &str, begin: &str, end: &str, content: &str) -> Result<String> {
    let begin_idx = input
        .find(begin)
        .with_context(|| format!("missing marker: {begin}"))?;
    let end_idx = input
        .find(end)
        .with_context(|| format!("missing marker: {end}"))?;
    if end_idx < begin_idx {
        bail!("end marker `{end}` appears before begin marker `{begin}`");
    }
    let before = &input[..begin_idx + begin.len()];
    let after = &input[end_idx..];
    Ok(format!("{before}\n\n{content}\n{after}"))
}
